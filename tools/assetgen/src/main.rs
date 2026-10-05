//! Offline asset preprocessing. Downloads public-domain source data once into `tools/assetgen/cache`
//! and writes small, render-ready assets into `assets/`:
//!
//! - `land_sdf.png`   2048x1024 R8 signed distance field of land (equirectangular), 128 = coastline
//! - `lights.png`     4096x2048 R8 NASA Black Marble 2016 night lights (equirectangular)
//! - `gazetteer.tsv`  countries + populated places with DE/EN names for headline geolocation
//! - `Inter-*.otf`    Latin subset of the Inter font (if `uvx` is available for fonttools)

use image::{imageops::FilterType, GrayImage, Luma};
use serde_json::Value;
use std::{fs, path::Path, process::Command};

const NE: &str = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson";
const BLACK_MARBLE: &str = "https://assets.science.nasa.gov/content/dam/science/esd/eo/images/imagerecords/144000/144897/BlackMarble_2016_3km_gray.jpg";
const INTER: &str = "https://github.com/rsms/inter/raw/v3.19/docs/font-files";

/// Hi-res raster used to build the SDF; the output is downsampled by `SDF_SCALE`.
const HI_W: usize = 8192;
const HI_H: usize = 4096;
const SDF_SCALE: usize = 4;
/// SDF encoding: output value = 128 + distance_in_output_px * SDF_STEPS (land positive).
const SDF_STEPS: f32 = 8.0;

/// Minimum population for a city to be considered (capitals and major cities always included).
const MIN_CITY_POP: f64 = 150_000.0;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cache = root.join("cache");
    let out = root.join("../../assets");
    fs::create_dir_all(&cache).unwrap();
    fs::create_dir_all(&out).unwrap();

    let land = fetch(&cache, &format!("{NE}/ne_50m_land.geojson"), "ne_50m_land.geojson");
    let places = fetch(&cache, &format!("{NE}/ne_10m_populated_places.geojson"), "ne_10m_populated_places.geojson");
    let countries = fetch(&cache, &format!("{NE}/ne_50m_admin_0_countries.geojson"), "ne_50m_admin_0_countries.geojson");
    let lights = fetch(&cache, BLACK_MARBLE, "BlackMarble_2016_3km_gray.jpg");

    land_sdf(&land, &out.join("land_sdf.png"));
    night_lights(&lights, &out.join("lights.png"));
    gazetteer(&countries, &places, &out.join("gazetteer.tsv"));
    for weight in ["Regular", "Medium"] {
        let name = format!("Inter-{weight}.otf");
        let src = fetch(&cache, &format!("{INTER}/{name}"), &name);
        subset_font(&src, &out.join(&name));
    }
}

fn fetch(cache: &Path, url: &str, name: &str) -> std::path::PathBuf {
    let path = cache.join(name);
    if !path.exists() {
        eprintln!("downloading {url}");
        let ok = Command::new("curl").args(["-sfL", "-o"]).arg(&path).arg(url).status().map(|s| s.success());
        assert!(matches!(ok, Ok(true)), "download failed: {url}");
    }
    path
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

// ---------------------------------------------------------------------------------------------
// Land SDF

fn land_sdf(src: &Path, dst: &Path) {
    let geo = read_json(src);
    let mut rings: Vec<Vec<(f64, f64)>> = Vec::new();
    for f in geo["features"].as_array().unwrap() {
        let g = &f["geometry"];
        let polys: Vec<&Value> = match g["type"].as_str().unwrap() {
            "Polygon" => vec![&g["coordinates"]],
            "MultiPolygon" => g["coordinates"].as_array().unwrap().iter().collect(),
            _ => continue,
        };
        for poly in polys {
            for ring in poly.as_array().unwrap() {
                rings.push(
                    ring.as_array()
                        .unwrap()
                        .iter()
                        .map(|p| {
                            let lon = p[0].as_f64().unwrap();
                            let lat = p[1].as_f64().unwrap();
                            ((lon + 180.0) / 360.0 * HI_W as f64, (90.0 - lat) / 180.0 * HI_H as f64)
                        })
                        .collect(),
                );
            }
        }
    }

    // Even-odd scanline fill of all rings (holes are inner rings, so even-odd handles them).
    let mut mask = vec![false; HI_W * HI_H];
    let mut xs: Vec<f64> = Vec::new();
    for y in 0..HI_H {
        let sy = y as f64 + 0.5;
        xs.clear();
        for ring in &rings {
            for w in ring.windows(2) {
                let ((x0, y0), (x1, y1)) = (w[0], w[1]);
                if (y0 <= sy) != (y1 <= sy) {
                    xs.push(x0 + (sy - y0) / (y1 - y0) * (x1 - x0));
                }
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for pair in xs.chunks(2) {
            if let [a, b] = pair {
                let start = (a - 0.5).ceil().max(0.0) as usize;
                let end = ((b - 0.5).floor() as isize).min(HI_W as isize - 1);
                for x in start as isize..=end {
                    mask[y * HI_W + x as usize] = true;
                }
            }
        }
    }
    // Antarctica's ring in Natural Earth runs along the map edge; make sure the polar rows are land.
    for y in HI_H - 4..HI_H {
        mask[y * HI_W..(y + 1) * HI_W].fill(true);
    }

    let to_sea = edt(&mask, true);
    let to_land = edt(&mask, false);
    let (w, h) = (HI_W / SDF_SCALE, HI_H / SDF_SCALE);
    let mut img = GrayImage::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            // Sample at the centre of the output pixel's footprint.
            let (hx, hy) = (x * SDF_SCALE + SDF_SCALE / 2, y * SDF_SCALE + SDF_SCALE / 2);
            let i = hy * HI_W + hx;
            let d = if mask[i] { to_sea[i].sqrt() - 0.5 } else { -(to_land[i].sqrt() - 0.5) };
            let v = 128.0 + d / SDF_SCALE as f32 * SDF_STEPS;
            img.put_pixel(x as u32, y as u32, Luma([v.round().clamp(0.0, 255.0) as u8]));
        }
    }
    img.save(dst).unwrap();
    eprintln!("wrote {} ({} KB)", dst.display(), fs::metadata(dst).unwrap().len() / 1024);
}

/// Squared Euclidean distance (in pixels) from every pixel to the nearest pixel whose mask value
/// differs from `inside` (Felzenszwalb & Huttenlocher). Horizontal pass wraps around the antimeridian.
fn edt(mask: &[bool], inside: bool) -> Vec<f32> {
    const INF: f32 = 1e20;
    let mut grid: Vec<f32> = mask.iter().map(|&m| if m == inside { INF } else { 0.0 }).collect();
    let mut f = vec![0f32; HI_W.max(HI_H) * 3];
    let mut d = vec![0f32; HI_W.max(HI_H) * 3];
    // Columns.
    for x in 0..HI_W {
        for y in 0..HI_H {
            f[y] = grid[y * HI_W + x];
        }
        edt_1d(&f[..HI_H], &mut d[..HI_H]);
        for y in 0..HI_H {
            grid[y * HI_W + x] = d[y];
        }
    }
    // Rows, tripled so distances wrap across ±180°.
    for y in 0..HI_H {
        let row = &grid[y * HI_W..(y + 1) * HI_W];
        for k in 0..3 {
            f[k * HI_W..(k + 1) * HI_W].copy_from_slice(row);
        }
        edt_1d(&f[..3 * HI_W], &mut d[..3 * HI_W]);
        grid[y * HI_W..(y + 1) * HI_W].copy_from_slice(&d[HI_W..2 * HI_W]);
    }
    grid
}

fn edt_1d(f: &[f32], d: &mut [f32]) {
    let n = f.len();
    let mut v = vec![0usize; n];
    let mut z = vec![0f32; n + 1];
    let mut k = 0;
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;
    for q in 1..n {
        loop {
            let p = v[k];
            let s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * q as f32 - 2.0 * p as f32);
            if s <= z[k] && k > 0 {
                k -= 1;
                continue;
            }
            if s <= z[k] {
                // k == 0: replace the first parabola.
                v[0] = q;
                z[1] = f32::INFINITY;
                break;
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = f32::INFINITY;
            break;
        }
    }
    k = 0;
    for q in 0..n {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let p = v[k];
        let dq = q as f32 - p as f32;
        d[q] = dq * dq + f[p];
    }
}

// ---------------------------------------------------------------------------------------------
// Night lights

fn night_lights(src: &Path, dst: &Path) {
    let img = image::open(src).unwrap().into_luma8();
    let small = image::imageops::resize(&img, 4096, 2048, FilterType::Triangle);
    // Clamp the faint noise floor to pure black: invisible on screen, but it compresses far better.
    let small = GrayImage::from_fn(small.width(), small.height(), |x, y| {
        let v = small.get_pixel(x, y)[0];
        Luma([if v < 8 { 0 } else { v }])
    });
    small.save(dst).unwrap();
    eprintln!("wrote {} ({} KB)", dst.display(), fs::metadata(dst).unwrap().len() / 1024);
}

// ---------------------------------------------------------------------------------------------
// Gazetteer

/// Normalise a name for the gazetteer: trim, collapse whitespace, ß -> ss (NZZ uses Swiss spelling).
fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").replace('ß', "ss")
}

fn names(p: &Value, keys: &[&str]) -> String {
    let mut out: Vec<String> = Vec::new();
    for k in keys {
        if let Some(s) = p[*k].as_str() {
            let n = norm(s);
            if !n.is_empty() && !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out.join("|")
}

fn gazetteer(countries: &Path, places: &Path, dst: &Path) {
    let mut lines = vec![
        "# kind\tiso2\tlat\tlon\tpopulation\tnames (| separated). Generated by tools/assetgen from Natural Earth (public domain).".to_string(),
    ];
    for f in read_json(countries)["features"].as_array().unwrap() {
        let p = &f["properties"];
        let iso = p["ISO_A2_EH"].as_str().unwrap_or("-");
        lines.push(format!(
            "C\t{iso}\t{:.2}\t{:.2}\t{}\t{}",
            p["LABEL_Y"].as_f64().unwrap(),
            p["LABEL_X"].as_f64().unwrap(),
            p["POP_EST"].as_f64().unwrap_or(0.0) as u64,
            names(p, &["NAME", "NAME_EN", "NAME_DE", "NAME_LONG", "ADMIN"])
        ));
    }
    let mut n = 0;
    for f in read_json(places)["features"].as_array().unwrap() {
        let p = &f["properties"];
        let pop = p["POP_MAX"].as_f64().unwrap_or(0.0);
        let capital = p["ADM0CAP"].as_f64().unwrap_or(0.0) > 0.0;
        let major = p["SCALERANK"].as_f64().unwrap_or(10.0) <= 3.0;
        if pop < MIN_CITY_POP && !capital && !major {
            continue;
        }
        n += 1;
        lines.push(format!(
            "P\t{}\t{:.3}\t{:.3}\t{}\t{}",
            p["ISO_A2"].as_str().unwrap_or("-"),
            p["LATITUDE"].as_f64().unwrap(),
            p["LONGITUDE"].as_f64().unwrap(),
            pop as u64,
            names(p, &["NAME", "NAMEASCII", "NAME_EN", "NAME_DE"])
        ));
    }
    fs::write(dst, lines.join("\n") + "\n").unwrap();
    eprintln!("wrote {} ({n} places, {} KB)", dst.display(), fs::metadata(dst).unwrap().len() / 1024);
}

// ---------------------------------------------------------------------------------------------
// Font

fn subset_font(src: &Path, dst: &Path) {
    // Basic Latin, Latin-1, Latin Extended-A, general punctuation, arrows, middle dot.
    let ok = Command::new("uvx")
        .args(["--from", "fonttools", "pyftsubset"])
        .arg(src)
        .arg(format!("--output-file={}", dst.display()))
        .args(["--unicodes=U+0020-007E,U+00A0-017F,U+2010-2027,U+2030-203A,U+2190-2193,U+20AC", "--layout-features=kern,tnum", "--no-hinting"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("uvx/fonttools not available, copying full font");
        fs::copy(src, dst).unwrap();
    }
    eprintln!("wrote {} ({} KB)", dst.display(), fs::metadata(dst).unwrap().len() / 1024);
}
