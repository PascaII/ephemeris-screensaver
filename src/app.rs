//! Application state machine: window lifecycle, frame pacing, spotlight cycling and input.

use crate::astronomy;
use crate::config::{Config, SunConfig};
use crate::dedup::Event;
use crate::renderer::overlay::SunInfo;
use crate::renderer::{Blank, Frame, Hits, Overlay, Renderer};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use time::UtcOffset;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorIcon, Fullscreen, WindowAttributes, WindowId};

/// Longest sleep between frames when nothing animates. The terminator moves 0.04° in 10 s.
const IDLE_FRAME: Duration = Duration::from_secs(10);
/// Frame interval while something animates.
const ANIM_FRAME: Duration = Duration::from_millis(33);
const FADE_OUT: f32 = 0.35;
const FADE_IN: f32 = 0.6;
const RING_SECS: f32 = 1.6;
/// Mouse travel (physical px) before the cursor is revealed; filters jitter and synthetic moves.
const MOVE_THRESHOLD: f64 = 8.0;
/// Input that would end the screensaver is ignored this long after the window is ready: its creation
/// (focus, the click on "Preview", a phantom AltGr on layouts that have one) produces events that
/// are not the user coming back. They arrive within milliseconds of the window being ready.
const INPUT_GRACE: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    /// Fullscreen screensaver (`/s`).
    Screensaver,
    /// Resizable development window (`--window`).
    Window,
    /// Live preview inside the Screen Saver Settings dialog (`/p <hwnd>`).
    Preview(isize),
}

pub enum UserEvent {
    News(Vec<Event>),
}

pub struct Options {
    pub mode: Mode,
    /// Render one frame to this PNG and exit.
    pub screenshot: Option<PathBuf>,
    /// Fixed Unix time instead of the clock (for screenshots / debugging).
    pub fixed_time: Option<f64>,
}

pub struct App {
    opts: Options,
    config: Config,
    utc_offset: UtcOffset,
    sun: Option<SunClock>,
    credits: String,
    renderer: Option<Renderer>,
    /// Black windows covering secondary monitors (screensaver mode).
    blanks: Vec<Blank>,
    events: Vec<Event>,

    next_frame: Instant,
    last_frame: Instant,
    /// Index of the event featured by the automatic spotlight, and when it started.
    spotlight: usize,
    spotlight_since: Instant,
    /// Event currently shown in the card, its opacity, and when its ring animation started.
    card: Option<usize>,
    card_alpha: f32,
    ring_since: Instant,

    mouse: Option<PhysicalPosition<f64>>,
    mouse_travel: f64,
    cursor_visible: bool,
    hovered_marker: Option<usize>,
    hovered_link: Option<usize>,
    hits: Hits,

    started: Instant,
    /// End of the start-up grace period (see `INPUT_GRACE`); reset once the window is ready.
    input_after: Instant,
    /// `exit.log` in the cache dir (`debug = true`): start-up window events and why the app quit.
    exit_log: Option<File>,
}

impl App {
    pub fn new(opts: Options, config: Config, utc_offset: UtcOffset, events: Vec<Event>) -> Self {
        let credits = config.source_order().join(", ");
        let now = Instant::now();
        let sun = SunClock::new(&config.sun);
        let mut app = App {
            opts,
            config,
            utc_offset,
            sun,
            credits,
            renderer: None,
            blanks: Vec::new(),
            events: Vec::new(),
            next_frame: now,
            last_frame: now,
            spotlight: 0,
            spotlight_since: now,
            card: None,
            card_alpha: 0.0,
            ring_since: now,
            mouse: None,
            mouse_travel: 0.0,
            cursor_visible: false,
            hovered_marker: None,
            hovered_link: None,
            hits: Hits::default(),
            started: now,
            input_after: now + INPUT_GRACE,
            exit_log: None,
        };
        // Not in preview: the settings dialog restarts it constantly and would overwrite the log.
        if app.config.debug && !app.preview() {
            let dir = crate::config::cache_dir();
            let _ = std::fs::create_dir_all(&dir);
            app.exit_log = File::create(dir.join("exit.log")).ok();
        }
        let mode = app.opts.mode.clone();
        app.log(format_args!("start {mode:?}"));
        app.set_events(events);
        if app.opts.screenshot.is_some() && !app.events.is_empty() {
            // Static frame: show the top event fully faded in.
            app.card = Some(0);
            app.card_alpha = 1.0;
            app.ring_since = now - Duration::from_secs(10);
        }
        app
    }

    fn now_unix(&self) -> f64 {
        self.opts.fixed_time.unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64())
    }

    fn set_events(&mut self, mut events: Vec<Event>) {
        events.truncate(self.config.max_events);
        // Keep showing the same story if it survived the refresh.
        let current = self.card.and_then(|i| self.events.get(i)).map(|e| e.articles[0].url.clone());
        self.events = events;
        let found = current.and_then(|url| self.events.iter().position(|e| e.articles.iter().any(|a| a.url == url)));
        self.card = found;
        if found.is_none() {
            self.card_alpha = 0.0;
        }
        self.spotlight = found.unwrap_or(0);
        self.hovered_marker = None;
        self.hovered_link = None;
    }

    /// The event the card should show right now.
    fn card_target(&self) -> Option<usize> {
        if self.events.is_empty() {
            return None;
        }
        if let Some(i) = self.hovered_marker {
            return Some(i);
        }
        let over_card = self.mouse.zip(self.hits.card).is_some_and(|(m, r)| r.contains(m.x as f32, m.y as f32));
        if over_card && self.card.is_some() {
            return self.card;
        }
        Some(self.spotlight.min(self.events.len() - 1))
    }

    /// Advance animations by `dt` seconds; returns true while anything is still moving.
    fn animate(&mut self, dt: f32, now: Instant) -> bool {
        let interacting = self.hovered_marker.is_some() || self.hovered_link.is_some();
        let spot = Duration::from_secs(self.config.spotlight_seconds.max(3));
        if interacting {
            self.spotlight_since = now;
        } else if !self.events.is_empty() && now - self.spotlight_since >= spot {
            self.spotlight = (self.spotlight + 1) % self.events.len();
            self.spotlight_since = now;
        }

        let target = self.card_target();
        if self.card != target {
            self.card_alpha -= dt / FADE_OUT;
            if self.card_alpha <= 0.0 || self.card.is_none() {
                self.card_alpha = 0.0;
                self.card = target;
                self.ring_since = now;
            }
            return true;
        }
        if self.card.is_some() && self.card_alpha < 1.0 {
            self.card_alpha = (self.card_alpha + dt / FADE_IN).min(1.0);
            return true;
        }
        (now - self.ring_since).as_secs_f32() < RING_SECS
    }

    fn redraw(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        let animating = self.opts.screenshot.is_none() && self.animate(dt, now);

        let unix = self.now_unix();
        if let Some(sun) = &mut self.sun {
            sun.update(unix as i64);
        }
        let minimal = self.preview();
        let Some(renderer) = self.renderer.as_mut() else { return };
        let frame = Frame {
            sun: astronomy::subsolar_point(unix),
            overlay: Overlay {
                events: &self.events,
                card: self.card.map(|i| (i, ease(self.card_alpha))),
                ring: (now - self.ring_since).as_secs_f32() / RING_SECS,
                hovered_link: self.hovered_link,
                now: unix as i64,
                utc_offset: self.utc_offset,
                show_clock: self.config.show_clock,
                sun: self.sun.as_ref().and_then(|sun| sun.info.as_ref()),
                show_topics: self.config.topics.len() > 1,
                credits: &self.credits,
                minimal,
            },
        };
        self.hits = renderer.draw(&frame);

        // Sleep until the next thing that changes: animation frame, spotlight switch, clock minute.
        self.next_frame = if animating {
            now + ANIM_FRAME
        } else {
            let to_minute = Duration::from_secs_f64(60.0 - unix.rem_euclid(60.0) + 0.05);
            let spot = Duration::from_secs(self.config.spotlight_seconds.max(3));
            let to_spot = (self.spotlight_since + spot).saturating_duration_since(now);
            now + IDLE_FRAME.min(to_minute).min(to_spot.max(ANIM_FRAME))
        };
    }

    fn screensaver(&self) -> bool {
        self.opts.mode == Mode::Screensaver
    }

    fn preview(&self) -> bool {
        matches!(self.opts.mode, Mode::Preview(_))
    }

    fn log(&mut self, msg: std::fmt::Arguments) {
        let t = self.started.elapsed().as_secs_f64();
        if let Some(f) = self.exit_log.as_mut() {
            let _ = writeln!(f, "{t:7.3}s {msg}");
        }
    }

    fn quit(&mut self, event_loop: &ActiveEventLoop, reason: &str) {
        self.log(format_args!("exit: {reason}"));
        event_loop.exit();
    }

    fn in_grace(&self) -> bool {
        Instant::now() < self.input_after
    }

    /// End the screensaver because the user is back, unless we are still in the start-up grace period.
    /// Returns true if the app is exiting.
    fn user_exit(&mut self, event_loop: &ActiveEventLoop, reason: &str) -> bool {
        if self.in_grace() {
            self.log(format_args!("ignored during grace period: {reason}"));
            return false;
        }
        self.quit(event_loop, reason);
        true
    }

    fn update_hover(&mut self) {
        let Some(m) = self.mouse.filter(|_| self.cursor_visible) else {
            self.hovered_marker = None;
            self.hovered_link = None;
            return;
        };
        let (mx, my) = (m.x as f32, m.y as f32);
        let slack = self.renderer.as_ref().map(|r| (r.view.height / 1080.0) as f32 * 8.0).unwrap_or(8.0);
        self.hovered_marker = self
            .hits
            .markers
            .iter()
            .map(|&(x, y, r, i)| ((x - mx).hypot(y - my) - r, i))
            .filter(|(d, _)| *d < slack)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, i)| i);
        self.hovered_link = self.hits.links.iter().position(|(r, _)| r.contains(mx, my));
        if let Some(r) = &self.renderer {
            let pointer = self.hovered_marker.is_some() || self.hovered_link.is_some();
            r.window.set_cursor(if pointer { CursorIcon::Pointer } else { CursorIcon::Default });
        }
    }

    fn click(&mut self, event_loop: &ActiveEventLoop) {
        let url = match (self.hovered_link, self.hovered_marker) {
            (Some(k), _) => self.hits.links.get(k).map(|(_, u)| u.clone()),
            (None, Some(i)) => self.events.get(i).map(|e| e.articles[0].url.clone()),
            _ => None,
        };
        match url {
            Some(url) => {
                open_url(&url);
                if self.screensaver() {
                    self.quit(event_loop, "opened article");
                }
            }
            None if self.screensaver() => {
                self.user_exit(event_loop, "left click on the map");
            }
            None => {}
        }
    }
}

/// Resolve the zone once; only recalculate astronomical events when its calendar date changes.
struct SunClock {
    config: SunConfig,
    zone: jiff::tz::TimeZone,
    date: Option<jiff::civil::Date>,
    info: Option<SunInfo>,
}

impl SunClock {
    fn new(config: &SunConfig) -> Option<Self> {
        if !config.enabled {
            return None;
        }
        if !config.latitude.is_finite()
            || !(-90.0..=90.0).contains(&config.latitude)
            || !config.longitude.is_finite()
            || !(-180.0..=180.0).contains(&config.longitude)
        {
            eprintln!("ephemeris: invalid sun coordinates; sun display disabled");
            return None;
        }
        let zone = match jiff::tz::TimeZone::get(&config.timezone) {
            Ok(zone) => zone,
            Err(e) => {
                eprintln!("ephemeris: invalid sun timezone {:?}: {e}; sun display disabled", config.timezone);
                return None;
            }
        };
        Some(Self { config: config.clone(), zone, date: None, info: None })
    }

    fn update(&mut self, unix: i64) {
        let Ok(timestamp) = jiff::Timestamp::from_second(unix) else {
            self.info = None;
            self.date = None;
            return;
        };
        let local = timestamp.to_zoned(self.zone.clone());
        let date = local.date();
        if self.date != Some(date) {
            self.info = None;
            let boundaries = date.tomorrow().and_then(|next| {
                let start = self.zone.to_ambiguous_zoned(date.at(0, 0, 0, 0)).compatible()?;
                let end = self.zone.to_ambiguous_zoned(next.at(0, 0, 0, 0)).compatible()?;
                Ok((start.timestamp().as_second(), end.timestamp().as_second()))
            });
            match boundaries {
                Ok((start, end)) => {
                    let events = astronomy::sun_times(start, end, self.config.latitude, self.config.longitude);
                    let format_time = |t: Option<i64>| {
                        t.and_then(|t| jiff::Timestamp::from_second(t).ok())
                            .map(|t| {
                                let d = self.zone.to_datetime(t);
                                format!("{:02}:{:02}", d.hour(), d.minute())
                            })
                            .unwrap_or_else(|| "—".into())
                    };
                    let times = match events.polar {
                        Some(astronomy::PolarState::Day) => "Polar day · Sun does not set".into(),
                        Some(astronomy::PolarState::Night) => "Polar night · Sun does not rise".into(),
                        None => format!("Sunrise {} · Sunset {}", format_time(events.sunrise), format_time(events.sunset)),
                    };
                    self.info = Some(SunInfo { location: String::new(), times });
                }
                Err(e) => eprintln!("ephemeris: cannot calculate sun date: {e}"),
            }
            self.date = Some(date);
        }
        // The abbreviation can change during a DST transition without changing the date.
        if let Some(info) = &mut self.info {
            info.location = format!("{} · {}", self.config.city, local.strftime("%Z"));
        }
    }
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Child-window attributes for the Screen Saver Settings preview: fill the parent's client area.
#[cfg(windows)]
fn preview_attributes(attrs: WindowAttributes, hwnd: isize) -> WindowAttributes {
    use raw_window_handle::{RawWindowHandle, Win32WindowHandle};
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetClientRect(hwnd as _, &mut rect) };
    let size = winit::dpi::PhysicalSize::new((rect.right - rect.left).max(1) as u32, (rect.bottom - rect.top).max(1) as u32);
    let Some(handle) = std::num::NonZeroIsize::new(hwnd) else { return attrs };
    let parent = RawWindowHandle::Win32(Win32WindowHandle::new(handle));
    // SAFETY: the handle comes from the Screen Saver Settings dialog and outlives our child window
    // (Windows destroys the child together with its parent, which ends our event loop).
    unsafe { attrs.with_parent_window(Some(parent)) }
        .with_inner_size(size)
        .with_position(winit::dpi::PhysicalPosition::new(0, 0))
        .with_decorations(false)
}

#[cfg(not(windows))]
fn preview_attributes(attrs: WindowAttributes, _hwnd: isize) -> WindowAttributes {
    attrs.with_inner_size(LogicalSize::new(152.0, 112.0))
}

/// Open a link in the default browser.
pub fn open_url(url: &str) {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let (op, file) = (wide("open"), wide(url));
        ShellExecuteW(std::ptr::null_mut(), op.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_some() {
            return;
        }
        let attrs = WindowAttributes::default().with_title("Ephemeris");
        let attrs = match self.opts.mode {
            Mode::Screensaver => {
                let primary = event_loop.primary_monitor();
                // Cover every other monitor with a black window; the map lives on the primary one.
                for m in event_loop.available_monitors().filter(|m| Some(m) != primary.as_ref()) {
                    let a = WindowAttributes::default()
                        .with_title("Ephemeris")
                        .with_fullscreen(Some(Fullscreen::Borderless(Some(m))));
                    let blank = Blank::new(event_loop, a);
                    blank.window.set_cursor_visible(false);
                    blank.window.request_redraw();
                    self.blanks.push(blank);
                }
                attrs.with_fullscreen(Some(Fullscreen::Borderless(primary)))
            }
            Mode::Window => attrs.with_inner_size(LogicalSize::new(1600.0, 900.0)),
            Mode::Preview(hwnd) => preview_attributes(attrs, hwnd),
        };
        let renderer = Renderer::new(event_loop, attrs, self.config.center_lon);
        match self.opts.mode {
            Mode::Screensaver => renderer.window.set_cursor_visible(false),
            Mode::Window => self.cursor_visible = true,
            Mode::Preview(_) => {}
        }
        renderer.window.request_redraw();
        self.renderer = Some(renderer);
        // Events from the window's creation are delivered after this returns; GL start-up time
        // (slow after long idle) must not eat into the grace period.
        self.input_after = Instant::now() + INPUT_GRACE;
        self.log(format_args!("window ready"));
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::News(events) => {
                self.set_events(events);
                if let Some(r) = &self.renderer {
                    r.window.request_redraw();
                }
            }
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause {
            if let Some(r) = &self.renderer {
                r.window.request_redraw();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.in_grace() && !matches!(event, WindowEvent::RedrawRequested) {
            self.log(format_args!("{id:?} {event:?}"));
        }
        let Some(primary) = self.renderer.as_ref().map(|r| r.window.id()) else { return };
        if self.preview() {
            // The settings dialog owns input; just draw, and quit when it closes the preview.
            match event {
                WindowEvent::RedrawRequested => {
                    self.redraw();
                    self.renderer.as_ref().unwrap().present();
                }
                WindowEvent::Resized(size) => {
                    if let Some(r) = self.renderer.as_mut() {
                        r.resize(size.width, size.height);
                    }
                }
                WindowEvent::CloseRequested | WindowEvent::Destroyed => self.quit(event_loop, "preview closed"),
                _ => {}
            }
            return;
        }
        if id != primary {
            // Secondary monitor: stay black; clicks and keys still end the screensaver.
            match event {
                WindowEvent::RedrawRequested => {
                    if let Some(b) = self.blanks.iter().find(|b| b.window.id() == id) {
                        b.draw();
                    }
                }
                WindowEvent::MouseInput { state: ElementState::Pressed, .. } => {
                    self.user_exit(event_loop, "click on a secondary monitor");
                }
                WindowEvent::KeyboardInput { event, is_synthetic: false, .. } if event.state == ElementState::Pressed => {
                    self.user_exit(event_loop, "key press on a secondary monitor");
                }
                WindowEvent::CloseRequested => self.quit(event_loop, "secondary window closed"),
                _ => {}
            }
            return;
        }
        match event {
            WindowEvent::CloseRequested => self.quit(event_loop, "close requested"),
            WindowEvent::Destroyed => self.quit(event_loop, "window destroyed"),
            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                    r.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                let renderer = self.renderer.as_ref().unwrap();
                if let Some(path) = &self.opts.screenshot {
                    renderer.save_png(path).expect("write screenshot");
                    self.quit(event_loop, "screenshot written");
                    return;
                }
                renderer.present();
            }
            WindowEvent::CursorMoved { position, .. } => {
                // While the fullscreen window settles, the cursor "moves" relative to it without the
                // user touching the mouse: don't count that as travel.
                if let Some(prev) = self.mouse.filter(|_| !self.in_grace()) {
                    self.mouse_travel += (position.x - prev.x).hypot(position.y - prev.y);
                }
                self.mouse = Some(position);
                if !self.cursor_visible && self.mouse_travel > MOVE_THRESHOLD {
                    if self.screensaver() && self.config.exit_on_mouse_move && self.user_exit(event_loop, "mouse moved") {
                        return;
                    }
                    self.cursor_visible = true;
                    if let Some(r) = &self.renderer {
                        r.window.set_cursor_visible(true);
                    }
                }
                let before = (self.hovered_marker, self.hovered_link);
                self.update_hover();
                if before != (self.hovered_marker, self.hovered_link) {
                    if let Some(r) = &self.renderer {
                        r.window.request_redraw();
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.mouse = None;
                self.update_hover();
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                self.update_hover();
                self.click(event_loop);
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button, .. } if self.screensaver() => {
                self.user_exit(event_loop, &format!("{button:?} click"));
            }
            // Synthetic presses are keys already held down when the window gained focus.
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } if event.state == ElementState::Pressed => {
                if self.screensaver() {
                    self.user_exit(event_loop, &format!("key press {:?}", event.physical_key));
                } else if event.logical_key == Key::Named(NamedKey::Escape) {
                    self.quit(event_loop, "escape");
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timestamp(s: &str) -> i64 {
        s.parse::<jiff::Timestamp>().unwrap().as_second()
    }

    #[test]
    fn sun_clock_uses_city_date_and_updates_on_rollover() {
        let mut clock = SunClock::new(&SunConfig::default()).unwrap();
        clock.update(timestamp("2026-06-20T23:30:00Z"));
        assert_eq!(clock.date.unwrap().to_string(), "2026-06-21");
        assert_eq!(clock.info.as_ref().unwrap().location, "Zürich · CEST");
        let old_times = clock.info.as_ref().unwrap().times.clone();
        clock.update(timestamp("2026-06-21T12:00:00Z"));
        assert_eq!(clock.info.as_ref().unwrap().times, old_times);
        clock.update(timestamp("2026-06-21T22:00:00Z"));
        assert_eq!(clock.date.unwrap().to_string(), "2026-06-22");
    }

    #[test]
    fn sun_clock_applies_dst_to_each_event() {
        let mut clock = SunClock::new(&SunConfig::default()).unwrap();
        clock.update(timestamp("2026-03-29T00:30:00Z"));
        assert_eq!(clock.info.as_ref().unwrap().location, "Zürich · CET");
        let events = clock.info.as_ref().unwrap().times.clone();
        clock.update(timestamp("2026-03-29T02:30:00Z"));
        assert_eq!(clock.info.as_ref().unwrap().location, "Zürich · CEST");
        assert_eq!(clock.info.as_ref().unwrap().times, events, "event offsets are evaluated at the event, not at now");
    }

    #[test]
    fn invalid_or_disabled_sun_settings_are_not_displayed() {
        for config in [
            SunConfig { enabled: false, ..SunConfig::default() },
            SunConfig { latitude: f64::NAN, ..SunConfig::default() },
            SunConfig { latitude: 91.0, ..SunConfig::default() },
            SunConfig { longitude: f64::INFINITY, ..SunConfig::default() },
            SunConfig { longitude: -181.0, ..SunConfig::default() },
            SunConfig { timezone: "Europe/Invalid".into(), ..SunConfig::default() },
        ] {
            assert!(SunClock::new(&config).is_none());
        }
    }
}
