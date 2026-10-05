//! Application state machine: window lifecycle, frame pacing and input.

use crate::astronomy;
use crate::renderer::{Frame, Renderer};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Fullscreen, WindowAttributes, WindowId};

/// Idle redraw interval. The terminator moves 0.25°/min and the clock ticks once a second.
const IDLE_FRAME: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    /// Fullscreen screensaver (`/s`).
    Screensaver,
    /// Resizable development window (`--window`).
    Window,
}

pub struct Options {
    pub mode: Mode,
    /// Render one frame to this PNG and exit.
    pub screenshot: Option<PathBuf>,
    /// Fixed Unix time instead of the clock (for screenshots / debugging).
    pub fixed_time: Option<f64>,
    pub center_lon: f64,
}

pub struct App {
    opts: Options,
    renderer: Option<Renderer>,
    next_frame: Instant,
}

impl App {
    pub fn new(opts: Options) -> Self {
        App { opts, renderer: None, next_frame: Instant::now() }
    }

    fn now_unix(&self) -> f64 {
        self.opts.fixed_time.unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_some() {
            return;
        }
        let attrs = WindowAttributes::default().with_title("Ephemeris");
        let attrs = match self.opts.mode {
            Mode::Screensaver => attrs.with_fullscreen(Some(Fullscreen::Borderless(None))),
            Mode::Window => attrs.with_inner_size(LogicalSize::new(1600.0, 900.0)),
        };
        self.renderer = Some(Renderer::new(event_loop, attrs, self.opts.center_lon));
        if let Some(r) = &self.renderer {
            r.window.request_redraw();
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause {
            if let Some(r) = &self.renderer {
                r.window.request_redraw();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let now = self.now_unix();
        let Some(renderer) = self.renderer.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                renderer.resize(size.width, size.height);
                renderer.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let frame = Frame { sun: astronomy::subsolar_point(now) };
                renderer.draw(&frame);
                if let Some(path) = &self.opts.screenshot {
                    renderer.save_png(path).expect("write screenshot");
                    event_loop.exit();
                    return;
                }
                renderer.present();
                self.next_frame = Instant::now() + IDLE_FRAME;
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}
