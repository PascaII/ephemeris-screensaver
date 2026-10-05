// Release builds on Windows are GUI applications (no console window).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod astronomy;
mod cache;
mod config;
mod news;
mod renderer;

use app::{App, Mode, Options};
use winit::event_loop::EventLoop;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--dump-news") {
        let cfg = config::Config::load();
        for a in news::refresh(&cfg, true) {
            println!("[{}] {} | {}", a.source, a.title, a.geo_tags.join("; "));
        }
        return;
    }
    let opts = parse_args(args);
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App::new(opts);
    event_loop.run_app(&mut app).expect("run app");
}

fn parse_args(args: Vec<String>) -> Options {
    let mut opts = Options {
        mode: if cfg!(windows) { Mode::Screensaver } else { Mode::Window },
        screenshot: None,
        fixed_time: None,
        center_lon: 0.0,
    };
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        // Windows passes screensaver switches as `/s`, `/S`, `-s`, `/p 1234`, `/c:1234`.
        let lower = arg.to_ascii_lowercase();
        let switch = lower.trim_start_matches(['/', '-']);
        match switch.split(':').next().unwrap_or("") {
            "s" => opts.mode = Mode::Screensaver,
            "window" => opts.mode = Mode::Window,
            "screenshot" => opts.screenshot = it.next().map(Into::into),
            "at" => opts.fixed_time = it.next().and_then(|s| s.parse().ok()),
            "center-lon" => opts.center_lon = it.next().and_then(|s| s.parse().ok()).unwrap_or(0.0),
            _ => {}
        }
    }
    if opts.screenshot.is_some() {
        opts.mode = Mode::Window;
    }
    opts
}
