//! OpenGL renderer: context creation plus the passes that draw a frame.

mod gl_util;
pub mod map;
pub mod overlay;
mod text;
mod ui;

use crate::astronomy::SubsolarPoint;
use glow::HasContext;
use glutin::config::{ConfigTemplateBuilder, GlConfig};
use glutin::context::{ContextApi, ContextAttributesBuilder, GlProfile, NotCurrentGlContext, PossiblyCurrentContext, PossiblyCurrentGlContext, Version};
use glutin::display::{GetGlDisplay, GlDisplay};
use glutin::surface::{GlSurface, Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use std::num::NonZeroU32;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

pub use map::View;
pub use overlay::{Hits, Overlay};

/// Everything needed to draw one frame.
pub struct Frame<'a> {
    pub sun: SubsolarPoint,
    pub overlay: Overlay<'a>,
}

/// A window with a current OpenGL 3.3 core context and the GPU resources to draw the scene.
pub struct Renderer {
    // Field order matters for drop order: GL objects die with the context, before the window.
    pub gl: glow::Context,
    map: map::MapPass,
    ui: ui::Ui,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    pub window: Window,
    pub view: View,
    center_lon: f64,
    land_mask: map::LandMask,
    /// Land coverage per screen cell, for placing the callout over water.
    land: map::LandGrid,
}

/// Cell size (px) of the land grid used for callout placement.
const LAND_CELL: f32 = 16.0;

/// Create a window with a current OpenGL 3.3 core context.
fn gl_window(
    event_loop: &ActiveEventLoop,
    attrs: WindowAttributes,
) -> (Window, Surface<WindowSurface>, PossiblyCurrentContext, glow::Context) {
    let template = ConfigTemplateBuilder::new();
    let (window, config) = DisplayBuilder::new()
        .with_window_attributes(Some(attrs))
        .build(event_loop, template, |configs| {
            // No MSAA needed: everything is anti-aliased analytically in the shaders.
            configs.min_by_key(|c| c.num_samples()).expect("no GL config")
        })
        .expect("create window");
    let window = window.expect("window");
    let raw = window.window_handle().ok().map(|h| h.as_raw());
    let ctx_attrs = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
        .with_profile(GlProfile::Core)
        .build(raw);
    let display = config.display();
    let not_current = unsafe { display.create_context(&config, &ctx_attrs) }.expect("create GL context");
    let surface_attrs = window.build_surface_attributes(Default::default()).expect("surface attributes");
    let surface = unsafe { display.create_window_surface(&config, &surface_attrs) }.expect("create surface");
    let context = not_current.make_current(&surface).expect("make current");
    let _ = surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::MIN));
    let gl = unsafe { glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s)) };
    (window, surface, context, gl)
}

/// A plain black window, used to cover secondary monitors in screensaver mode.
pub struct Blank {
    gl: glow::Context,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    pub window: Window,
}

impl Blank {
    pub fn new(event_loop: &ActiveEventLoop, attrs: WindowAttributes) -> Self {
        let (window, surface, context, gl) = gl_window(event_loop, attrs);
        Blank { gl, surface, context, window }
    }

    pub fn draw(&self) {
        let _ = self.context.make_current(&self.surface);
        unsafe {
            self.gl.clear_color(0.0, 0.0, 0.0, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
        let _ = self.surface.swap_buffers(&self.context);
    }
}

impl Renderer {
    pub fn new(event_loop: &ActiveEventLoop, attrs: WindowAttributes, center_lon: f64) -> Self {
        let (window, surface, context, gl) = gl_window(event_loop, attrs);
        let map = unsafe { map::MapPass::new(&gl, window.current_monitor().map(|m| m.size().width).unwrap_or(4096)) };
        let ui = unsafe { ui::Ui::new(&gl) };
        let size = window.inner_size();
        let view = View::new(size.width.max(1) as f64, size.height.max(1) as f64, center_lon);
        let land_mask = map::LandMask::load();
        let land = map::LandGrid::new(&land_mask, &view, LAND_CELL);
        Renderer {
            gl,
            map,
            ui,
            surface,
            context,
            view,
            land_mask,
            land,
            window,
            center_lon,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (NonZeroU32::new(width.max(1)).unwrap(), NonZeroU32::new(height.max(1)).unwrap());
        self.surface.resize(&self.context, w, h);
        self.view = View::new(w.get() as f64, h.get() as f64, self.center_lon);
        self.land = map::LandGrid::new(&self.land_mask, &self.view, LAND_CELL);
    }

    pub fn draw(&mut self, frame: &Frame) -> Hits {
        // Other windows (secondary monitors) have their own contexts.
        let _ = self.context.make_current(&self.surface);
        unsafe {
            let gl = &self.gl;
            gl.viewport(0, 0, self.view.width as i32, self.view.height as i32);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            self.map.draw(gl, &self.view, frame.sun);
        }
        overlay::draw(&mut self.ui, &self.gl, &self.view, &self.land, &frame.overlay)
    }

    pub fn present(&self) {
        self.surface.swap_buffers(&self.context).expect("swap buffers");
    }

    /// Read the back buffer into a PNG file (used for `--screenshot`).
    pub fn save_png(&self, path: &std::path::Path) -> std::io::Result<()> {
        let (w, h) = (self.view.width as u32, self.view.height as u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        unsafe {
            self.gl.read_pixels(
                0,
                0,
                w as i32,
                h as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut rgba)),
            );
        }
        // GL rows are bottom-up.
        let row = (w * 4) as usize;
        let flipped: Vec<u8> = rgba.chunks(row).rev().flatten().copied().collect();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path)?), w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(&flipped)?;
        Ok(())
    }
}
