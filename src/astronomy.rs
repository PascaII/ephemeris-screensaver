//! Solar position and offline sunrise/sunset using NOAA/Meeus formulas.
//! The map uses low-precision position (~0.01° accuracy), far below one pixel.

use std::f64::consts::PI;

/// Point on Earth where the sun is directly overhead, in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubsolarPoint {
    pub lat: f64,
    pub lon: f64,
}

/// Julian day for a Unix timestamp (seconds, may be fractional).
pub fn julian_day(unix: f64) -> f64 {
    unix / 86_400.0 + 2_440_587.5
}

pub fn subsolar_point(unix: f64) -> SubsolarPoint {
    let n = julian_day(unix) - 2_451_545.0; // days since J2000.0
    let rad = PI / 180.0;
    let l = (280.460 + 0.985_647_4 * n).rem_euclid(360.0); // mean longitude
    let g = ((357.528 + 0.985_600_3 * n).rem_euclid(360.0)) * rad; // mean anomaly
    let lambda = (l + 1.915 * g.sin() + 0.020 * (2.0 * g).sin()) * rad; // ecliptic longitude
    let eps = (23.439 - 0.000_000_4 * n) * rad; // obliquity of the ecliptic

    let ra = (eps.cos() * lambda.sin()).atan2(lambda.cos()) / rad;
    let dec = (eps.sin() * lambda.sin()).asin() / rad;
    let gmst = (280.460_618_37 + 360.985_647_366_29 * n).rem_euclid(360.0);
    SubsolarPoint { lat: dec, lon: wrap_lon(ra - gmst) }
}

fn wrap_lon(lon: f64) -> f64 {
    (lon + 180.0).rem_euclid(360.0) - 180.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolarState {
    Day,
    Night,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SunTimes {
    pub sunrise: Option<i64>,
    pub sunset: Option<i64>,
    pub polar: Option<PolarState>,
}

/// NOAA/Meeus solar altitude, in degrees. Longitude is positive east.
/// https://gml.noaa.gov/grad/solcalc/calcdetails.html
fn solar_altitude(unix: f64, latitude: f64, longitude: f64) -> f64 {
    let t = (julian_day(unix) - 2_451_545.0) / 36_525.0;
    let l = (280.46646 + t * (36_000.76983 + t * 0.0003032)).rem_euclid(360.0);
    let m = (357.52911 + t * (35_999.05029 - 0.0001537 * t)).to_radians();
    let e = 0.016708634 - t * (0.000042037 + 0.0000001267 * t);
    let c = m.sin() * (1.914602 - t * (0.004817 + 0.000014 * t)) + (2.0 * m).sin() * (0.019993 - 0.000101 * t) + (3.0 * m).sin() * 0.000289;
    let omega = (125.04 - 1934.136 * t).to_radians();
    let lambda = (l + c - 0.00569 - 0.00478 * omega.sin()).to_radians();
    let eps = (23.0 + (26.0 + (21.448 - t * (46.815 + t * (0.00059 - t * 0.001813))) / 60.0) / 60.0 + 0.00256 * omega.cos()).to_radians();
    let decl = (eps.sin() * lambda.sin()).asin();
    let y = (eps / 2.0).tan().powi(2);
    let l = l.to_radians();
    let equation = 4.0
        * (y * (2.0 * l).sin() - 2.0 * e * m.sin() + 4.0 * e * y * m.sin() * (2.0 * l).cos()
            - 0.5 * y * y * (4.0 * l).sin()
            - 1.25 * e * e * (2.0 * m).sin())
        .to_degrees();
    let hour_angle = ((unix.rem_euclid(86_400.0) / 60.0 + equation + 4.0 * longitude) / 4.0 - 180.0).to_radians();
    let lat = latitude.to_radians();
    (lat.sin() * decl.sin() + lat.cos() * decl.cos() * hour_angle.cos()).clamp(-1.0, 1.0).asin().to_degrees()
}

/// Events within a city's local calendar day, expressed as UTC seconds. The caller supplies
/// timezone-aware midnight boundaries: a DST day can contain 23 or 25 hours. A minute-spaced
/// scan brackets horizon crossings, then bisection refines them to subsecond precision.
/// Standard upper-limb sunrise uses a -0.833° centre altitude, with a flat horizon.
pub fn sun_times(start: i64, end: i64, latitude: f64, longitude: f64) -> SunTimes {
    let height = |t: f64| solar_altitude(t, latitude, longitude) + 0.833;
    let mut result = SunTimes { sunrise: None, sunset: None, polar: None };
    let mut left = start as f64;
    let mut previous = height(left);
    let mut all_day = previous >= 0.0;
    let mut all_night = previous < 0.0;
    while left < end as f64 {
        let right = (left + 60.0).min(end as f64);
        let current = height(right);
        all_day &= current >= 0.0;
        all_night &= current < 0.0;
        if (previous < 0.0) != (current < 0.0) {
            let rising = previous < 0.0;
            let (mut a, mut b) = (left, right);
            for _ in 0..20 {
                let mid = (a + b) / 2.0;
                if (height(mid) < 0.0) == rising {
                    a = mid;
                } else {
                    b = mid;
                }
            }
            let crossing = ((a + b) / 2.0).round() as i64;
            if crossing >= start && crossing < end {
                if rising {
                    result.sunrise = Some(crossing);
                } else {
                    result.sunset = Some(crossing);
                }
            }
        }
        left = right;
        previous = current;
    }
    if all_day {
        result.polar = Some(PolarState::Day);
    }
    if all_night {
        result.polar = Some(PolarState::Night);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    fn timestamp(s: &str) -> i64 {
        s.parse::<jiff::Timestamp>().unwrap().as_second()
    }

    #[test]
    fn zurich_matches_independent_usno_references() {
        // US Naval Observatory API v4.0.1, retrieved 2026-10-08, coordinates 47.3769,8.5417.
        // https://aa.usno.navy.mil/api/rstt/oneday?date=2026-06-21&coords=47.3769%2C8.5417&tz=2
        // https://aa.usno.navy.mil/api/rstt/oneday?date=2026-12-21&coords=47.3769%2C8.5417&tz=1
        for (start, end, rise, set) in [
            ("2026-06-20T22:00:00Z", "2026-06-21T22:00:00Z", "2026-06-21T03:29:00Z", "2026-06-21T19:26:00Z"),
            ("2026-12-20T23:00:00Z", "2026-12-21T23:00:00Z", "2026-12-21T07:10:00Z", "2026-12-21T15:38:00Z"),
        ] {
            let events = sun_times(timestamp(start), timestamp(end), 47.3769, 8.5417);
            assert!((events.sunrise.unwrap() - timestamp(rise)).abs() <= 60, "{events:?}");
            assert!((events.sunset.unwrap() - timestamp(set)).abs() <= 60, "{events:?}");
            assert_eq!(events.polar, None);
        }
    }

    #[test]
    fn polar_day_and_night() {
        for (date, expected) in [("2026-06-21T00:00:00Z", PolarState::Day), ("2026-12-21T00:00:00Z", PolarState::Night)] {
            let start = timestamp(date);
            let events = sun_times(start, start + 86_400, 78.2232, 15.6469);
            assert_eq!(events, SunTimes { sunrise: None, sunset: None, polar: Some(expected) });
        }
    }

    #[test]
    fn distant_timezone_and_dst_day_boundaries() {
        for (name, date, hours, lat, lon) in [
            ("Europe/Zurich", "2026-03-29", 23, 47.3769, 8.5417),
            ("Europe/Zurich", "2026-10-25", 25, 47.3769, 8.5417),
            ("Pacific/Auckland", "2026-06-21", 24, -36.8485, 174.7633),
        ] {
            let zone = jiff::tz::TimeZone::get(name).unwrap();
            let date: jiff::civil::Date = date.parse().unwrap();
            let start = date.at(0, 0, 0, 0).to_zoned(zone.clone()).unwrap().timestamp().as_second();
            let end = date.tomorrow().unwrap().at(0, 0, 0, 0).to_zoned(zone.clone()).unwrap().timestamp().as_second();
            assert_eq!(end - start, hours * 3600);
            let events = sun_times(start, end, lat, lon);
            for event in [events.sunrise.unwrap(), events.sunset.unwrap()] {
                assert!((start..end).contains(&event));
                assert_eq!(jiff::Timestamp::from_second(event).unwrap().to_zoned(zone.clone()).date(), date);
            }
        }
    }

    #[test]
    fn june_solstice_noon_utc() {
        // 2026-06-21 12:00:00 UTC: declination ≈ +23.44°, equation of time ≈ -1.6 min: solar noon at Greenwich is 12:01.6, so lon ≈ +0.4°.
        let p = subsolar_point(1_782_043_200.0);
        assert!(close(p.lat, 23.44, 0.05), "{p:?}");
        assert!(close(p.lon, 0.4, 0.3), "{p:?}");
    }

    #[test]
    fn december_solstice() {
        // 2025-12-21 12:00:00 UTC: declination ≈ -23.44°, equation of time ≈ +1.9 min: the sun already passed Greenwich, lon ≈ -0.5°.
        let p = subsolar_point(1_766_318_400.0);
        assert!(close(p.lat, -23.44, 0.05), "{p:?}");
        assert!(close(p.lon, -0.5, 0.3), "{p:?}");
    }

    #[test]
    fn march_equinox_and_rotation() {
        // 2026-03-20 14:46 UTC is the equinox: declination ≈ 0.
        let t = 1_774_017_960.0;
        let p = subsolar_point(t);
        assert!(close(p.lat, 0.0, 0.05), "{p:?}");
        // Six hours later the subsolar point has moved ~90° west.
        let q = subsolar_point(t + 6.0 * 3600.0);
        assert!(close(wrap_lon(p.lon - q.lon), 90.0, 0.2), "{p:?} {q:?}");
    }
}
