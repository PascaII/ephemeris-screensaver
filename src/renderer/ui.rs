//! Immediate-mode 2D batch: rounded rects, discs, rings, glows and text in one draw call.

use super::gl_util;
use super::text::{Fonts, Weight};
use glow::HasContext;

pub type Color = [f32; 4];

const FLOATS_PER_VERTEX: usize = 12;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}

#[derive(Clone, Copy)]
pub struct TextStyle {
    pub weight: Weight,
    pub size: f32,
    pub color: Color,
    /// Extra spacing between letters, in pixels.
    pub tracking: f32,
}

pub struct Ui {
    program: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    u_res: Option<glow::UniformLocation>,
    verts: Vec<f32>,
    pub fonts: Fonts,
}

impl Ui {
    pub unsafe fn new(gl: &glow::Context) -> Self {
        let program = gl_util::program(gl, include_str!("shaders/ui.vert"), include_str!("shaders/ui.frag"));
        let vao = gl.create_vertex_array().expect("vao");
        let vbo = gl.create_buffer().expect("vbo");
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        let stride = (FLOATS_PER_VERTEX * 4) as i32;
        for (loc, size, offset) in [(0, 2, 0), (1, 2, 2), (2, 4, 4), (3, 4, 8)] {
            gl.enable_vertex_attrib_array(loc);
            gl.vertex_attrib_pointer_f32(loc, size, glow::FLOAT, false, stride, offset * 4);
        }
        gl.use_program(Some(program));
        gl.uniform_1_i32(gl.get_uniform_location(program, "u_atlas").as_ref(), 0);
        Ui {
            u_res: gl.get_uniform_location(program, "u_res"),
            program,
            vao,
            vbo,
            verts: Vec::with_capacity(16 * 1024),
            fonts: Fonts::new(gl),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn quad(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, uv0: [f32; 2], uv1: [f32; 2], color: Color, param: [f32; 4]) {
        let corners = [(x0, y0, uv0[0], uv0[1]), (x1, y0, uv1[0], uv0[1]), (x1, y1, uv1[0], uv1[1]), (x0, y1, uv0[0], uv1[1])];
        for i in [0, 1, 2, 0, 2, 3] {
            let (x, y, u, v) = corners[i];
            self.verts.extend_from_slice(&[x, y, u, v]);
            self.verts.extend_from_slice(&color);
            self.verts.extend_from_slice(&param);
        }
    }

    pub fn rect(&mut self, r: Rect, radius: f32, color: Color) {
        let (hw, hh) = (r.w / 2.0, r.h / 2.0);
        self.quad(r.x - 1.0, r.y - 1.0, r.x + r.w + 1.0, r.y + r.h + 1.0, [-hw - 1.0, -hh - 1.0], [hw + 1.0, hh + 1.0], color, [0.0, hw, hh, radius]);
    }

    fn centered(&mut self, cx: f32, cy: f32, extent: f32, color: Color, param: [f32; 4]) {
        self.quad(cx - extent, cy - extent, cx + extent, cy + extent, [-extent, -extent], [extent, extent], color, param);
    }

    pub fn disc(&mut self, cx: f32, cy: f32, r: f32, color: Color) {
        self.centered(cx, cy, r + 1.5, color, [2.0, r, 0.0, 0.0]);
    }

    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, thickness: f32, color: Color) {
        self.centered(cx, cy, r + thickness + 1.5, color, [3.0, r, thickness, 0.0]);
    }

    pub fn glow(&mut self, cx: f32, cy: f32, sigma: f32, color: Color) {
        self.centered(cx, cy, sigma * 3.0, color, [4.0, sigma, 0.0, 0.0]);
    }

    /// Draw `s` with its baseline at `y`; returns the advance width.
    pub fn text(&mut self, gl: &glow::Context, style: TextStyle, x: f32, y: f32, s: &str) -> f32 {
        let mut pen = x;
        let mut prev = None;
        for c in s.chars() {
            if let Some(p) = prev {
                pen += self.fonts.kern(style.weight, style.size, p, c);
            }
            let g = self.fonts.glyph(gl, style.weight, style.size, c);
            if g.w > 0.0 {
                // Snap to whole pixels so small text stays crisp.
                let gx = (pen + g.xmin).round();
                let gy = (y - g.h - g.ymin).round();
                self.quad(gx, gy, gx + g.w, gy + g.h, [g.uv[0], g.uv[1]], [g.uv[2], g.uv[3]], style.color, [1.0, 0.0, 0.0, 0.0]);
            }
            pen += g.advance + style.tracking;
            prev = Some(c);
        }
        pen - x
    }

    pub fn flush(&mut self, gl: &glow::Context, width: f32, height: f32) {
        if self.verts.is_empty() {
            return;
        }
        unsafe {
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(self.program));
            gl.uniform_2_f32(self.u_res.as_ref(), width, height);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.fonts.texture));
            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            let bytes = std::slice::from_raw_parts(self.verts.as_ptr() as *const u8, self.verts.len() * 4);
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STREAM_DRAW);
            gl.draw_arrays(glow::TRIANGLES, 0, (self.verts.len() / FLOATS_PER_VERTEX) as i32);
            gl.disable(glow::BLEND);
        }
        self.verts.clear();
    }
}
