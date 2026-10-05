//! Local news cache: per-feed HTTP validators and the articles of the last few days.
//!
//! Only headline, teaser and link are stored, and articles expire after `max_age_hours`
//! (NZZ's terms forbid permanent storage).

use crate::news::Article;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Default, Serialize, Deserialize)]
pub struct Cache {
    pub feeds: BTreeMap<String, FeedCache>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct FeedCache {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// Unix time of the last successful fetch (including 304 Not Modified).
    pub fetched_at: i64,
    pub articles: Vec<Article>,
}

fn path() -> PathBuf {
    crate::config::cache_dir().join("news-cache.json")
}

impl Cache {
    pub fn load() -> Cache {
        std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Write to a temp file and rename, so a crash never leaves a truncated cache behind.
        let tmp = p.with_extension("tmp");
        if serde_json::to_vec(self).ok().and_then(|b| std::fs::write(&tmp, b).ok()).is_some() {
            let _ = std::fs::rename(tmp, p);
        }
    }

    /// Drop articles older than `min_published` and feeds that are no longer configured.
    pub fn prune(&mut self, min_published: i64, urls: &[&str]) {
        self.feeds.retain(|url, _| urls.contains(&url.as_str()));
        for feed in self.feeds.values_mut() {
            feed.articles.retain(|a| a.published >= min_published);
        }
    }
}
