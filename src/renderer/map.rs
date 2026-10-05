//! Map pass: one fullscreen triangle, everything else happens in `map.frag`.

use super::gl_util;
use crate::astronomy::SubsolarPoint;
use glow::HasContext;

static LAND_SDF: &[u8] = include_bytes!("../../assets/land_sdf.png");
static LIGHTS: &[u8] = include_bytes!("../../assets/lights.png");
static MARBLE: &[u8] = include_bytes!("../../assets/bluemarble.png");

/// Northern edge of the visible map. Miller y-coordinate of 84°N.
pub const TOP_LAT: f64 = 84.0;
const MAX_LAT: f64 = 85.0;

pub struct MapPass {
    program: glow::Program,
    vao: glow::VertexArray,
    land: glow::Texture,
    lights: glow::Texture,
    marble: glow::Texture,
    u_res: Option<glow::UniformLocation>,
    u_view: Option<glow::UniformLocation>,
    u_sun: Option<glow::UniformLocation>,
}

/// How the map is laid out on screen: width-filling Miller projection anchored at `TOP_LAT`.
/// On screens taller than the projection allows, the map is centred vertically instead.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub y_top: f64,
    pub y_span: f64,
    pub center_lon: f64,
    /// Horizontal letterbox in pixels (only for extremely wide screens).
    pub x_pad: f64,
    pub width: f64,
    pub height: f64,
}

pub fn miller_y(lat_deg: f64) -> f64 {
    let lat = lat_deg.to_radians();
    1.25 * (std::f64::consts::FRAC_PI_4 + 0.4 * lat).tan().ln()
}

impl View {
    pub fn new(width: f64, height: f64, center_lon_deg: f64) -> Self {
        let max_y = miller_y(MAX_LAT);
        let mut x_pad = 0.0;
        let mut y_span = 2.0 * std::f64::consts::PI * height / width;
        if y_span > 2.0 * max_y {
            y_span = 2.0 * max_y;
        }
        let mut y_top = miller_y(TOP_LAT).min(max_y);
        if y_top - y_span < -max_y {
            y_top = y_span / 2.0; // tall screen: centre on the equator
        }
        // Keep the full longitude range visible; pad horizontally only if the map can't fill height.
        let map_w = 2.0 * std::f64::consts::PI * height / y_span;
        if map_w < width {
            x_pad = (width - map_w) / 2.0;
        }
        View { y_top, y_span, center_lon: center_lon_deg.to_radians(), x_pad, width, height }
    }

    /// Geographic coordinates (degrees) to window pixels (origin top-left).
    pub fn project(&self, lat: f64, lon: f64) -> (f64, f64) {
        let map_w = self.width - 2.0 * self.x_pad;
        let dlon = (lon.to_radians() - self.center_lon + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI)
            - std::f64::consts::PI;
        let x = self.x_pad + (dlon / (2.0 * std::f64::consts::PI) + 0.5) * map_w;
        let y = (self.y_top - miller_y(lat.clamp(-MAX_LAT, MAX_LAT))) / self.y_span * self.height;
        (x, y)
    }

    /// Window pixels to geographic coordinates (degrees); `None` outside the map.
    pub fn unproject(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let map_w = self.width - 2.0 * self.x_pad;
        if x < self.x_pad || x > self.width - self.x_pad {
            return None;
        }
        let my = self.y_top - y / self.height * self.y_span;
        let lat = ((my / 1.25).exp().atan() - std::f64::consts::FRAC_PI_4) / 0.4;
        let lon = self.center_lon + ((x - self.x_pad) / map_w - 0.5) * 2.0 * std::f64::consts::PI;
        Some((lat.to_degrees(), lon.to_degrees()))
    }
}

/// CPU copy of the land mask (1024 x 512, 0 = sea, 255 = land), used to keep the callout over water.
pub struct LandMask {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl LandMask {
    pub fn load() -> Self {
        let mut reader = png::Decoder::new(LAND_SDF).read_info().expect("png header");
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).expect("png data");
        let (sw, sh) = (info.width as usize, info.height as usize);
        let (w, h) = (sw / 2, sh / 2);
        let mut px = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let land = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().filter(|(dx, dy)| buf[(2 * y + dy) * sw + 2 * x + dx] >= 128).count();
                px[y * w + x] = (land * 255 / 4) as u8;
            }
        }
        LandMask { w, h, px }
    }

    fn at(&self, lat: f64, lon: f64) -> f32 {
        let u = ((lon / 360.0 + 0.5).rem_euclid(1.0) * self.w as f64) as usize % self.w;
        let v = (((0.5 - lat / 180.0) * self.h as f64) as usize).min(self.h - 1);
        self.px[v * self.w + u] as f32 / 255.0
    }
}

/// Land coverage of the screen in square cells, with an integral image so the land fraction under
/// any rectangle costs four lookups. Rebuilt when the window size changes.
#[derive(Default)]
pub struct LandGrid {
    cell: f32,
    cols: usize,
    rows: usize,
    /// (cols + 1) x (rows + 1) summed-area table of per-cell coverage.
    sum: Vec<f32>,
}

impl LandGrid {
    pub fn new(mask: &LandMask, view: &View, cell: f32) -> Self {
        let cols = (view.width as f32 / cell).ceil() as usize;
        let rows = (view.height as f32 / cell).ceil() as usize;
        let mut sum = vec![0.0; (cols + 1) * (rows + 1)];
        for r in 0..rows {
            let mut acc = 0.0;
            for c in 0..cols {
                // 2x2 samples per cell.
                let mut v = 0.0;
                for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    let (x, y) = ((c as f32 + sx) * cell, (r as f32 + sy) * cell);
                    v += view.unproject(x as f64, y as f64).map_or(0.0, |(lat, lon)| mask.at(lat, lon));
                }
                acc += v / 4.0;
                sum[(r + 1) * (cols + 1) + c + 1] = sum[r * (cols + 1) + c + 1] + acc;
            }
        }
        LandGrid { cell, cols, rows, sum }
    }

    /// Fraction (0..1) of the rectangle that is land, at cell resolution.
    pub fn fraction(&self, x: f32, y: f32, w: f32, h: f32) -> f32 {
        if self.sum.is_empty() {
            return 0.0;
        }
        let c0 = ((x / self.cell).round().max(0.0) as usize).min(self.cols);
        let c1 = (((x + w) / self.cell).round().max(0.0) as usize).min(self.cols);
        let r0 = ((y / self.cell).round().max(0.0) as usize).min(self.rows);
        let r1 = (((y + h) / self.cell).round().max(0.0) as usize).min(self.rows);
        let n = ((c1 - c0) * (r1 - r0)) as f32;
        if n == 0.0 {
            return 0.0;
        }
        let s = |r: usize, c: usize| self.sum[r * (self.cols + 1) + c];
        (s(r1, c1) - s(r0, c1) - s(r1, c0) + s(r0, c0)) / n
    }
}

impl MapPass {
    /// `screen_width` limits the night-lights texture: no point keeping 4096 px for a 1080p screen.
    pub unsafe fn new(gl: &glow::Context, screen_width: u32) -> Self {
        let program = gl_util::program(gl, include_str!("shaders/fullscreen.vert"), include_str!("shaders/map.frag"));
        let land = gl_util::gray_png_texture(gl, LAND_SDF, false, u32::MAX);
        let lights = gl_util::gray_png_texture(gl, LIGHTS, true, screen_width.max(1024));
        let marble = gl_util::rgb_png_texture(gl, MARBLE);
        gl.use_program(Some(program));
        gl.uniform_1_i32(gl.get_uniform_location(program, "u_land").as_ref(), 0);
        gl.uniform_1_i32(gl.get_uniform_location(program, "u_lights").as_ref(), 1);
        gl.uniform_1_i32(gl.get_uniform_location(program, "u_marble").as_ref(), 2);
        MapPass {
            vao: gl.create_vertex_array().expect("vao"),
            u_res: gl.get_uniform_location(program, "u_res"),
            u_view: gl.get_uniform_location(program, "u_view"),
            u_sun: gl.get_uniform_location(program, "u_sun"),
            program,
            land,
            lights,
            marble,
        }
    }

    pub unsafe fn draw(&self, gl: &glow::Context, view: &View, sun: SubsolarPoint) {
        gl.use_program(Some(self.program));
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(self.land));
        gl.active_texture(glow::TEXTURE1);
        gl.bind_texture(glow::TEXTURE_2D, Some(self.lights));
        gl.active_texture(glow::TEXTURE2);
        gl.bind_texture(glow::TEXTURE_2D, Some(self.marble));
        gl.uniform_2_f32(self.u_res.as_ref(), view.width as f32, view.height as f32);
        gl.uniform_4_f32(
            self.u_view.as_ref(),
            view.y_top as f32,
            view.y_span as f32,
            view.center_lon as f32,
            view.x_pad as f32,
        );
        gl.uniform_2_f32(self.u_sun.as_ref(), sun.lat.to_radians() as f32, sun.lon.to_radians() as f32);
        gl.bind_vertex_array(Some(self.vao));
        gl.draw_arrays(glow::TRIANGLES, 0, 3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn land_grid_tells_sea_from_land() {
        let view = View::new(1920.0, 1080.0, 0.0);
        let grid = LandGrid::new(&LandMask::load(), &view, 16.0);
        let around = |lat: f64, lon: f64| {
            let (x, y) = view.project(lat, lon);
            grid.fraction(x as f32 - 60.0, y as f32 - 40.0, 120.0, 80.0)
        };
        assert!(around(25.0, 10.0) > 0.95, "Sahara is land");
        assert!(around(30.0, -40.0) < 0.05, "mid-Atlantic is sea");
        assert!(around(-25.0, -120.0) < 0.05, "south Pacific is sea");
    }

    #[test]
    fn unproject_inverts_project() {
        let view = View::new(1920.0, 1080.0, 0.0);
        let (x, y) = view.project(47.37, 8.54);
        let (lat, lon) = view.unproject(x, y).unwrap();
        assert!((lat - 47.37).abs() < 1e-6 && (lon - 8.54).abs() < 1e-6);
    }
}
