//! Text: two embedded Inter weights rasterised on demand into one R8 glyph atlas.

use glow::HasContext;
use std::collections::HashMap;

static REGULAR: &[u8] = include_bytes!("../../assets/Inter-Regular.otf");
static MEDIUM: &[u8] = include_bytes!("../../assets/Inter-Medium.otf");

const ATLAS: i32 = 1024;
const PAD: i32 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular = 0,
    Medium = 1,
}

#[derive(Clone, Copy)]
pub struct Glyph {
    /// Atlas uv rectangle (u0, v0, u1, v1).
    pub uv: [f32; 4],
    pub w: f32,
    pub h: f32,
    pub xmin: f32,
    pub ymin: f32,
    pub advance: f32,
}

pub struct Fonts {
    fonts: [fontdue::Font; 2],
    glyphs: HashMap<(Weight, u32, char), Glyph>,
    pub texture: glow::Texture,
    cursor: (i32, i32),
    row_h: i32,
}

impl Fonts {
    pub unsafe fn new(gl: &glow::Context) -> Self {
        let load = |b: &[u8]| fontdue::Font::from_bytes(b, fontdue::FontSettings::default()).expect("font");
        let texture = gl.create_texture().expect("atlas");
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::R8 as i32,
            ATLAS,
            ATLAS,
            0,
            glow::RED,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(&vec![0u8; (ATLAS * ATLAS) as usize])),
        );
        for (p, v) in [(glow::TEXTURE_MIN_FILTER, glow::LINEAR), (glow::TEXTURE_MAG_FILTER, glow::LINEAR)] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, p, v as i32);
        }
        Fonts { fonts: [load(REGULAR), load(MEDIUM)], glyphs: HashMap::new(), texture, cursor: (0, 0), row_h: 0 }
    }

    pub fn kern(&self, weight: Weight, size: f32, a: char, b: char) -> f32 {
        self.fonts[weight as usize].horizontal_kern(a, b, size).unwrap_or(0.0)
    }

    pub fn glyph(&mut self, gl: &glow::Context, weight: Weight, size: f32, c: char) -> Glyph {
        let key = (weight, (size * 4.0).round() as u32, c);
        if let Some(g) = self.glyphs.get(&key) {
            return *g;
        }
        let (m, bitmap) = self.fonts[weight as usize].rasterize(c, size);
        let (w, h) = (m.width as i32, m.height as i32);
        if self.cursor.0 + w + PAD > ATLAS {
            self.cursor = (0, self.cursor.1 + self.row_h + PAD);
            self.row_h = 0;
        }
        if self.cursor.1 + h + PAD > ATLAS {
            // Atlas full (only with unusual text); start over. Glyphs already queued this frame
            // may render garbled for one frame.
            self.glyphs.clear();
            self.cursor = (0, 0);
            self.row_h = 0;
        }
        let (x, y) = self.cursor;
        if w > 0 && h > 0 {
            unsafe {
                gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
                gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    x,
                    y,
                    w,
                    h,
                    glow::RED,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&bitmap)),
                );
            }
        }
        self.cursor.0 += w + PAD;
        self.row_h = self.row_h.max(h);
        let a = ATLAS as f32;
        let g = Glyph {
            uv: [x as f32 / a, y as f32 / a, (x + w) as f32 / a, (y + h) as f32 / a],
            w: w as f32,
            h: h as f32,
            xmin: m.xmin as f32,
            ymin: m.ymin as f32,
            advance: m.advance_width,
        };
        self.glyphs.insert(key, g);
        g
    }

    /// Advance width of a string (no rasterisation needed).
    pub fn measure(&self, weight: Weight, size: f32, tracking: f32, s: &str) -> f32 {
        let font = &self.fonts[weight as usize];
        let mut w = 0.0;
        let mut prev = None;
        for c in s.chars() {
            if let Some(p) = prev {
                w += font.horizontal_kern(p, c, size).unwrap_or(0.0);
            }
            w += font.metrics(c, size).advance_width + tracking;
            prev = Some(c);
        }
        w - if prev.is_some() { tracking } else { 0.0 }
    }

    /// Greedy word wrap into at most `max_lines`; the last line gets an ellipsis if text remains.
    pub fn wrap(&self, weight: Weight, size: f32, s: &str, max_w: f32, max_lines: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut line = String::new();
        let words: Vec<&str> = s.split_whitespace().collect();
        for (i, word) in words.iter().enumerate() {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if self.measure(weight, size, 0.0, &candidate) <= max_w || line.is_empty() {
                line = candidate;
                continue;
            }
            lines.push(std::mem::take(&mut line));
            if lines.len() == max_lines {
                let last = lines.last_mut().unwrap();
                *last = self.ellipsize(weight, size, &format!("{last} {}", words[i..].join(" ")), max_w);
                return lines;
            }
            line = word.to_string();
        }
        if !line.is_empty() {
            let fitted = self.ellipsize(weight, size, &line, max_w);
            lines.push(fitted);
        }
        lines
    }

    /// Cut a single line to `max_w`, ending in "…" when shortened.
    pub fn ellipsize(&self, weight: Weight, size: f32, s: &str, max_w: f32) -> String {
        if self.measure(weight, size, 0.0, s) <= max_w {
            return s.to_string();
        }
        let mut out: String = s.to_string();
        while !out.is_empty() && self.measure(weight, size, 0.0, &format!("{out}…")) > max_w {
            out.pop();
        }
        format!("{}…", out.trim_end_matches([' ', ',', ':', '–', '-']))
    }
}
