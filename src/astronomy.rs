//! Solar position. Low-precision NOAA/Meeus formulas (~0.01° accuracy), which is far below one pixel.

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

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
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
