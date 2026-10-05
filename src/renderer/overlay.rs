//! Everything drawn on top of the map: event markers and labels, the event card, the clock and
//! the attribution line. Returns hit regions so the app can handle hover and clicks.

use super::map::View;
use super::text::Weight;
use super::ui::{Color, Rect, TextStyle, Ui};
use crate::dedup::Event;
use time::{OffsetDateTime, UtcOffset};

/// Number of events that get a text label next to their marker.
const LABELED: usize = 6;
/// Article lines listed in the card.
const CARD_ARTICLES: usize = 4;

pub struct Overlay<'a> {
    pub events: &'a [Event],
    /// Event shown in the card, with its opacity (0..1).
    pub card: Option<(usize, f32)>,
    /// Progress (0..1) of the highlight ring animation around the card's marker.
    pub ring: f32,
    /// Card article line under the mouse.
    pub hovered_link: Option<usize>,
    pub now: i64,
    pub utc_offset: UtcOffset,
    pub show_clock: bool,
    /// Label the card with the event's topic (useful when several topics are selected).
    pub show_topics: bool,
    /// Source names for the attribution line.
    pub credits: &'a str,
    /// Markers only (tiny preview in the Screen Saver Settings dialog).
    pub minimal: bool,
}

#[derive(Default)]
pub struct Hits {
    /// Marker centre, radius and event index.
    pub markers: Vec<(f32, f32, f32, usize)>,
    pub card: Option<Rect>,
    /// Clickable article lines in the card.
    pub links: Vec<(Rect, String)>,
}

fn rgba(rgb: [f32; 3], a: f32) -> Color {
    [rgb[0], rgb[1], rgb[2], a]
}

const INK: [f32; 3] = [0.93, 0.95, 0.98];
const MUTED: [f32; 3] = [0.66, 0.71, 0.78];
const MARKER: [f32; 3] = [0.96, 0.97, 1.0];
const ACCENT: [f32; 3] = [0.55, 0.80, 1.0];

pub fn source_color(name: &str) -> [f32; 3] {
    match name {
        "NZZ" => [0.58, 0.78, 0.98],
        "BBC" => [0.98, 0.62, 0.55],
        "NYT" => [0.84, 0.85, 0.90],
        _ => [0.65, 0.90, 0.72],
    }
}

/// "12 MIN", "3 H", "2 D".
pub fn ago(now: i64, t: i64) -> String {
    let m = ((now - t).max(0) / 60) as u64;
    match m {
        0..=1 => "JUST NOW".into(),
        2..=59 => format!("{m} MIN AGO"),
        60..=2879 => format!("{} H AGO", m / 60),
        _ => format!("{} D AGO", m / 1440),
    }
}

pub fn draw(ui: &mut Ui, gl: &glow::Context, view: &View, o: &Overlay) -> Hits {
    let (w, h) = (view.width as f32, view.height as f32);
    let s = (h / 1080.0).min(w / 1920.0).max(0.5);
    let mut hits = Hits::default();

    // ---- markers
    let mut placed: Vec<(f32, f32)> = Vec::new();
    for (i, e) in o.events.iter().enumerate() {
        let (x, y) = view.project(e.location.lat, e.location.lon);
        let (mut x, mut y) = (x as f32, y as f32);
        // Nudge markers that would sit on top of each other (e.g. two events at a country centroid).
        let mut k = 0;
        while k < 12 && placed.iter().any(|(px, py)| (px - x).hypot(py - y) < 9.0 * s) {
            let a = k as f32 * 2.4;
            x += a.cos() * 8.0 * s;
            y += a.sin() * 8.0 * s;
            k += 1;
        }
        placed.push((x, y));
        let sources = e.sources().len() as f32;
        let r = (2.2 + 0.9 * sources) * s;
        let age_h = (o.now - e.latest).max(0) as f32 / 3600.0;
        let fresh = (1.0 - age_h / 72.0).clamp(0.35, 1.0);
        ui.glow(x, y, r * 2.6, rgba(ACCENT, 0.22 * fresh));
        ui.disc(x, y, r, rgba(MARKER, 0.92 * fresh));
        hits.markers.push((x, y, r.max(6.0 * s), i));
    }

    // ---- highlight ring around the card's event
    if let Some((i, alpha)) = o.card {
        if let Some(&(x, y, _, _)) = hits.markers.iter().find(|m| m.3 == i) {
            let t = o.ring.clamp(0.0, 1.0);
            let ease = 1.0 - (1.0 - t).powi(3);
            // A ripple that expands and fades, then a calm steady ring.
            ui.ring(x, y, (8.0 + 22.0 * ease) * s, 1.2 * s, rgba(ACCENT, 0.5 * (1.0 - ease) * alpha));
            ui.ring(x, y, 9.0 * s, 1.3 * s, rgba(ACCENT, 0.8 * alpha * ease.max(0.3)));
        }
    }

    if o.minimal {
        ui.flush(gl, w, h);
        return hits;
    }

    // ---- labels for the top events
    let label = TextStyle { weight: Weight::Medium, size: (10.5 * s).round(), color: rgba(MUTED, 0.75), tracking: 1.4 * s };
    let mut taken: Vec<Rect> = hits.markers.iter().map(|&(x, y, r, _)| Rect { x: x - r, y: y - r, w: 2.0 * r, h: 2.0 * r }).collect();
    for &(x, y, _, i) in hits.markers.iter().take(LABELED) {
        if o.card.is_some_and(|(c, a)| c == i && a > 0.05) {
            continue; // the card already names it
        }
        let text = o.events[i].location.name.to_uppercase();
        let tw = ui.fonts.measure(label.weight, label.size, label.tracking, &text);
        let th = label.size;
        let candidates = [(x + 9.0 * s, y + th * 0.35), (x - 9.0 * s - tw, y + th * 0.35)];
        for (lx, ly) in candidates {
            let r = Rect { x: lx - 2.0, y: ly - th, w: tw + 4.0, h: th + 4.0 };
            if r.x > 8.0 && r.x + r.w < w - 8.0 && !taken.iter().any(|t| t.intersects(&r)) {
                ui.text(gl, label, lx, ly, &text);
                taken.push(r);
                break;
            }
        }
    }

    // ---- event card
    if let Some((i, alpha)) = o.card.filter(|(_, a)| *a > 0.01) {
        let e = &o.events[i];
        let margin = 44.0 * s;
        let pad = 22.0 * s;
        let card_w = (560.0 * s).min(w * 0.4);
        let inner = card_w - 2.0 * pad;

        let meta = TextStyle { weight: Weight::Medium, size: (10.5 * s).round(), color: rgba(ACCENT, 0.85 * alpha), tracking: 1.5 * s };
        let head = TextStyle { weight: Weight::Medium, size: (23.0 * s).round(), color: rgba(INK, alpha), tracking: 0.0 };
        let body = TextStyle { weight: Weight::Regular, size: (14.0 * s).round(), color: rgba(MUTED, 0.85 * alpha), tracking: 0.0 };
        let chip = TextStyle { weight: Weight::Medium, size: (10.0 * s).round(), color: [0.0; 4], tracking: 1.2 * s };
        let item = TextStyle { weight: Weight::Regular, size: (13.0 * s).round(), color: rgba(INK, 0.72 * alpha), tracking: 0.0 };

        let lead = &e.articles[0];
        let place = e.location.name.to_uppercase();
        let meta_text = match e.location.country.as_str() {
            c if c.is_empty() || !e.location.precise || c.eq_ignore_ascii_case(&e.location.name) => place,
            c => format!("{place}  ·  {}", c.to_uppercase()),
        };
        let lead_meta = if o.show_topics && !lead.topic.is_empty() {
            format!("{}  ·  {}  ·  {}", lead.topic.to_uppercase(), lead.source, ago(o.now, lead.published))
        } else {
            format!("{}  ·  {}", lead.source, ago(o.now, lead.published))
        };
        let head_lines = ui.fonts.wrap(head.weight, head.size, &lead.title, inner, 3);
        let body_lines = if lead.summary.is_empty() { Vec::new() } else { ui.fonts.wrap(body.weight, body.size, &lead.summary, inner, 2) };
        // The lead article is the headline itself; the list shows the other reports of this event.
        let others: Vec<_> = e.articles.iter().skip(1).take(CARD_ARTICLES).collect();
        let hidden = e.articles.len().saturating_sub(1 + CARD_ARTICLES);

        let head_lh = head.size * 1.22;
        let body_lh = body.size * 1.45;
        let item_lh = item.size * 1.9;
        let mut height = pad + meta.size + 12.0 * s + head_lh * head_lines.len() as f32;
        if !body_lines.is_empty() {
            height += 8.0 * s + body_lh * body_lines.len() as f32;
        }
        if others.is_empty() {
            height += pad * 0.6;
        } else {
            height += 16.0 * s + item_lh * others.len() as f32 + pad * 0.6;
        }
        if hidden > 0 {
            height += item_lh * 0.8;
        }

        let card = Rect { x: margin, y: h - margin - height, w: card_w, h: height };
        ui.rect(Rect { x: card.x - 1.0, y: card.y - 1.0, w: card.w + 2.0, h: card.h + 2.0 }, 15.0 * s, [1.0, 1.0, 1.0, 0.07 * alpha]);
        ui.rect(card, 14.0 * s, [0.028, 0.036, 0.050, 0.80 * alpha]);
        hits.card = Some(card);

        let x = card.x + pad;
        let mut y = card.y + pad + meta.size;
        ui.text(gl, meta, x, y, &meta_text);
        let lead_style = TextStyle { color: rgba(source_color(&lead.source), 0.9 * alpha), ..meta };
        let lw = ui.fonts.measure(lead_style.weight, lead_style.size, lead_style.tracking, &lead_meta);
        ui.text(gl, lead_style, x + inner - lw, y, &lead_meta);
        let head_top = y;
        y += 12.0 * s;
        for line in &head_lines {
            y += head_lh;
            ui.text(gl, head, x, y - head_lh * 0.22, line);
        }
        hits.links.push((Rect { x: card.x, y: head_top, w: card.w, h: y - head_top }, lead.url.clone()));
        if !body_lines.is_empty() {
            y += 8.0 * s;
            for line in &body_lines {
                y += body_lh;
                ui.text(gl, body, x, y - body_lh * 0.3, line);
            }
        }
        let chip_w = 44.0 * s;
        if !others.is_empty() {
            y += 10.0 * s;
            ui.rect(Rect { x, y, w: inner, h: 1.0 }, 0.0, [1.0, 1.0, 1.0, 0.08 * alpha]);
            y += 6.0 * s;
        }
        for (k, a) in others.iter().enumerate() {
            let k = k + 1; // link 0 is the headline
            let row = Rect { x: card.x, y, w: card.w, h: item_lh };
            let hovered = o.hovered_link == Some(k);
            if hovered {
                ui.rect(Rect { x: card.x + 6.0 * s, y, w: card.w - 12.0 * s, h: item_lh }, 8.0 * s, [1.0, 1.0, 1.0, 0.05 * alpha]);
            }
            let base = y + item_lh * 0.66;
            let c = source_color(&a.source);
            ui.text(gl, TextStyle { color: rgba(c, 0.95 * alpha), ..chip }, x, base, &a.source);
            let t = ui.fonts.ellipsize(item.weight, item.size, &a.title, inner - chip_w - 70.0 * s);
            let style = if hovered { TextStyle { color: rgba(INK, alpha), ..item } } else { item };
            ui.text(gl, style, x + chip_w, base, &t);
            let when = ago(o.now, a.published).replace(" AGO", "");
            let ww = ui.fonts.measure(chip.weight, chip.size, chip.tracking, &when);
            ui.text(gl, TextStyle { color: rgba(MUTED, 0.6 * alpha), ..chip }, x + inner - ww, base, &when);
            hits.links.push((row, a.url.clone()));
            y += item_lh;
        }
        if hidden > 0 {
            let more = format!("+{hidden} MORE");
            ui.text(gl, TextStyle { color: rgba(MUTED, 0.6 * alpha), ..chip }, x + chip_w, y + item_lh * 0.5, &more);
        }
    }

    // ---- clock (top right)
    if o.show_clock {
        let local = OffsetDateTime::from_unix_timestamp(o.now).unwrap_or(OffsetDateTime::UNIX_EPOCH).to_offset(o.utc_offset);
        let time_text = format!("{:02}:{:02}", local.hour(), local.minute());
        let off = o.utc_offset.whole_minutes();
        let zone = if off == 0 { "UTC".to_string() } else if off % 60 == 0 { format!("UTC{:+}", off / 60) } else { format!("UTC{:+}:{:02}", off / 60, (off % 60).abs()) };
        let date_text = format!(
            "{} {} {}  ·  {}",
            &local.weekday().to_string()[..3],
            local.day(),
            &local.month().to_string()[..3],
            zone
        )
        .to_uppercase();
        let big = TextStyle { weight: Weight::Regular, size: (34.0 * s).round(), color: rgba(INK, 0.82), tracking: 0.5 * s };
        let small = TextStyle { weight: Weight::Medium, size: (10.0 * s).round(), color: rgba(MUTED, 0.6), tracking: 1.5 * s };
        let margin = 44.0 * s;
        let tw = ui.fonts.measure(big.weight, big.size, big.tracking, &time_text);
        let dw = ui.fonts.measure(small.weight, small.size, small.tracking, &date_text);
        let right = w - margin;
        ui.text(gl, big, right - tw, margin + big.size * 0.8, &time_text);
        ui.text(gl, small, right - dw, margin + big.size * 0.8 + 20.0 * s, &date_text);
    }

    // ---- attribution (bottom right)
    let credit = TextStyle { weight: Weight::Regular, size: (10.0 * s).round(), color: rgba(MUTED, 0.38), tracking: 0.6 * s };
    let text = format!("{}   ·   Night lights NASA Black Marble   ·   Natural Earth", o.credits);
    let cw = ui.fonts.measure(credit.weight, credit.size, credit.tracking, &text);
    ui.text(gl, credit, w - 44.0 * s - cw, h - 30.0 * s, &text);

    ui.flush(gl, w, h);
    hits
}
