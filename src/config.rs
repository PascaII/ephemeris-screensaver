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
    /// Write start-up window events and the exit reason to `exit.log` in the cache directory.
    pub debug: bool,
    /// Skip articles whose NZZ-style kicker starts with one of these (opinion, podcasts, ads).
    pub skip_kickers: Vec<String>,
    /// News topics to show; see `TOPICS`.
    pub topics: Vec<String>,
    /// Built-in publishers to use, in order of preference (the first one's headline leads a card).
    pub publishers: Vec<String>,
    /// Additional RSS feeds beyond the built-in catalog.
    pub extra_feeds: Vec<SourceConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceConfig {
    /// Short label shown on the map ("NZZ", "BBC", "NYT"). Feeds with the same name form one source.
    pub name: String,
    pub url: String,
    /// "de" or "en"; used by geolocation and deduplication.
    pub lang: String,
    /// Topic label of this feed (e.g. "sport"); empty if unknown.
    #[serde(default)]
    pub topic: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

/// Topics that the built-in catalog knows, in display order.
pub const TOPICS: [&str; 8] = ["top", "world", "politics", "business", "sport", "science", "tech", "culture"];

/// Built-in feeds: (publisher, language, [(topic, url)]).
/// "politics" is domestic politics for each publisher (Swiss, UK and US respectively).
const CATALOG: &[(&str, &str, &[(&str, &str)])] = &[
    ("NZZ", "de", &[
        ("top", "https://www.nzz.ch/startseite.rss"),
        ("world", "https://www.nzz.ch/international.rss"),
        ("politics", "https://www.nzz.ch/schweiz.rss"),
        ("business", "https://www.nzz.ch/wirtschaft.rss"),
        ("sport", "https://www.nzz.ch/sport.rss"),
        ("science", "https://www.nzz.ch/wissenschaft.rss"),
        ("tech", "https://www.nzz.ch/technologie.rss"),
        ("culture", "https://www.nzz.ch/feuilleton.rss"),
    ]),
    ("BBC", "en", &[
        ("top", "https://feeds.bbci.co.uk/news/rss.xml"),
        ("world", "https://feeds.bbci.co.uk/news/world/rss.xml"),
        ("politics", "https://feeds.bbci.co.uk/news/politics/rss.xml"),
        ("business", "https://feeds.bbci.co.uk/news/business/rss.xml"),
        ("sport", "https://feeds.bbci.co.uk/sport/rss.xml"),
        ("science", "https://feeds.bbci.co.uk/news/science_and_environment/rss.xml"),
        ("tech", "https://feeds.bbci.co.uk/news/technology/rss.xml"),
        ("culture", "https://feeds.bbci.co.uk/news/entertainment_and_arts/rss.xml"),
    ]),
    ("NYT", "en", &[
        ("top", "https://rss.nytimes.com/services/xml/rss/nyt/HomePage.xml"),
        ("world", "https://rss.nytimes.com/services/xml/rss/nyt/World.xml"),
        ("politics", "https://rss.nytimes.com/services/xml/rss/nyt/Politics.xml"),
        ("business", "https://rss.nytimes.com/services/xml/rss/nyt/Business.xml"),
        ("sport", "https://rss.nytimes.com/services/xml/rss/nyt/Sports.xml"),
        ("science", "https://rss.nytimes.com/services/xml/rss/nyt/Science.xml"),
        ("tech", "https://rss.nytimes.com/services/xml/rss/nyt/Technology.xml"),
        ("culture", "https://rss.nytimes.com/services/xml/rss/nyt/Arts.xml"),
    ]),
];

const HEADER: &str = "# Ephemeris screensaver settings. Restart the screensaver after editing.
#
# topics      any of: top, world, politics, business, sport, science, tech, culture
#             (\"politics\" is domestic politics: Swiss for NZZ, UK for BBC, US for NYT)
# publishers  any of: NZZ, BBC, NYT, in order of preference
# extra feeds add more RSS feeds like this:
#   [[extra_feeds]]
#   name = \"Guardian\"
#   url = \"https://www.theguardian.com/world/rss\"
#   lang = \"en\"
#   topic = \"world\"
# debug       true writes why the screensaver quit to %LOCALAPPDATA%\\Ephemeris\\exit.log
";

fn yes() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Config {
            center_lon: 10.0,
            refresh_minutes: 60,
            max_age_hours: 72,
            max_events: 16,
            spotlight_seconds: 12,
            show_clock: true,
            exit_on_mouse_move: false,
            debug: false,
            skip_kickers: ["KOMMENTAR", "GASTKOMMENTAR", "INTERVIEW", "PODCAST", "SPONSORED", "QUIZ", "NEWSLETTER"]
                .map(String::from)
                .to_vec(),
            topics: vec!["top".into(), "world".into()],
            publishers: vec!["NZZ".into(), "BBC".into(), "NYT".into()],
            extra_feeds: Vec::new(),
        }
    }
}

impl Config {
    /// Load the config file, writing defaults if it doesn't exist. A broken file falls back to
    /// defaults (and is left untouched so the user can fix it).
    pub fn load() -> Config {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<Config>(&text) {
                Ok(cfg) => {
                    // Files from before topics existed list feeds under [[sources]]; rewrite them in
                    // the new format, keeping all other settings.
                    if text.contains("[[sources]]") {
                        cfg.save();
                    }
                    for t in cfg.topics.iter().filter(|t| !TOPICS.contains(&t.as_str())) {
                        eprintln!("ephemeris: unknown topic {t:?} (known: {})", TOPICS.join(", "));
                    }
                    cfg
                }
                Err(e) => {
                    eprintln!("ephemeris: ignoring invalid {}: {e}", path.display());
                    Config::default()
                }
            },
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
        let text = toml::to_string(self).unwrap_or_default();
        let _ = std::fs::write(&path, format!("{HEADER}\n{text}"));
    }

    /// All feeds to fetch: the catalog feeds for the selected publishers and topics, then extras.
    /// Order matters: when one article appears in several feeds, the first feed's topic wins.
    pub fn feeds(&self) -> Vec<SourceConfig> {
        let mut out = Vec::new();
        for topic in &self.topics {
            for publisher in &self.publishers {
                let Some((name, lang, feeds)) = CATALOG.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(publisher)) else {
                    continue;
                };
                for (t, url) in feeds.iter().filter(|(t, _)| t.eq_ignore_ascii_case(topic)) {
                    out.push(SourceConfig {
                        name: name.to_string(),
                        url: url.to_string(),
                        lang: lang.to_string(),
                        topic: t.to_string(),
                        enabled: true,
                    });
                }
            }
        }
        out.extend(self.extra_feeds.iter().filter(|f| f.enabled).cloned());
        out
    }

    /// Source names in order of preference (publishers first, then extra feeds).
    pub fn source_order(&self) -> Vec<String> {
        let mut order: Vec<String> = Vec::new();
        for f in self.feeds() {
            if !order.contains(&f.name) {
                order.push(f.name);
            }
        }
        order
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_topics_and_publishers() {
        let cfg: Config = toml::from_str("topics = [\"sport\"]\npublishers = [\"BBC\", \"nzz\"]").unwrap();
        let feeds = cfg.feeds();
        let urls: Vec<&str> = feeds.iter().map(|f| f.url.as_str()).collect();
        assert_eq!(urls, ["https://feeds.bbci.co.uk/sport/rss.xml", "https://www.nzz.ch/sport.rss"]);
        assert_eq!(feeds[1].lang, "de");
        assert_eq!(feeds[1].topic, "sport");
        assert_eq!(cfg.source_order(), ["BBC", "NZZ"]);
        assert_eq!(cfg.max_age_hours, 72, "unspecified settings keep their defaults");
    }

    #[test]
    fn every_topic_exists_for_every_publisher() {
        for (name, _, feeds) in CATALOG {
            for t in TOPICS {
                assert!(feeds.iter().any(|(ft, _)| *ft == t), "{name} lacks {t}");
            }
        }
    }

    #[test]
    fn legacy_sources_are_ignored() {
        let cfg: Config = toml::from_str("max_age_hours = 24\n[[sources]]\nname = \"NZZ\"\nurl = \"x\"\nlang = \"de\"").unwrap();
        assert_eq!(cfg.max_age_hours, 24);
        assert_eq!(cfg.feeds().len(), 6);
    }
}
