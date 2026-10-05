//! Map pass: one fullscreen triangle, everything else happens in `map.frag`.

use super::gl_util;
use crate::astronomy::SubsolarPoint;
use glow::HasContext;

static LAND_SDF: &[u8] = include_bytes!("../../assets/land_sdf.png");
static LIGHTS: &[u8] = include_bytes!("../../assets/lights.png");

/// Northern edge of the visible map. Miller y-coordinate of 84°N.
pub const TOP_LAT: f64 = 84.0;
const MAX_LAT: f64 = 85.0;

pub struct MapPass {
    program: glow::Program,
    vao: glow::VertexArray,
    land: glow::Texture,
    lights: glow::Texture,
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
}

impl MapPass {
    /// `screen_width` limits the night-lights texture: no point keeping 4096 px for a 1080p screen.
    pub unsafe fn new(gl: &glow::Context, screen_width: u32) -> Self {
        let program = gl_util::program(gl, include_str!("shaders/fullscreen.vert"), include_str!("shaders/map.frag"));
        let land = gl_util::gray_png_texture(gl, LAND_SDF, false, u32::MAX);
        let lights = gl_util::gray_png_texture(gl, LIGHTS, true, screen_width.max(1024));
        gl.use_program(Some(program));
        gl.uniform_1_i32(gl.get_uniform_location(program, "u_land").as_ref(), 0);
        gl.uniform_1_i32(gl.get_uniform_location(program, "u_lights").as_ref(), 1);
        MapPass {
            vao: gl.create_vertex_array().expect("vao"),
            u_res: gl.get_uniform_location(program, "u_res"),
            u_view: gl.get_uniform_location(program, "u_view"),
            u_sun: gl.get_uniform_location(program, "u_sun"),
            program,
            land,
            lights,
        }
    }

    pub unsafe fn draw(&self, gl: &glow::Context, view: &View, sun: SubsolarPoint) {
        gl.use_program(Some(self.program));
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(self.land));
        gl.active_texture(glow::TEXTURE1);
        gl.bind_texture(glow::TEXTURE_2D, Some(self.lights));
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
