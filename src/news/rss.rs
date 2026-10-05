//! Minimal RSS 2.0 parser: just the fields the map needs.

use super::Article;
use crate::config::SourceConfig;
use quick_xml::events::Event;
use quick_xml::Reader;
use time::format_description::well_known::Rfc2822;
use time::OffsetDateTime;

const NYT_GEO: &str = "nyt_geo";
const NYT_ENTITIES: [&str; 2] = ["nyt_per", "nyt_org"];

pub fn parse(xml: &str, source: &SourceConfig) -> Vec<Article> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut articles = Vec::new();
    let mut item: Option<Article> = None;
    let mut field = String::new();
    let mut category_domain = String::new();
    let mut text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if name == "item" {
                    item = Some(Article {
                        source: source.name.clone(),
                        lang: source.lang.clone(),
                        topic: source.topic.clone(),
                        rank: articles.len() as u32,
                        ..Default::default()
                    });
                } else if item.is_some() {
                    if name == "category" {
                        category_domain = e
                            .try_get_attribute("domain")
                            .ok()
                            .flatten()
                            .map(|a| String::from_utf8_lossy(&a.value).into_owned())
                            .unwrap_or_default();
                    }
                    field = name;
                    text.clear();
                }
            }
            Ok(Event::Text(t)) if item.is_some() => {
                match t.unescape() {
                    Ok(s) => text.push_str(&s),
                    Err(_) => text.push_str(&String::from_utf8_lossy(&t)),
                }
            }
            Ok(Event::CData(c)) if item.is_some() => text.push_str(&String::from_utf8_lossy(&c)),
            Ok(Event::End(e)) => {
                let name = e.name();
                if name.as_ref() == b"item" {
                    if let Some(a) = item.take() {
                        if !a.title.is_empty() && !a.url.is_empty() && a.published > 0 {
                            articles.push(a);
                        }
                    }
                } else if let Some(a) = item.as_mut() {
                    let value = clean(&text);
                    match field.as_str() {
                        "title" => a.title = value,
                        "description" => a.summary = value,
                        "link" => a.url = value,
                        "guid" => a.guid = value,
                        "pubDate" => a.published = parse_date(&value).unwrap_or(0),
                        "category" if category_domain.ends_with(NYT_GEO) => a.geo_tags.push(value),
                        "category" if NYT_ENTITIES.iter().any(|d| category_domain.ends_with(d)) => {
                            a.entity_tags.push(value)
                        }
                        _ => {}
                    }
                    field.clear();
                    text.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                eprintln!("ephemeris: rss parse error in {}: {e}", source.url);
                break;
            }
            _ => {}
        }
    }
    for a in &mut articles {
        if let Some((kicker, title)) = split_kicker(&a.title) {
            a.kicker = kicker;
            a.title = title;
        }
        if a.guid.is_empty() {
            a.guid = a.url.clone();
        }
        a.url = strip_tracking(&a.url);
    }
    articles
}

fn parse_date(s: &str) -> Option<i64> {
    // RFC 2822 requires a numeric zone in newer specs; feeds still use "GMT".
    let s = s.replace(" GMT", " +0000").replace(" UTC", " +0000");
    OffsetDateTime::parse(&s, &Rfc2822).ok().map(|d| d.unix_timestamp())
}

/// Strip HTML tags and collapse whitespace.
fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "LIVE-TICKER - Krieg in der Ukraine: ..." -> ("LIVE-TICKER", "Krieg in der Ukraine: ...").
fn split_kicker(title: &str) -> Option<(String, String)> {
    let (kicker, rest) = title.split_once(" - ")?;
    let is_label = kicker.len() <= 40 && kicker.chars().any(char::is_alphabetic) && !kicker.chars().any(char::is_lowercase);
    is_label.then(|| (kicker.trim().to_string(), rest.trim().to_string()))
}

/// Remove analytics query parameters (BBC appends `?at_medium=RSS&at_campaign=rss`).
fn strip_tracking(url: &str) -> String {
    match url.split_once('?') {
        Some((base, q)) if q.split('&').all(|p| p.starts_with("at_") || p.starts_with("utm_")) => base.to_string(),
        _ => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(name: &str, lang: &str) -> SourceConfig {
        SourceConfig { name: name.into(), url: "test".into(), lang: lang.into(), topic: "world".into(), enabled: true }
    }

    #[test]
    fn parses_nzz_style_feed() {
        let xml = r#"<?xml version="1.0"?><rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/"><channel>
            <title>NZZ</title>
            <item>
              <title>Erdbeben erschüttert Tokio &amp; Umgebung</title>
              <description>Ein starkes Beben der Stärke 6,8 hat am Montag die japanische Hauptstadt getroffen.</description>
              <media:thumbnail width="200" height="200" url="https://img.example/x.jpg"/>
              <link>https://www.nzz.ch/international/erdbeben-ld.1</link>
              <pubDate>Mon, 05 Oct 2026 16:50:52 GMT</pubDate>
              <guid isPermaLink="false">ld.1</guid>
            </item>
            <item><title>No link</title><pubDate>Mon, 05 Oct 2026 16:50:52 GMT</pubDate></item>
        </channel></rss>"#;
        let a = parse(xml, &source("NZZ", "de"));
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].title, "Erdbeben erschüttert Tokio & Umgebung");
        assert_eq!(a[0].guid, "ld.1");
        assert_eq!(a[0].published, 1_791_219_052);
        assert_eq!(a[0].source, "NZZ");
        assert_eq!(a[0].rank, 0);
    }

    #[test]
    fn splits_nzz_kicker() {
        assert_eq!(
            split_kicker("LIVE-TICKER - Krieg in der Ukraine: Toter bei Luftangriff"),
            Some(("LIVE-TICKER".into(), "Krieg in der Ukraine: Toter bei Luftangriff".into()))
        );
        assert_eq!(split_kicker("Sánchez wagt die Flucht nach vorn - die Wahl"), None);
    }

    #[test]
    fn parses_bbc_cdata_and_strips_tracking() {
        let xml = r#"<rss version="2.0"><channel><item>
            <title><![CDATA[Earthquake hits Tokyo]]></title>
            <description><![CDATA[A strong <b>quake</b> struck Japan's capital.]]></description>
            <link>https://www.bbc.co.uk/news/articles/abc?at_medium=RSS&amp;at_campaign=rss</link>
            <pubDate>Mon, 05 Oct 2026 16:13:35 GMT</pubDate>
        </item></channel></rss>"#;
        let a = parse(xml, &source("BBC", "en"));
        assert_eq!(a[0].title, "Earthquake hits Tokyo");
        assert_eq!(a[0].summary, "A strong quake struck Japan's capital.");
        assert_eq!(a[0].url, "https://www.bbc.co.uk/news/articles/abc");
    }

    #[test]
    fn parses_nyt_categories() {
        let xml = r#"<rss version="2.0"><channel><item>
            <title>Russia Moves to Tamp Down Rumors About Plague Outbreak in Siberia</title>
            <link>https://www.nytimes.com/2026/10/05/world/europe/russia-plague-siberia.html</link>
            <pubDate>Mon, 05 Oct 2026 15:49:14 +0000</pubDate>
            <category domain="http://www.nytimes.com/namespaces/keywords/des">Rumors and Misinformation</category>
            <category domain="http://www.nytimes.com/namespaces/keywords/nyt_geo">Irkutsk (Russia)</category>
            <category domain="http://www.nytimes.com/namespaces/keywords/nyt_geo">Siberia</category>
            <category domain="http://www.nytimes.com/namespaces/keywords/nyt_per">Putin, Vladimir V</category>
        </item></channel></rss>"#;
        let a = parse(xml, &source("NYT", "en"));
        assert_eq!(a[0].geo_tags, vec!["Irkutsk (Russia)", "Siberia"]);
        assert_eq!(a[0].entity_tags, vec!["Putin, Vladimir V"]);
        assert_eq!(a[0].published, 1_791_215_354);
    }
}
