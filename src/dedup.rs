//! Cluster articles from several sources into map events.
//!
//! Each article becomes a sparse feature vector:
//! - content words of title (weight 2) and summary (1), accent-folded, without stop words
//! - language-neutral concepts ("wahl" and "election" both add `#election`)
//! - its location (`@CH`, `@zurich`)
//! - feed-provided people/organisations (NYT)
//!
//! Features are IDF-weighted over the current article set, so rare shared words such as a name
//! ("Fairford", "Bolsonaro") count far more than common ones. Articles in different languages are
//! compared only on features seen in both languages. Two articles are linked when they were
//! published within 48 h of each other, share at least one rare specific feature, and are similar
//! enough on average to the cluster. Nearby articles (same country or < 500 km) need less textual
//! similarity than distant ones.

use crate::geolocation::Location;
use crate::news::Article;
use std::collections::HashMap;

const MAX_GAP_SECS: i64 = 48 * 3600;
const NEAR_KM: f64 = 500.0;
/// Similarity needed for nearby articles.
const SIM_NEAR: f64 = 0.26;
/// Similarity needed regardless of location.
const SIM_ANY: f64 = 0.45;

#[derive(Clone, Debug)]
pub struct Event {
    pub location: Location,
    /// Best article first (preferred source, then most recent).
    pub articles: Vec<Article>,
    pub score: f64,
    /// Unix time of the newest article.
    pub latest: i64,
}

impl Event {
    #[cfg(test)]
    pub fn headline(&self) -> &str {
        &self.articles[0].title
    }

    /// Distinct source names, in article order.
    pub fn sources(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for a in &self.articles {
            if !out.contains(&a.source.as_str()) {
                out.push(&a.source);
            }
        }
        out
    }
}

/// Language-neutral concepts: (concept, word stems). A stem matches the start of a word, or anywhere
/// inside it when it's at least 6 letters (German compounds: "Schülerproteste" contains "protest").
const CONCEPTS: &[(&str, &[&str])] = &[
    ("election", &["wahl", "election", "vote", "voter", "wähler", "abstimmung", "ballot", "runoff"]),
    ("earthquake", &["erdbeben", "beben", "earthquake", "quake", "tsunami"]),
    ("war", &["krieg", "warfare", "wartime"]),
    ("attack", &["angriff", "attack", "strike", "anschlag", "assault", "bombard", "beschuss"]),
    ("ceasefire", &["waffenruhe", "waffenstillstand", "ceasefire", "truce"]),
    ("protest", &["protest", "demonstr", "kundgebung", "unrest", "unruhen"]),
    ("bomber", &["bomber"]),
    ("drone", &["drohne", "drone"]),
    ("school", &["schul", "schüler", "school", "student", "pupil"]),
    ("plague", &["pest", "plague"]),
    ("epidemic", &["epidemi", "pandemi", "outbreak", "ausbruch", "seuche"]),
    ("ebola", &["ebola"]),
    ("nobel", &["nobel"]),
    ("medicine", &["medizin", "medicine", "physiolog"]),
    ("housing", &["wohnung", "housing", "miete", "rent"]),
    ("migration", &["migrant", "migration", "flüchtling", "refugee", "asyl", "asylum"]),
    ("hostage", &["geisel", "hostage", "entführ", "kidnap", "abduct"]),
    ("shooting", &["schiess", "schuss", "shooting", "gunman", "shot"]),
    ("murder", &["mord", "murder", "tötung", "killing", "homicide"]),
    ("arrest", &["festnahme", "festgenommen", "verhaft", "arrest", "detain"]),
    ("president", &["präsident", "president"]),
    ("headofgov", &["regierungschef", "premier", "prime", "kanzler", "chancellor"]),
    ("parliament", &["parlament", "parliament", "congress", "kongress"]),
    ("sanctions", &["sanktion", "sanction"]),
    ("tariff", &["zoll", "zölle", "tariff"]),
    ("oil", &["ölpreis", "erdöl", "oil", "pipeline"]),
    ("flood", &["überschwemm", "hochwasser", "flood", "flut"]),
    ("fire", &["waldbrand", "wildfire", "brand", "blaze"]),
    ("storm", &["sturm", "storm", "hurrikan", "hurricane", "taifun", "typhoon", "tornado", "zyklon", "cyclone"]),
    ("crash", &["absturz", "abgestürzt", "crash", "collision"]),
    ("aviation", &["flugzeug", "plane", "flight", "flug", "pilot", "airline", "airport", "flughafen", "cockpit"]),
    ("nuclear", &["atom", "nuclear", "nuklear", "reaktor", "reactor"]),
    ("coup", &["putsch", "coup", "junta"]),
    ("resign", &["rücktritt", "resign", "zurückgetreten"]),
    ("dead", &["tote", "getötet", "killed", "dead", "death", "starb", "gestorben", "died", "dies"]),
    ("military", &["militär", "military", "armee", "army", "truppen", "troops", "soldat", "soldier"]),
    ("airbase", &["luftwaffe", "stützpunkt", "air base", "airbase", "base"]),
    ("marine", &["marine", "navy"]),
    ("pope", &["papst", "pope", "vatikan", "vatican"]),
    ("moon", &["mond", "moon", "lunar"]),
    ("robot", &["roboter", "robot"]),
    ("cartel", &["kartell", "cartel"]),
    ("weapons", &["waffen", "weapon", "guns", "gun", "rifle"]),
    ("terror", &["terror", "extremis", "dschihad", "jihad"]),
    ("separatist", &["separatist", "unabhängig", "independence", "autonom"]),
    ("court", &["gericht", "court", "urteil", "verdict", "richter", "judge", "anklage", "charged"]),
    ("hunger", &["hunger", "starv", "famine"]),
    ("economy", &["wirtschaft", "economy", "economic", "inflation", "rezession", "recession"]),
    ("talks", &["verhandlung", "gespräch", "talks", "negotiat", "summit", "gipfel"]),
];

const STOP: &[&str] = &[
    // English
    "about", "after", "again", "against", "also", "amid", "among", "been", "before", "being", "between", "both",
    "could", "does", "during", "each", "even", "every", "first", "from", "have", "here", "into", "just", "last",
    "least", "like", "made", "make", "many", "more", "most", "much", "must", "near", "never", "only", "other",
    "over", "said", "says", "same", "should", "since", "some", "still", "such", "than", "that", "their", "them",
    "then", "there", "these", "they", "this", "those", "three", "through", "under", "until", "very", "want",
    "were", "what", "when", "where", "which", "while", "will", "with", "would", "year", "years", "your", "what's",
    "know", "news", "watch", "live", "people", "says", "told", "report", "reports", "according", "officials",
    "government", "new", "week", "today", "time", "first", "second", "says", "following", "inside",
    // German
    "aber", "alle", "allem", "allen", "aller", "alles", "als", "also", "andere", "anderen", "auch", "auf", "aus",
    "bei", "beim", "bereits", "bis", "bisher", "dabei", "damit", "dann", "darauf", "darum", "dass", "dem", "den",
    "denn", "der", "des", "die", "dies", "diese", "diesem", "diesen", "dieser", "dieses", "doch", "dort", "durch",
    "eine", "einem", "einen", "einer", "eines", "einige", "etwa", "etwas", "für", "gegen", "geht", "gibt", "habe",
    "haben", "hat", "hatte", "hätte", "heute", "ihre", "ihren", "ihrer", "immer", "jahr", "jahre", "jahren",
    "jedoch", "jetzt", "kann", "kein", "keine", "können", "könnte", "lässt", "mehr", "muss", "nach", "neue",
    "neuen", "neuer", "nicht", "noch", "nun", "oder", "ohne", "schon", "sehr", "seien", "sein", "seine", "seiner",
    "seit", "sich", "sind", "soll", "sollen", "sollte", "sowie", "über", "um", "und", "uns", "unter", "viel",
    "viele", "vom", "von", "vor", "während", "war", "waren", "warum", "was", "weil", "weiter", "wenn", "werden",
    "wie", "wieder", "will", "wird", "wurde", "wurden", "zum", "zur", "zwei", "zwischen", "sagt", "neueste",
    "neuesten", "meldungen", "entwicklungen",
];

/// Lower-case and fold accents so "Sánchez" == "Sanchez" and "Zürich" == "Zurich".
fn fold(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        })
        .collect::<String>()
        .replace('ß', "ss")
}

fn concept_matches(word: &str, stem: &str) -> bool {
    word.starts_with(stem) || (stem.chars().count() >= 6 && word.contains(stem))
}

fn features(a: &Article, loc: &Location) -> HashMap<String, f64> {
    let mut f: HashMap<String, f64> = HashMap::new();
    let mut add = |k: String, w: f64| *f.entry(k).or_default() += w;
    for (text, w) in [(&a.title, 2.0), (&a.summary, 1.0)] {
        let lower = text.to_lowercase();
        for word in lower.split(|c: char| !c.is_alphanumeric()) {
            for (concept, stems) in CONCEPTS {
                if stems.iter().any(|s| concept_matches(word, s)) {
                    add(format!("#{concept}"), w);
                }
            }
            // Numbers are good anchors ("B-52", "170 killed"), except years.
            let is_year = word.len() == 4 && (word.starts_with("19") || word.starts_with("20"));
            let is_number = word.len() >= 2 && word.chars().all(|c| c.is_ascii_digit()) && !is_year;
            if !is_year && ((word.chars().count() >= 4 && !STOP.contains(&word)) || is_number) {
                add(fold(word), w);
            }
        }
    }
    for tag in &a.entity_tags {
        // "Putin, Vladimir V" -> "putin", "vladimir"
        for word in tag.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() >= 4) {
            add(fold(word), 1.0);
        }
    }
    add(format!("@{}", loc.iso), 1.0);
    if loc.precise {
        add(format!("@{}", fold(&loc.name)), 1.5);
    }
    f
}

fn cosine(a: &HashMap<String, f64>, b: &HashMap<String, f64>) -> f64 {
    let dot: f64 = a.iter().filter_map(|(k, x)| b.get(k).map(|y| x * y)).sum();
    let norm = |v: &HashMap<String, f64>| v.values().map(|x| x * x).sum::<f64>().sqrt();
    let d = norm(a) * norm(b);
    if d > 0.0 {
        dot / d
    } else {
        0.0
    }
}

pub fn distance_km(a: &Location, b: &Location) -> f64 {
    let (la1, lo1, la2, lo2) = (a.lat.to_radians(), a.lon.to_radians(), b.lat.to_radians(), b.lon.to_radians());
    let h = ((la2 - la1) / 2.0).sin().powi(2) + la1.cos() * la2.cos() * ((lo2 - lo1) / 2.0).sin().powi(2);
    2.0 * 6371.0 * h.sqrt().asin()
}

/// Group articles into events, score them and return them best first.
/// `source_order` lists preferred sources first (the event headline comes from the first one).
pub fn cluster(items: Vec<(Article, Location)>, source_order: &[String], now: i64) -> Vec<Event> {
    let n = items.len();
    let raw: Vec<HashMap<String, f64>> = items.iter().map(|(a, l)| features(a, l)).collect();

    // IDF over the current article set.
    let mut df: HashMap<&str, f64> = HashMap::new();
    for f in &raw {
        for k in f.keys() {
            *df.entry(k).or_default() += 1.0;
        }
    }
    let vecs: Vec<HashMap<String, f64>> = raw
        .iter()
        .map(|f| f.iter().map(|(k, w)| (k.clone(), w * ((n as f64 + 1.0) / df[k.as_str()]).ln())).collect())
        .collect();

    // Cross-language pairs are compared only on vocabulary that occurs in both languages in the
    // current corpus (names, places, numbers, concepts); German-only and English-only words would
    // otherwise drown the shared signal.
    let mut langs_of: HashMap<&str, Vec<&str>> = HashMap::new();
    for (f, (a, _)) in raw.iter().zip(&items) {
        for k in f.keys() {
            let l = langs_of.entry(k).or_default();
            if !l.contains(&a.lang.as_str()) {
                l.push(&a.lang);
            }
        }
    }
    let shared: Vec<HashMap<String, f64>> = vecs
        .iter()
        .map(|v| v.iter().filter(|(k, _)| langs_of[k.as_str()].len() > 1).map(|(k, w)| (k.clone(), *w)).collect())
        .collect();

    let sim = |i: usize, j: usize| {
        if items[i].0.lang == items[j].0.lang {
            cosine(&vecs[i], &vecs[j])
        } else {
            cosine(&shared[i], &shared[j])
        }
    };
    // A link also needs specific shared evidence: two rare names/numbers/places, or one plus two
    // shared concepts. Shared concepts or countries alone ("election" in "Spain") are not enough.
    let rare_df = 3.0f64.max(n as f64 / 8.0);
    let anchored = |i: usize, j: usize| {
        let (a, b) = (&raw[i], &raw[j]);
        let mut specific: Vec<&str> = Vec::new();
        let mut concepts = 0;
        for k in a.keys().filter(|k| b.contains_key(*k)) {
            if k.starts_with('#') {
                concepts += 1;
            } else if !(k.starts_with('@') && k.len() == 3) && df[k.as_str()] <= rare_df {
                // "@mumbai" and "mumbai" are the same evidence.
                let name = k.trim_start_matches('@');
                if !specific.contains(&name) {
                    specific.push(name);
                }
            }
        }
        specific.len() >= 2 || (specific.len() == 1 && concepts >= 2)
    };
    let near = |i: usize, j: usize| {
        let (la, lb) = (&items[i].1, &items[j].1);
        la.iso == lb.iso || distance_km(la, lb) < NEAR_KM
    };

    // Agglomerative average-linkage: repeatedly merge the most similar pair of clusters. Unlike
    // single linkage this doesn't chain unrelated stories through one ambiguous article.
    let sims: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| if i == j { 1.0 } else { sim(i, j) }).collect()).collect();
    let mut clusters: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    loop {
        let mut best: Option<(usize, usize, f64)> = None;
        for a in 0..clusters.len() {
            for b in a + 1..clusters.len() {
                let pairs = || clusters[a].iter().flat_map(|&i| clusters[b].iter().map(move |&j| (i, j)));
                if pairs().any(|(i, j)| (items[i].0.published - items[j].0.published).abs() > MAX_GAP_SECS) {
                    continue;
                }
                let avg = pairs().map(|(i, j)| sims[i][j]).sum::<f64>() / (clusters[a].len() * clusters[b].len()) as f64;
                if best.is_some_and(|(_, _, s)| avg <= s) || avg < SIM_NEAR {
                    continue;
                }
                let need = if pairs().any(|(i, j)| near(i, j)) { SIM_NEAR } else { SIM_ANY };
                if avg >= need && pairs().any(|(i, j)| anchored(i, j)) {
                    best = Some((a, b, avg));
                }
            }
        }
        let Some((a, b, _)) = best else { break };
        if std::env::var_os("EPHEMERIS_DEBUG_DEDUP").is_some() {
            for &i in &clusters[a] {
                for &j in &clusters[b] {
                    let shared: Vec<String> = raw[i].keys().filter(|k| raw[j].contains_key(*k)).map(|k| format!("{k}:{}", df[k.as_str()])).collect();
                    eprintln!("merge {:.2} {} <> {} :: {}", sims[i][j], items[i].0.title, items[j].0.title, shared.join(" "));
                }
            }
        }
        let merged = clusters.swap_remove(b);
        clusters[a].extend(merged);
    }

    let source_pos = |s: &str| source_order.iter().position(|x| x == s).unwrap_or(usize::MAX);
    let mut events: Vec<Event> = clusters
        .into_iter()
        .map(|members| {
            let location = representative_location(&members.iter().map(|&i| &items[i].1).collect::<Vec<_>>());
            let mut articles: Vec<Article> = members.iter().map(|&i| items[i].0.clone()).collect();
            articles.sort_by(|a, b| {
                source_pos(&a.source).cmp(&source_pos(&b.source)).then(b.published.cmp(&a.published))
            });
            let latest = articles.iter().map(|a| a.published).max().unwrap_or(0);
            let mut event = Event { location, articles, score: 0.0, latest };
            event.score = score(&event, now);
            event
        })
        .collect();
    events.sort_by(|a, b| b.score.total_cmp(&a.score));
    events
}

/// The most frequently named location; a city/region counts a bit more than a whole country.
fn representative_location(locs: &[&Location]) -> Location {
    let mut best: Option<(&Location, f64)> = None;
    for l in locs {
        let count = locs.iter().filter(|o| o.name == l.name).count() as f64;
        let key = count + if l.precise { 0.5 } else { 0.0 };
        if best.is_none_or(|(_, k)| key > k) {
            best = Some((l, key));
        }
    }
    best.map(|(l, _)| (*l).clone()).expect("non-empty cluster")
}

fn score(e: &Event, now: i64) -> f64 {
    let sources = e.sources().len() as f64;
    let age_h = (now - e.latest).max(0) as f64 / 3600.0;
    let recency = (-age_h / 36.0).exp();
    let top_rank = e.articles.iter().map(|a| a.rank).min().unwrap_or(99) as f64;
    let prominence = 1.0 / (1.0 + top_rank / 8.0);
    let live = if e.articles.iter().any(|a| a.kicker.contains("LIVE")) { 0.5 } else { 0.0 };
    1.6 * sources + 0.4 * (e.articles.len() as f64).ln_1p() + recency + prominence + live
        + if e.location.precise { 0.2 } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geolocation::Gazetteer;

    fn art(source: &str, lang: &str, title: &str, summary: &str, hours_ago: i64) -> Article {
        Article {
            source: source.into(),
            lang: lang.into(),
            title: title.into(),
            summary: summary.into(),
            url: format!("https://example.com/{}", title.len()),
            published: 1_000_000 - hours_ago * 3600,
            ..Default::default()
        }
    }

    fn run(articles: Vec<Article>) -> Vec<Event> {
        let g = Gazetteer::load();
        let items = articles.into_iter().filter_map(|a| g.locate(&a).map(|l| (a, l))).collect();
        cluster(items, &["NZZ".into(), "BBC".into(), "NYT".into()], 1_000_000)
    }

    #[test]
    fn merges_same_event_across_languages() {
        let events = run(vec![
            art("NZZ", "de", "Neue Drohungen gegen die Militärbasis Fairford: Jetzt ziehen die USA ihre Bomber aus England ab",
                "Nach einem Vorfall beim Stützpunkt verlegen die Amerikaner ihre B-52-Bomber.", 3),
            art("BBC", "en", "No10 insists UK military base RAF Fairford is safe after US withdraws bombers",
                "US media reported a new threat led to the bombers being removed on Sunday.", 2),
            art("NYT", "en", "U.S. Rushes to Withdraw Bombers From U.K. Air Base After New Threats",
                "The Pentagon moved B-52 bombers out of RAF Fairford in England.", 1),
            art("NZZ", "de", "Brasiliens Rechte erlebt einen Erdrutschsieg",
                "Flávio Bolsonaro gewinnt die erste Runde der Präsidentschaftswahl in Brasilien.", 5),
            art("BBC", "en", "Right-wing Flávio Bolsonaro wins first round of Brazil election",
                "The son of the former president is heading for a runoff against Lula.", 4),
            art("BBC", "en", "Rare tornado whips through small Australian town", "Residents in Victoria were stunned.", 6),
        ]);
        assert_eq!(events.len(), 3, "{:#?}", events.iter().map(|e| (e.headline(), e.articles.len())).collect::<Vec<_>>());
        let fairford = events.iter().find(|e| e.location.name == "Fairford").expect("fairford event");
        assert_eq!(fairford.sources(), vec!["NZZ", "BBC", "NYT"]);
        let brazil = events.iter().find(|e| e.location.iso == "BR").expect("brazil event");
        assert_eq!(brazil.articles.len(), 2);
        assert_eq!(brazil.headline(), "Brasiliens Rechte erlebt einen Erdrutschsieg");
        // The multi-source events outrank the single-source one.
        assert_eq!(events[2].location.iso, "AU");
    }

    #[test]
    fn keeps_unrelated_stories_in_same_country_apart() {
        let events = run(vec![
            art("NZZ", "de", "Dürre in der Schweiz: Die Pegel an Zürichsee und Bodensee sinken so tief wie noch nie", "", 1),
            art("NZZ", "de", "Guy Parmelins Nachfolge im Bundesrat: Das sind die Kronfavoriten in der SVP", "", 2),
            art("NZZ", "de", "Tamedia baut in Zürich 34 Vollzeitstellen ab", "", 3),
        ]);
        assert_eq!(events.len(), 3);
    }
}
