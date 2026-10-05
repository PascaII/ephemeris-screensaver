//! User configuration: a small TOML file, created with defaults on first use.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Longitude at the centre of the screen, in degrees (e.g. 10 to centre Europe).
    pub center_lon: f64,
    /// How often news feeds are refreshed, in minutes. Values below 30 are raised to 30.
    pub refresh_minutes: u64,
    /// Articles older than this are ignored and dropped from the cache.
    pub max_age_hours: u64,
    /// Maximum number of events shown on the map.
    pub max_events: usize,
    /// Seconds each featured event stays in the spotlight card.
    pub spotlight_seconds: u64,
    /// Show a small clock in the top-right corner.
    pub show_clock: bool,
    /// Exit the screensaver on mouse movement (classic behaviour) instead of revealing hover cards.
    pub exit_on_mouse_move: bool,
    pub sources: Vec<SourceConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceConfig {
    /// Short label shown on the map ("NZZ", "BBC", "NYT"). Feeds with the same name form one source.
    pub name: String,
    pub url: String,
    /// "de" or "en"; used by geolocation and deduplication.
    pub lang: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        let src = |name: &str, url: &str, lang: &str| SourceConfig {
            name: name.into(),
            url: url.into(),
            lang: lang.into(),
            enabled: true,
        };
        Config {
            center_lon: 10.0,
            refresh_minutes: 60,
            max_age_hours: 72,
            max_events: 16,
            spotlight_seconds: 12,
            show_clock: true,
            exit_on_mouse_move: false,
            sources: vec![
                src("NZZ", "https://www.nzz.ch/startseite.rss", "de"),
                src("NZZ", "https://www.nzz.ch/international.rss", "de"),
                src("BBC", "https://feeds.bbci.co.uk/news/world/rss.xml", "en"),
                src("NYT", "https://rss.nytimes.com/services/xml/rss/nyt/World.xml", "en"),
            ],
        }
    }
}

impl Config {
    /// Load the config file, writing defaults if it doesn't exist. A broken file falls back to
    /// defaults (and is left untouched so the user can fix it).
    pub fn load() -> Config {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                eprintln!("ephemeris: ignoring invalid {}: {e}", path.display());
                Config::default()
            }),
            Err(_) => {
                let cfg = Config::default();
                cfg.save();
                cfg
            }
        }
    }

    pub fn save(&self) {
        let path = config_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = toml::to_string_pretty(self).unwrap_or_default();
        let _ = std::fs::write(&path, format!("# Ephemeris screensaver settings\n\n{text}"));
    }

    pub fn refresh_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.refresh_minutes.max(30) * 60)
    }
}

/// `%APPDATA%\Ephemeris\config.toml` on Windows, the platform config directory elsewhere.
pub fn config_path() -> PathBuf {
    app_dir("APPDATA", "Library/Application Support", "XDG_CONFIG_HOME", ".config").join("config.toml")
}

/// `%LOCALAPPDATA%\Ephemeris` on Windows, the platform cache directory elsewhere.
pub fn cache_dir() -> PathBuf {
    app_dir("LOCALAPPDATA", "Library/Caches", "XDG_CACHE_HOME", ".cache")
}

fn app_dir(win_var: &str, mac_rel: &str, xdg_var: &str, xdg_rel: &str) -> PathBuf {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let home = env("HOME").or_else(|| env("USERPROFILE")).unwrap_or_else(|| PathBuf::from("."));
    let base = if cfg!(windows) {
        env(win_var).unwrap_or(home)
    } else if cfg!(target_os = "macos") {
        home.join(mac_rel)
    } else {
        env(xdg_var).unwrap_or_else(|| home.join(xdg_rel))
    };
    base.join("Ephemeris")
}
