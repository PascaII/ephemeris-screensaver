//! Assign coordinates to articles by finding place names in their text.
//!
//! Offline heuristics over an embedded gazetteer (Natural Earth countries and cities with English
//! and German names, plus `assets/aliases.tsv` for demonyms, regions and institutions):
//!
//! 1. Collect mentions from feed geo tags (weight 3), the title (2) and the summary (1).
//!    A preceding locative preposition ("in", "bei", "near", ...) adds 1. People/parties count half.
//! 2. Sum weights per country; ambiguous names are resolved towards the strongest country.
//! 3. Pick the strongest country, then its most mentioned city/region, else the country itself.

use crate::news::Article;
use std::collections::{HashMap, HashSet};

static GAZETTEER: &str = include_str!("../assets/gazetteer.tsv");
static ALIASES: &str = include_str!("../assets/aliases.tsv");

const PREPOSITIONS: [&str; 10] = ["in", "im", "near", "bei", "aus", "at", "off", "vor", "outside", "nahe"];
const MAX_NGRAM: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Country,
    City,
    Region,
}

#[derive(Debug)]
pub struct Place {
    pub name: String,
    pub iso: String,
    pub lat: f64,
    pub lon: f64,
    pub pop: u64,
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub name: String,
    pub iso: String,
    pub lat: f64,
    pub lon: f64,
    /// City or region rather than a whole country.
    pub precise: bool,
}

#[derive(Clone, Copy)]
struct Entry {
    place: usize,
    weak: bool,
}

pub struct Gazetteer {
    places: Vec<Place>,
    /// Normalised name -> candidates.
    names: HashMap<String, Vec<Entry>>,
    /// Names that must match with exactly this capitalisation (abbreviations like "US").
    exact_case: HashSet<String>,
    /// Lower-case word prefixes (German adjective stems).
    stems: Vec<(String, Entry)>,
    stop: HashSet<String>,
}

/// Lower-case, ß -> ss, and split into words on anything that isn't a letter or digit.
fn words(s: &str) -> Vec<&str> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect()
}

fn norm(words: &[&str]) -> String {
    words.iter().map(|w| w.to_lowercase().replace('ß', "ss")).collect::<Vec<_>>().join(" ")
}

impl Gazetteer {
    pub fn load() -> Gazetteer {
        let mut g = Gazetteer {
            places: Vec::new(),
            names: HashMap::new(),
            exact_case: HashSet::new(),
            stems: Vec::new(),
            stop: HashSet::new(),
        };
        let mut country_of: HashMap<String, usize> = HashMap::new();
        let rows = GAZETTEER.lines().chain(ALIASES.lines());
        for line in rows.filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let f: Vec<&str> = line.split('\t').collect();
            let [kind, iso, lat, lon, rest @ ..] = f.as_slice() else { continue };
            let (pop, names) = match rest {
                [pop, names] => (pop.parse().unwrap_or(0), *names),
                [names] => (0, *names),
                _ => continue,
            };
            let place = match *kind {
                "S" => {
                    for n in names.split('|') {
                        g.stop.insert(norm(&words(n)));
                    }
                    continue;
                }
                "X" => match country_of.get(*iso) {
                    Some(&i) => {
                        g.places[i].name = names.split('|').next().unwrap_or("").to_string();
                        i
                    }
                    None => continue,
                },
                _ => {
                    let kind = match *kind {
                        "C" => Kind::Country,
                        "P" => Kind::City,
                        _ => Kind::Region,
                    };
                    let name = names.split('|').next().unwrap_or("").trim_start_matches('~').to_string();
                    g.places.push(Place {
                        name,
                        iso: iso.to_string(),
                        lat: lat.parse().unwrap_or(0.0),
                        lon: lon.parse().unwrap_or(0.0),
                        pop,
                        kind,
                    });
                    let i = g.places.len() - 1;
                    if kind == Kind::Country {
                        country_of.entry(iso.to_string()).or_insert(i);
                    }
                    i
                }
            };
            for raw in names.split('|') {
                let weak = raw.starts_with('~');
                let raw = raw.trim_start_matches('~');
                let entry = Entry { place, weak };
                if let Some(stem) = raw.strip_suffix('*') {
                    g.stems.push((stem.to_lowercase(), entry));
                    continue;
                }
                let w = words(raw);
                if w.is_empty() || w.len() > MAX_NGRAM {
                    continue;
                }
                let key = norm(&w);
                if raw.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase()) && raw.len() <= 5 {
                    g.exact_case.insert(w.join(" "));
                }
                let list = g.names.entry(key).or_default();
                if !list.iter().any(|e| e.place == place) {
                    list.push(entry);
                }
            }
        }
        g
    }

    /// Find place mentions in `text`, each with its weight.
    fn mentions(&self, text: &str, weight: f64, german: bool, out: &mut Vec<(Vec<Entry>, f64)>) {
        let toks = words(text);
        let mut i = 0;
        while i < toks.len() {
            let boost = if i > 0 && PREPOSITIONS.contains(&toks[i - 1].to_lowercase().as_str()) { 1.0 } else { 0.0 };
            let mut matched = 0;
            for n in (1..=MAX_NGRAM.min(toks.len() - i)).rev() {
                let span = &toks[i..i + n];
                if let Some(entries) = self.lookup(span, german) {
                    out.push((entries, weight + boost));
                    matched = n;
                    break;
                }
            }
            if matched == 0 {
                let lower = toks[i].to_lowercase();
                if let Some((_, e)) = self.stems.iter().find(|(s, _)| lower.starts_with(s.as_str())) {
                    out.push((vec![*e], weight + boost));
                }
                matched = 1;
            }
            i += matched;
        }
    }

    fn lookup(&self, span: &[&str], german: bool) -> Option<Vec<Entry>> {
        let capitalised = span[0].chars().next().is_some_and(char::is_uppercase);
        if !capitalised {
            return None;
        }
        let key = norm(span);
        if self.stop.contains(&key) {
            return None;
        }
        let candidates = |key: &str| -> Option<Vec<Entry>> {
            let list = self.names.get(key)?;
            let joined = span.join(" ");
            // "US" must not match the English pronoun "us" or "Us".
            if self.exact_case.iter().any(|e| e.to_lowercase() == key) && !self.exact_case.contains(&joined) {
                return None;
            }
            Some(list.clone())
        };
        candidates(&key).or_else(|| {
            // German genitive: "Brasiliens", "Chinas", "Kiews".
            (german && span.len() == 1 && key.len() > 4 && key.ends_with('s'))
                .then(|| candidates(&key[..key.len() - 1]))
                .flatten()
        })
    }

    pub fn locate(&self, a: &Article) -> Option<Location> {
        let german = a.lang == "de";
        let mut mentions: Vec<(Vec<Entry>, f64)> = Vec::new();
        for tag in &a.geo_tags {
            // NYT tags look like "Irkutsk (Russia)" or "Siberia".
            let (name, country) = match tag.split_once(" (") {
                Some((n, c)) => (n, Some(c.trim_end_matches(')'))),
                None => (tag.as_str(), None),
            };
            let before = mentions.len();
            self.mentions(name, 3.0, false, &mut mentions);
            if mentions.len() == before {
                if let Some(c) = country {
                    self.mentions(c, 3.0, false, &mut mentions);
                }
            }
        }
        self.mentions(&a.title, 2.0, german, &mut mentions);
        self.mentions(&a.summary, 1.0, german, &mut mentions);
        self.resolve(&mentions)
    }

    fn resolve(&self, mentions: &[(Vec<Entry>, f64)]) -> Option<Location> {
        let weight = |e: &Entry, w: f64| if e.weak { w * 0.5 } else { w };
        // Pass 1: unambiguous mentions (all candidates in one country) vote for their country.
        let mut country: HashMap<&str, f64> = HashMap::new();
        for (entries, w) in mentions {
            let iso = &self.places[entries[0].place].iso;
            if entries.iter().all(|e| &self.places[e.place].iso == iso) {
                *country.entry(iso).or_default() += weight(&entries[0], *w);
            }
        }
        // Pass 2: ambiguous mentions pick the candidate in the strongest country, else the largest.
        let mut chosen: Vec<(usize, f64)> = Vec::new();
        for (entries, w) in mentions {
            let best = entries
                .iter()
                .max_by(|a, b| {
                    let score = |e: &Entry| country.get(self.places[e.place].iso.as_str()).copied().unwrap_or(0.0);
                    score(a).total_cmp(&score(b)).then(self.places[a.place].pop.cmp(&self.places[b.place].pop))
                })
                .unwrap();
            if entries.len() > 1 && !entries.iter().all(|e| self.places[e.place].iso == self.places[best.place].iso) {
                *country.entry(&self.places[best.place].iso).or_default() += weight(best, *w);
            }
            chosen.push((best.place, weight(best, *w)));
        }

        // Strongest country; ties go to the first mentioned.
        let mut best_iso: Option<(&str, f64)> = None;
        for (p, _) in &chosen {
            let iso = self.places[*p].iso.as_str();
            let s = country[iso];
            if best_iso.is_none_or(|(_, b)| s > b) {
                best_iso = Some((iso, s));
            }
        }
        let (iso, score) = best_iso?;
        if score < 1.0 {
            return None;
        }

        // Most specific place inside that country (in order of first mention, so ties are stable).
        let mut specific: Vec<(usize, f64)> = Vec::new();
        for (p, w) in &chosen {
            let pl = &self.places[*p];
            if pl.iso == iso && pl.kind != Kind::Country {
                match specific.iter_mut().find(|(q, _)| q == p) {
                    Some((_, sum)) => *sum += w,
                    None => specific.push((*p, *w)),
                }
            }
        }
        let place = specific
            .iter()
            .rev() // max_by returns the last maximum; reversing makes it the first mentioned
            .max_by(|a, b| a.1.total_cmp(&b.1).then(self.places[a.0].pop.cmp(&self.places[b.0].pop)))
            .map(|(p, _)| *p)
            .or_else(|| chosen.iter().map(|(p, _)| *p).find(|p| self.places[*p].iso == iso))?;
        let pl = &self.places[place];
        Some(Location {
            name: pl.name.clone(),
            iso: pl.iso.clone(),
            lat: pl.lat,
            lon: pl.lon,
            precise: pl.kind != Kind::Country,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn article(lang: &str, title: &str, summary: &str) -> Article {
        Article { lang: lang.into(), title: title.into(), summary: summary.into(), ..Default::default() }
    }

    fn loc(g: &Gazetteer, lang: &str, title: &str, summary: &str) -> Option<String> {
        g.locate(&article(lang, title, summary)).map(|l| l.name)
    }

    #[test]
    fn locates_real_headlines() {
        let g = Gazetteer::load();
        let cases = [
            ("en", "No10 insists UK military base RAF Fairford is safe after US withdraws bombers", "Fairford"),
            ("de", "Neue Drohungen gegen die Militärbasis Fairford: Jetzt ziehen die USA ihre Bomber aus England ab", "Fairford"),
            ("en", "'Not again': Arrest of US marine for murder reignites protests in Japan's Okinawa", "Okinawa"),
            ("de", "Brasiliens Rechte erlebt einen Erdrutschsieg", "Brazil"),
            ("en", "Right-wing Flávio Bolsonaro wins first round of Brazil election", "Brazil"),
            ("de", "Äthiopische Truppen erobern Mekele: Der Kampf um die Region Tigray eskaliert", "Tigray"),
            ("en", "Yemeni military says it has retaken Mokha and 'secured' Red Sea waterway", "Mokha"),
            ("de", "Keine Chance für Putinisten: In Lettland gewinnen proukrainische Kräfte die Wahlen mit Abstand", "Latvia"),
            ("en", "Bombs preventing rescue of kidnapped youths, Nigerian police say", "Nigeria"),
            ("de", "Weshalb in Ceuta noch immer Tausende Migranten auf der Strasse schlafen", "Ceuta"),
            ("en", "Spanish PM Sánchez calls early election after housing protests", "Spain"),
        ];
        for (lang, title, want) in cases {
            assert_eq!(loc(&g, lang, title, "").as_deref(), Some(want), "{title}");
        }
    }

    #[test]
    fn ignores_pronouns_and_common_words() {
        let g = Gazetteer::load();
        assert_eq!(loc(&g, "en", "Tell us what you think", "Join us for a chat"), None);
        assert_eq!(loc(&g, "de", "Gutes Essen in der Halle", ""), None);
    }

    #[test]
    fn uses_nyt_geo_tags() {
        let g = Gazetteer::load();
        let mut a = article("en", "Russia Moves to Tamp Down Rumors About Plague Outbreak", "");
        a.geo_tags = vec!["Irkutsk (Russia)".into(), "Siberia".into()];
        let l = g.locate(&a).unwrap();
        assert_eq!(l.iso, "RU");
        assert!(l.precise);
    }
}
