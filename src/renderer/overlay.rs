//! Everything drawn on top of the map: event markers and labels, the callout (a headline card set
//! in the nearest open water and joined to its marker by a leader line), the clock and the
//! attribution line. Returns hit regions so the app can handle hover and clicks.
//!
//! Sizes are in 1080p design pixels and scaled by `s`. Colours follow the "Ephemeris Nocturne"
//! design system.

use super::map::{LandGrid, View};
use super::text::Weight;
use super::ui::{Color, Rect, TextStyle, Ui};
use crate::dedup::Event;
use time::{OffsetDateTime, UtcOffset};

/// Number of events that get a text label next to their marker.
const LABELED: usize = 5;
/// Further articles listed under the headline.
const CARD_ARTICLES: usize = 3;
/// Headline size, line height (design px) and the most lines shown before an ellipsis.
const HEAD_SIZE: f32 = 28.0;
const HEAD_LINE: f32 = 34.0;
const HEAD_LINES: usize = 4;
/// Callout widths tried, widest first (design px).
const CARD_WIDTHS: [f32; 2] = [480.0, 420.0];
/// Beyond this distance from its marker the callout gives up on open water and sits beside the place.
const MAX_LEADER: f32 = 320.0;

const INK: [f32; 3] = [0.953, 0.961, 0.969]; // #f3f5f7
const INK_MUTED: [f32; 3] = [0.784, 0.827, 0.871]; // #c8d3de
const INK_FAINT: [f32; 3] = [0.576, 0.631, 0.682]; // #93a1ae
const MARKER: [f32; 3] = [0.902, 0.922, 0.933]; // #e6ebee
const FOCUS: [f32; 3] = [1.0, 1.0, 1.0];
const CREDIT: [f32; 3] = [0.490, 0.541, 0.584]; // #7d8a95
/// Callout ground. The design's glass card uses a backdrop blur at 74%; without the blur we go a
/// little more opaque so headlines stay legible over coastlines and lights.
const CARD: [f32; 4] = [0.012, 0.031, 0.063, 0.82];

pub struct SunInfo {
    pub location: String,
    pub times: String,
}

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
    pub sun: Option<&'a SunInfo>,
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

/// "just now", "12 min", "3 h", "2 d".
pub fn ago(now: i64, t: i64) -> String {
    let m = ((now - t).max(0) / 60) as u64;
    match m {
        0..=1 => "just now".into(),
        2..=59 => format!("{m} min"),
        60..=2879 => format!("{} h", m / 60),
        _ => format!("{} d", m / 1440),
    }
}

/// Display name of a feed topic id ("top" -> "Top stories", "tech" -> "Tech").
fn topic_name(id: &str) -> String {
    match id {
        "top" => "Top stories".into(),
        _ => id.chars().take(1).flat_map(char::to_uppercase).chain(id.chars().skip(1)).collect(),
    }
}

/// Marker radius from the event score: 3 px plus 0.9 px per point above 3.
fn marker_radius(score: f64, s: f32) -> f32 {
    (3.0 + 0.9 * (score as f32 - 3.0).clamp(0.0, 6.0)) * s
}

fn dist_to_rect(x: f32, y: f32, r: &Rect) -> f32 {
    let dx = (r.x - x).max(0.0).max(x - (r.x + r.w));
    let dy = (r.y - y).max(0.0).max(y - (r.y + r.h));
    dx.hypot(dy)
}

/// Text styles and vertical rhythm of the callout.
struct CardStyle {
    s: f32,
    place: TextStyle,
    meta: TextStyle,
    head: TextStyle,
    dek: TextStyle,
    code: TextStyle,
    title: TextStyle,
    age: TextStyle,
}

impl CardStyle {
    fn new(s: f32) -> Self {
        let t = |weight, size: f32, color| TextStyle { weight, size: (size * s).round(), color, tracking: 0.0 };
        CardStyle {
            s,
            place: t(Weight::Semibold, 16.0, rgba(INK_MUTED, 1.0)),
            meta: t(Weight::Regular, 16.0, rgba(INK_FAINT, 1.0)),
            head: t(Weight::Serif, HEAD_SIZE, rgba(INK, 1.0)),
            dek: t(Weight::Regular, 16.0, rgba(INK_MUTED, 1.0)),
            code: t(Weight::Semibold, 15.0, rgba(INK, 1.0)),
            title: t(Weight::Regular, 15.0, rgba(INK_MUTED, 1.0)),
            age: t(Weight::Regular, 15.0, rgba(INK_FAINT, 1.0)),
        }
    }
    fn px(&self, v: f32) -> f32 {
        v * self.s
    }
}

struct Row {
    code: String,
    lines: Vec<String>,
    age: String,
    url: String,
}

/// The callout's text, wrapped for one width, and the height that results.
struct CardLayout {
    w: f32,
    h: f32,
    place: String,
    meta: String,
    head: Vec<String>,
    head_url: String,
    dek: Vec<String>,
    rows: Vec<Row>,
    more: usize,
}

fn layout(ui: &Ui, st: &CardStyle, e: &Event, o: &Overlay, w: f32) -> CardLayout {
    let inner = w - 2.0 * st.px(28.0);
    let lead = &e.articles[0];
    let place = match e.location.country.as_str() {
        c if c.is_empty() || !e.location.precise || c.eq_ignore_ascii_case(&e.location.name) => e.location.name.clone(),
        c => format!("{}, {c}", e.location.name),
    };
    let when = match ago(o.now, lead.published) {
        n if n == "just now" => n,
        n => format!("{n} ago"),
    };
    let meta = if o.show_topics && !lead.topic.is_empty() { format!("{}, {}, {when}", topic_name(&lead.topic), lead.source) } else { format!("{}, {when}", lead.source) };
    let head = ui.fonts.wrap(st.head.weight, st.head.size, &lead.title, inner, HEAD_LINES);
    let dek = if lead.summary.is_empty() { Vec::new() } else { ui.fonts.wrap(st.dek.weight, st.dek.size, &lead.summary, inner, 2) };
    let age_w = st.px(44.0);
    let title_w = inner - st.px(40.0) - st.px(10.0) - st.px(10.0) - age_w;
    let rows: Vec<Row> = e
        .articles
        .iter()
        .skip(1)
        .take(CARD_ARTICLES)
        .map(|a| Row {
            code: a.source.clone(),
            lines: ui.fonts.wrap(st.title.weight, st.title.size, &a.title, title_w, 2),
            age: ago(o.now, a.published),
            url: a.url.clone(),
        })
        .collect();
    let more = e.articles.len().saturating_sub(1 + CARD_ARTICLES);

    let mut h = st.px(26.0) + st.px(22.0) + st.px(8.0) + st.px(HEAD_LINE) * head.len() as f32;
    if !dek.is_empty() {
        h += st.px(12.0) + st.px(24.0) * dek.len() as f32;
    }
    if !rows.is_empty() {
        h += st.px(16.0) + st.px(11.0);
        h += rows.iter().map(|r| st.px(21.0) * r.lines.len() as f32 + st.px(10.0)).sum::<f32>();
        if more > 0 {
            h += st.px(26.0);
        }
    }
    h += st.px(20.0);
    CardLayout { w, h, place, meta, head, head_url: lead.url.clone(), dek, rows, more }
}

/// Cheapest spot for a `w` x `h` card near `a`: little land underneath, close to the marker, not
/// hiding other markers, clear of `keep_out`. Distances are in design px.
#[allow(clippy::too_many_arguments)]
fn place(land: &LandGrid, a: (f32, f32), w: f32, h: f32, s: f32, screen: (f32, f32), avoid: &[(f32, f32)], keep_out: &[Rect], land_weight: f32) -> (Rect, f32) {
    let (step, margin, gap) = (16.0 * s, 32.0 * s, 64.0);
    let mut best = (Rect { x: margin, y: margin, w, h }, f32::MAX);
    let mut y = margin;
    while y + h <= screen.1 - margin {
        let mut x = margin;
        while x + w <= screen.0 - margin {
            let r = Rect { x, y, w, h };
            let d = dist_to_rect(a.0, a.1, &r) / s;
            let mut c = land.fraction(x, y, w, h) * land_weight + d + d * d / 1200.0;
            if d < gap {
                c += (gap - d) * 40.0;
            }
            c += 180.0 * avoid.iter().filter(|p| dist_to_rect(p.0, p.1, &r) < 14.0 * s).count() as f32;
            if keep_out.iter().any(|k| k.intersects(&r)) {
                c += 1e5;
            }
            if c < best.1 {
                best = (r, c);
            }
            x += step;
        }
        y += step;
    }
    best
}

pub fn draw(ui: &mut Ui, gl: &glow::Context, view: &View, land: &LandGrid, o: &Overlay) -> Hits {
    let (w, h) = (view.width as f32, view.height as f32);
    let s = (h / 1080.0).min(w / 1920.0).max(0.5);
    let mut hits = Hits::default();
    let margin = 64.0 * s;

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
        let r = marker_radius(e.score, s);
        let age_h = (o.now - e.latest).max(0) as f32 / 3600.0;
        let fresh = (1.0 - age_h / 72.0).clamp(0.35, 1.0);
        let focused = o.card.is_some_and(|(c, a)| c == i && a > 0.01);
        if !focused {
            ui.disc(x, y, r, rgba(MARKER, 0.85 * fresh));
        }
        hits.markers.push((x, y, r.max(6.0 * s), i));
    }

    // ---- focus dot and ring
    if let Some((i, alpha)) = o.card {
        if let Some(&(x, y, _, _)) = hits.markers.iter().find(|m| m.3 == i) {
            let r = marker_radius(o.events[i].score, s) + s;
            let t = o.ring.clamp(0.0, 1.0);
            let ease = 1.0 - (1.0 - t).powi(3);
            ui.disc(x, y, r, rgba(FOCUS, 0.85 + 0.15 * alpha));
            // A ripple that expands and fades, then a calm steady ring 9 px out.
            ui.ring(x, y, r + (9.0 + 22.0 * ease) * s, 1.2 * s, rgba(FOCUS, 0.45 * (1.0 - ease) * alpha));
            ui.ring(x, y, r + 9.0 * s, 1.5 * s, rgba(FOCUS, 0.9 * alpha * ease.max(0.3)));
        }
    }

    if o.minimal {
        ui.flush(gl, w, h);
        return hits;
    }

    // ---- clock (top right) and credits (bottom right): laid out first so the callout avoids them
    let mut keep_out: Vec<Rect> = Vec::new();
    let clock = o.show_clock.then(|| {
        let local = OffsetDateTime::from_unix_timestamp(o.now).unwrap_or(OffsetDateTime::UNIX_EPOCH).to_offset(o.utc_offset);
        let time_text = format!("{:02}:{:02}", local.hour(), local.minute());
        let date_text = format!("{} {} {}", local.weekday(), local.day(), local.month());
        let big = TextStyle { weight: Weight::Serif, size: (72.0 * s).round(), color: rgba(INK, 0.95), tracking: 0.0 };
        let small = TextStyle { weight: Weight::Regular, size: (17.0 * s).round(), color: rgba(INK_MUTED, 0.9), tracking: 0.0 };
        let tw = ui.fonts.measure(big.weight, big.size, 0.0, &time_text);
        let dw = ui.fonts.measure(small.weight, small.size, 0.0, &date_text);
        let base = 52.0 * s + big.size * 0.72;
        keep_out.push(Rect { x: w - margin - tw.max(dw) - 16.0 * s, y: 0.0, w: tw.max(dw) + margin + 16.0 * s, h: base + 40.0 * s });
        (time_text, date_text, big, small, tw, dw, base)
    });
    let sun = o.sun.map(|info| {
        let style = TextStyle { weight: Weight::Regular, size: (17.0 * s).round(), color: rgba(INK_MUTED, 0.9), tracking: 0.0 };
        let max_width = (w - 2.0 * margin).min(380.0 * s);
        let location = ui.fonts.ellipsize(style.weight, style.size, &info.location, max_width);
        let times = ui.fonts.ellipsize(style.weight, style.size, &info.times, max_width);
        let lw = ui.fonts.measure(style.weight, style.size, 0.0, &location);
        let sw = ui.fonts.measure(style.weight, style.size, 0.0, &times);
        let base = clock.as_ref().map(|c| c.6 + 62.0 * s).unwrap_or(64.0 * s);
        keep_out.push(Rect { x: w - margin - lw.max(sw) - 16.0 * s, y: base - 22.0 * s,
            w: lw.max(sw) + margin + 16.0 * s, h: 60.0 * s });
        (location, times, style, lw, sw, base)
    });
    let credit = TextStyle { weight: Weight::Regular, size: (13.0 * s).round(), color: rgba(CREDIT, 1.0), tracking: 0.0 };
    let credit_text = format!("{}   Imagery: NASA Blue Marble, Black Marble   Coastlines: Natural Earth", o.credits);
    let cw = ui.fonts.measure(credit.weight, credit.size, 0.0, &credit_text);
    keep_out.push(Rect { x: w - margin - cw - 8.0 * s, y: h - 40.0 * s - credit.size - 8.0 * s, w: cw + margin + 8.0 * s, h: 40.0 * s + credit.size + 8.0 * s });

    // ---- callout layout and placement
    let st = CardStyle::new(s);
    let callout = o.card.filter(|(_, a)| *a > 0.01).map(|(i, alpha)| {
        let e = &o.events[i];
        let (ax, ay, _, _) = *hits.markers.iter().find(|m| m.3 == i).unwrap();
        let avoid: Vec<(f32, f32)> = hits.markers.iter().filter(|m| m.3 != i).map(|m| (m.0, m.1)).collect();
        let mut best: Option<(CardLayout, Rect, f32)> = None;
        for cw in CARD_WIDTHS {
            let lay = layout(ui, &st, e, o, (cw * s).min(w * 0.45));
            let (r, c) = place(land, (ax, ay), lay.w, lay.h, s, (w, h), &avoid, &keep_out, 4000.0);
            if best.as_ref().is_none_or(|b| c < b.2) {
                best = Some((lay, r, c));
            }
        }
        let (lay, mut rect, _) = best.unwrap();
        if dist_to_rect(ax, ay, &rect) / s > MAX_LEADER {
            // No open water nearby (crowded Europe): sit beside the place, over land.
            rect = place(land, (ax, ay), lay.w, lay.h, s, (w, h), &avoid, &keep_out, 300.0).0;
        }
        (lay, rect, (ax, ay), alpha)
    });

    // ---- labels for the top events (sentence case, on whichever side is free)
    let label = TextStyle { weight: Weight::Regular, size: (15.0 * s).round(), color: rgba(INK_MUTED, 0.9), tracking: 0.0 };
    let mut taken: Vec<Rect> = hits.markers.iter().map(|&(x, y, r, _)| Rect { x: x - r, y: y - r, w: 2.0 * r, h: 2.0 * r }).collect();
    taken.extend(keep_out.iter().copied());
    if let Some((_, r, _, _)) = &callout {
        taken.push(*r);
    }
    for &(x, y, _, i) in hits.markers.iter().take(LABELED) {
        if o.card.is_some_and(|(c, a)| c == i && a > 0.05) {
            continue; // the callout already names it
        }
        let text = &o.events[i].location.name;
        let tw = ui.fonts.measure(label.weight, label.size, 0.0, text);
        let th = label.size;
        let off = 12.0 * s + marker_radius(o.events[i].score, s);
        for lx in [x + off, x - off - tw] {
            let r = Rect { x: lx - 2.0, y: y - th * 0.7, w: tw + 4.0, h: th * 1.3 };
            if r.x > 8.0 && r.x + r.w < w - 8.0 && !taken.iter().any(|t| t.intersects(&r)) {
                ui.text(gl, label, lx, y + th * 0.35, text);
                taken.push(r);
                break;
            }
        }
    }

    // ---- callout: leader, card, text
    if let Some((lay, card, (ax, ay), alpha)) = callout {
        let attach = card.y + st.px(26.0) + st.px(11.0);
        let (tx, ty) = if ax < card.x {
            (card.x, attach)
        } else if ax > card.x + card.w {
            (card.x + card.w, attach)
        } else {
            (ax, if ay < card.y { card.y } else { card.y + card.h })
        };
        let len = (tx - ax).hypot(ty - ay).max(1.0);
        let inset = 16.0 * s;
        ui.line(ax + (tx - ax) / len * inset, ay + (ty - ay) / len * inset, tx, ty, 1.25 * s, rgba(FOCUS, 0.75 * alpha));

        ui.rect(Rect { x: card.x - 1.0, y: card.y - 1.0, w: card.w + 2.0, h: card.h + 2.0 }, 11.0 * s, [1.0, 1.0, 1.0, 0.10 * alpha]);
        ui.rect(card, 10.0 * s, [CARD[0], CARD[1], CARD[2], CARD[3] * alpha]);
        hits.card = Some(card);

        let fade = |t: TextStyle| TextStyle { color: [t.color[0], t.color[1], t.color[2], t.color[3] * alpha], ..t };
        let (pad, inner) = (st.px(28.0), card.w - 2.0 * st.px(28.0));
        let x = card.x + pad;
        // Text sits on a baseline at roughly the middle of its line box plus a third of the size.
        let base = |top: f32, line: f32, size: f32| top + line / 2.0 + size * 0.35;

        let mut y = card.y + st.px(26.0);
        let pw = ui.text(gl, fade(st.place), x, base(y, st.px(22.0), st.place.size), &lay.place);
        ui.text(gl, fade(st.meta), x + pw + st.px(12.0), base(y, st.px(22.0), st.meta.size), &lay.meta);
        y += st.px(22.0) + st.px(8.0);
        let head_top = y;
        for line in &lay.head {
            ui.text(gl, fade(st.head), x, base(y, st.px(HEAD_LINE), st.head.size), line);
            y += st.px(HEAD_LINE);
        }
        hits.links.push((Rect { x: card.x, y: head_top, w: card.w, h: y - head_top }, lay.head_url.clone()));
        if !lay.dek.is_empty() {
            y += st.px(12.0);
            for line in &lay.dek {
                ui.text(gl, fade(st.dek), x, base(y, st.px(24.0), st.dek.size), line);
                y += st.px(24.0);
            }
        }
        if !lay.rows.is_empty() {
            y += st.px(16.0);
            ui.rect(Rect { x, y, w: inner, h: 1.0 }, 0.0, [1.0, 1.0, 1.0, 0.10 * alpha]);
            y += st.px(11.0);
            let title_x = x + st.px(40.0) + st.px(10.0);
            for (k, row) in lay.rows.iter().enumerate() {
                let k = k + 1; // link 0 is the headline
                let row_h = st.px(21.0) * row.lines.len() as f32 + st.px(10.0);
                let hovered = o.hovered_link == Some(k);
                if hovered {
                    ui.rect(Rect { x: x - st.px(8.0), y, w: inner + st.px(16.0), h: row_h }, 2.0 * s, [1.0, 1.0, 1.0, 0.08 * alpha]);
                }
                let mut ly = y + st.px(5.0);
                ui.text(gl, fade(st.code), x, base(ly, st.px(21.0), st.code.size), &row.code);
                let aw = ui.fonts.measure(st.age.weight, st.age.size, 0.0, &row.age);
                ui.text(gl, fade(st.age), x + inner - aw, base(ly, st.px(21.0), st.age.size), &row.age);
                let title = if hovered { TextStyle { color: rgba(INK, 1.0), ..st.title } } else { st.title };
                for line in &row.lines {
                    let bl = base(ly, st.px(21.0), st.title.size);
                    let lw = ui.text(gl, fade(title), title_x, bl, line);
                    if hovered {
                        ui.rect(Rect { x: title_x, y: bl + 3.0 * s, w: lw, h: s.max(1.0) }, 0.0, rgba(INK, 0.8 * alpha));
                    }
                    ly += st.px(21.0);
                }
                hits.links.push((Rect { x: card.x, y, w: card.w, h: row_h }, row.url.clone()));
                y += row_h;
            }
            if lay.more > 0 {
                ui.text(gl, fade(st.age), title_x, base(y, st.px(26.0), st.age.size), &format!("+{} more", lay.more));
            }
        }
    }

    // ---- clock and credits
    if let Some((time_text, date_text, big, small, tw, dw, base)) = clock {
        ui.text(gl, big, w - margin - tw, base, &time_text);
        ui.text(gl, small, w - margin - dw, base + 6.0 * s + 22.0 * s, &date_text);
    }
    if let Some((location, times, style, lw, sw, base)) = sun {
        ui.text(gl, style, w - margin - lw, base, &location);
        ui.text(gl, style, w - margin - sw, base + 24.0 * s, &times);
    }
    ui.text(gl, credit, w - margin - cw, h - 40.0 * s, &credit_text);

    ui.flush(gl, w, h);
    hits
}
