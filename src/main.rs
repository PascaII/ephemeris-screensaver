// Release builds on Windows are GUI applications (no console window).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod astronomy;
mod cache;
mod config;
mod dedup;
mod geolocation;
mod news;
mod renderer;

use app::{App, Mode, Options, UserEvent};
use winit::event_loop::EventLoop;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = config::Config::load();
    if args.iter().any(|a| a == "--dump-news") {
        dump_news(&config);
        return;
    }
    let opts = parse_args(args);
    // Read the local UTC offset before any other thread exists (required on Unix).
    let utc_offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);

    let event_loop = EventLoop::<UserEvent>::with_user_event().build().expect("event loop");
    let initial = if opts.screenshot.is_some() {
        // Deterministic single frame: build events synchronously.
        let gazetteer = geolocation::Gazetteer::load();
        news::events(&config, &gazetteer, news::refresh(&config, true))
    } else {
        let proxy = event_loop.create_proxy();
        news::spawn(config.clone(), move |events| proxy.send_event(UserEvent::News(events)).is_ok());
        Vec::new()
    };
    let mut app = App::new(opts, config, utc_offset, initial);
    event_loop.run_app(&mut app).expect("run app");
}

fn dump_news(config: &config::Config) {
    let gazetteer = geolocation::Gazetteer::load();
    let events = news::events(config, &gazetteer, news::refresh(config, true));
    for (i, e) in events.iter().enumerate() {
        println!("{:>2}. {:.2}  {} ({})  [{}]", i + 1, e.score, e.location.name, e.location.iso, e.sources().join(" "));
        for a in &e.articles {
            println!("        {:<4} {}", a.source, a.title);
        }
    }
}

fn parse_args(args: Vec<String>) -> Options {
    let mut opts = Options {
        mode: if cfg!(windows) { Mode::Screensaver } else { Mode::Window },
        screenshot: None,
        fixed_time: None,
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
            _ => {}
        }
    }
    if opts.screenshot.is_some() {
        opts.mode = Mode::Window;
    }
    opts
}
