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
    let Some(opts) = parse_args(args) else {
        open_config();
        return;
    };
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

/// `/c` (or double-clicking the .scr): open the settings file in a text editor.
fn open_config() {
    let path = config::config_path();
    #[cfg(windows)]
    let _ = std::process::Command::new("notepad.exe").arg(&path).spawn();
    #[cfg(not(windows))]
    println!("Settings: {}", path.display());
}

/// Parse screensaver switches. Returns `None` for configuration mode.
///
/// Windows passes `/s` (run), `/p <hwnd>` (preview), `/c` or `/c:<hwnd>` (configure), in any case
/// and with `/` or `-`. No arguments means "configure" on Windows; elsewhere it opens a window.
fn parse_args(args: Vec<String>) -> Option<Options> {
    let mut opts = Options { mode: Mode::Window, screenshot: None, fixed_time: None };
    let mut configure = cfg!(windows) && args.is_empty();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let lower = arg.to_ascii_lowercase();
        let switch = lower.trim_start_matches(['/', '-']);
        let (name, value) = match switch.split_once(':') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (switch, None),
        };
        match name {
            "s" => opts.mode = Mode::Screensaver,
            "c" => configure = true,
            "p" | "l" => {
                let hwnd = value.or_else(|| it.next()).and_then(|v| v.parse().ok());
                match hwnd {
                    Some(h) => opts.mode = Mode::Preview(h),
                    None => return None,
                }
            }
            "window" => opts.mode = Mode::Window,
            "screenshot" => opts.screenshot = it.next().map(Into::into),
            "at" => opts.fixed_time = it.next().and_then(|s| s.parse().ok()),
            _ => {}
        }
    }
    if configure {
        return None;
    }
    if opts.screenshot.is_some() {
        opts.mode = Mode::Window;
    }
    Some(opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(args: &[&str]) -> Option<Mode> {
        parse_args(args.iter().map(|s| s.to_string()).collect()).map(|o| o.mode)
    }

    #[test]
    fn parses_screensaver_switches() {
        assert_eq!(mode(&["/s"]), Some(Mode::Screensaver));
        assert_eq!(mode(&["/S"]), Some(Mode::Screensaver));
        assert_eq!(mode(&["-s"]), Some(Mode::Screensaver));
        assert_eq!(mode(&["/p", "1234"]), Some(Mode::Preview(1234)));
        assert_eq!(mode(&["/p:5678"]), Some(Mode::Preview(5678)));
        assert_eq!(mode(&["/c"]), None);
        assert_eq!(mode(&["/c:1234"]), None);
        assert_eq!(mode(&["--window"]), Some(Mode::Window));
    }
}
