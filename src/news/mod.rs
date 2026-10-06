//! News: fetching feeds, caching them, and turning articles into map events.
//!
//! Everything runs on one background thread that wakes up once per refresh interval.
//! The UI thread only receives finished `Event` lists.

pub mod rss;

use crate::cache::{Cache, FeedCache};
use crate::config::{Config, SourceConfig};
use crate::dedup::{self, Event};
use crate::geolocation::Gazetteer;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Article {
    /// Source label, e.g. "NZZ".
    pub source: String,
    /// "de" or "en".
    pub lang: String,
    /// Topic of the feed it came from ("world", "sport", ...).
    #[serde(default)]
    pub topic: String,
    pub title: String,
    /// Upper-case label NZZ puts before some titles ("LIVE-TICKER", "INTERVIEW"), removed from `title`.
    #[serde(default)]
    pub kicker: String,
    pub summary: String,
    pub url: String,
    pub guid: String,
    /// Unix seconds.
    pub published: i64,
    /// Position in the feed (0 = top story); a proxy for editorial importance.
    pub rank: u32,
    /// Place tags provided by the feed (NYT `nyt_geo`).
    #[serde(default)]
    pub geo_tags: Vec<String>,
    /// People / organisations provided by the feed (NYT `nyt_per`, `nyt_org`).
    #[serde(default)]
    pub entity_tags: Vec<String>,
}

/// A news source. RSS is the only implementation today; JSON APIs would implement this too.
pub trait Source {
    fn url(&self) -> &str;
    /// Fetch new articles. `Ok(None)` means "not modified since the cached copy".
    fn fetch(&self, agent: &ureq::Agent, cached: &FeedCache) -> Result<Option<FeedCache>, String>;
}

pub struct RssSource(pub SourceConfig);

impl Source for RssSource {
    fn url(&self) -> &str {
        &self.0.url
    }

    fn fetch(&self, agent: &ureq::Agent, cached: &FeedCache) -> Result<Option<FeedCache>, String> {
        let mut req = agent.get(&self.0.url).header("User-Agent", USER_AGENT);
        if let Some(etag) = &cached.etag {
            req = req.header("If-None-Match", etag);
        }
        if let Some(lm) = &cached.last_modified {
            req = req.header("If-Modified-Since", lm);
        }
        let mut resp = req.call().map_err(|e| e.to_string())?;
        if resp.status() == 304 {
            return Ok(None);
        }
        let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).map(String::from);
        let (etag, last_modified) = (header("etag"), header("last-modified"));
        let body = resp.body_mut().with_config().limit(4 << 20).read_to_string().map_err(|e| e.to_string())?;
        Ok(Some(FeedCache { etag, last_modified, fetched_at: now(), articles: rss::parse(&body, &self.0) }))
    }
}

const USER_AGENT: &str = concat!("Ephemeris-Screensaver/", env!("CARGO_PKG_VERSION"), " (personal RSS reader)");

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn agent() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    // ureq defaults to WebPki roots, which native-tls cannot use: trust the OS certificate store.
    let tls = TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into()
}

/// Refresh stale feeds (respecting the refresh interval and HTTP validators), update the cache and
/// return all recent articles, de-duplicated by URL. Network errors keep the cached copy.
pub fn refresh(config: &Config, allow_network: bool) -> Vec<Article> {
    let mut cache = Cache::load();
    let interval = config.refresh_interval().as_secs() as i64;
    let agent = agent();
    let sources: Vec<RssSource> = config.feeds().into_iter().map(RssSource).collect();

    let mut changed = false;
    for source in &sources {
        let cached = cache.feeds.get(source.url()).cloned().unwrap_or_default();
        // Small slack so an hourly refresh doesn't skip a feed fetched 59m59s ago.
        if !allow_network || now() - cached.fetched_at < interval - 60 {
            continue;
        }
        match source.fetch(&agent, &cached) {
            Ok(Some(fresh)) => {
                cache.feeds.insert(source.url().to_string(), fresh);
            }
            Ok(None) => {
                cache.feeds.entry(source.url().to_string()).or_default().fetched_at = now();
            }
            Err(e) => eprintln!("ephemeris: fetching {} failed: {e}", source.url()),
        }
        changed = true;
    }

    let min_published = now() - config.max_age_hours as i64 * 3600;
    let urls: Vec<&str> = sources.iter().map(|s| s.url()).collect();
    cache.prune(min_published, &urls);
    if changed {
        cache.save();
    }

    let mut seen = std::collections::HashSet::new();
    let mut articles: Vec<Article> = Vec::new();
    for url in urls {
        for a in cache.feeds.get(url).map(|f| f.articles.as_slice()).unwrap_or_default() {
            let skip = config.skip_kickers.iter().any(|k| !k.is_empty() && a.kicker.starts_with(k.as_str()));
            if !skip && seen.insert(a.url.clone()) {
                articles.push(a.clone());
            }
        }
    }
    articles
}

/// Geolocate articles (dropping those without a place) and cluster them into scored events.
pub fn events(config: &Config, gazetteer: &Gazetteer, articles: Vec<Article>) -> Vec<Event> {
    let items = articles.into_iter().filter_map(|a| gazetteer.locate(&a).map(|l| (a, l))).collect();
    dedup::cluster(items, &config.source_order(), now())
}

/// Background refresh loop: publish cached events immediately, then refresh from the network and
/// repeat every refresh interval. Stops when `deliver` returns false (the UI is gone).
pub fn spawn(config: Config, deliver: impl Fn(Vec<Event>) -> bool + Send + 'static) {
    std::thread::Builder::new()
        .name("news".into())
        .spawn(move || {
            let gazetteer = Gazetteer::load();
            if !deliver(events(&config, &gazetteer, refresh(&config, false))) {
                return;
            }
            loop {
                if !deliver(events(&config, &gazetteer, refresh(&config, true))) {
                    return;
                }
                std::thread::sleep(config.refresh_interval());
            }
        })
        .expect("spawn news thread");
}
