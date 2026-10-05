# Ephemeris — project guide for agents

Minimal, lightweight **Windows screensaver** (`.scr`) written in Rust. It shows a flat 16:9 world map with:
- the real-time solar terminator (day/night + twilight)
- NASA night-lights that appear on the night side
- geolocated, **deduplicated** news events from NZZ, BBC and NYT RSS feeds (last 72 h)

Guiding principle: **look sophisticated, stay extremely small, fast and efficient.**

## Resource budget (do not regress)
- Single self-contained `.scr`, all assets embedded via `include_bytes!`; target ≤ ~6 MB.
- Idle: < 1 % CPU, < 60 MB RAM. Redraw at ~1 fps when idle, ~30 fps only during transitions. Never busy-loop.
- Network: each feed at most once per hour, with conditional requests (ETag / If-Modified-Since). No scraping of article pages.
- No runtime downloads of datasets. Preprocess offline (`tools/assetgen`) and commit the results to `assets/`.
- Prefer few, small dependencies. Don't add `image`, `tokio`, `reqwest` or `wgpu` without a strong reason.

## Stack
`winit` 0.30 + `glutin` + `glow` (OpenGL 3.3 core, one fullscreen-quad shader), `fontdue` (text),
`ureq` (blocking HTTP on one background thread), `quick-xml` (RSS), `time`, `serde`/`serde_json`/`toml`, `png`,
`windows-sys` (Win32 only).

## Layout
```
src/main.rs              arg parsing (/s, /p <hwnd>, /c, --window, --screenshot, --at, --dump-news)
src/app.rs               frame pacing, spotlight cycling, card fades, hybrid input, secondary monitors
src/config.rs            TOML config (%APPDATA%\Ephemeris\config.toml), topic/publisher feed catalog, cache dir
src/astronomy.rs         subsolar point from UTC (pure, unit-tested)
src/renderer/mod.rs      GL context/window creation, Renderer, Blank (secondary monitors)
src/renderer/map.rs      map pass + View (Miller projection, lat/lon -> pixels)
src/renderer/shaders/    map.frag (land SDF, day/night, twilight, lights), ui.vert/ui.frag (2D batch)
src/renderer/ui.rs       immediate-mode 2D batch: rects, discs, rings, glows, text
src/renderer/text.rs     fontdue glyph atlas, measuring, wrapping, ellipsis
src/renderer/overlay.rs  markers, labels, event card, clock, attribution; returns hit regions
src/news/mod.rs          Article, Source trait, refresh (conditional GET + cache), events pipeline, thread
src/news/rss.rs          RSS 2.0 parser (NYT geo/entity tags, NZZ kickers)
src/geolocation.rs       article -> Location via gazetteer + aliases (weighted mentions)
src/dedup.rs             IDF cosine + anchor rule, agglomerative average-linkage, event scoring
src/cache.rs             local JSON cache with 72 h expiry + HTTP validators
assets/                  land_sdf.png, lights.png, gazetteer.tsv (generated); aliases.tsv (hand-curated); Inter subsets
tools/assetgen/          offline preprocessing (downloads Natural Earth + NASA Black Marble + Inter)
```

## Tuning geolocation / dedup
- `cargo run --release -- --dump-news` prints the clustered events with their articles.
- `EPHEMERIS_DEBUG_DEDUP=1` additionally logs every merge with the shared features.
- Wrong or missing places: add to `assets/aliases.tsv` (demonyms, regions, stoplist). Keep tests in
  `geolocation.rs` / `dedup.rs` passing and add a case for the fix.

## Data sources & licensing
- Natural Earth (land, populated places, countries): public domain.
- NASA Black Marble 2016 night lights: public domain (credit NASA Earth Observatory).
- Inter font: SIL OFL.
- News RSS (NZZ, BBC, NYT): **personal, non-commercial use only.** Show headline/teaser/link with attribution.
  NZZ forbids permanent storage, so the cache only keeps headline/teaser/link and expires after 72 h.

## Screensaver behaviour
- `/s` fullscreen; `/p <hwnd>` preview in the parent window; `/c` or no args opens config; `--window` = dev window.
- Hybrid input: moving the mouse reveals the cursor and hover cards. Clicking a marker opens the article and exits.
  A key press or a click on the empty map exits. When idle, featured events cycle automatically.

## Commands
```sh
cargo run --release -- --window                  # dev window (macOS / Windows)
cargo run --release -- --screenshot out.png      # render one frame to PNG (add --at <unix> for a fixed time)
cargo run --release -- --dump-news               # print clustered news events
cargo test                                       # unit tests (astronomy, rss, geolocation, dedup, args)
cargo check --target x86_64-pc-windows-msvc      # type-check Windows-only code from macOS
cargo run -p assetgen --release                  # regenerate assets/ (needs curl; uvx for font subsetting)
```
Windows `.scr`: CI builds `target/release/ephemeris.exe` on `windows-latest` and uploads it as `ephemeris.scr`.

## Conventions
- **Commits:** commit regularly, use [Conventional Commits](https://www.conventionalcommits.org/)
  (`feat(renderer): …`, `fix(news): …`, `chore: …`, `docs: …`, `test: …`, `perf: …`, `refactor: …`).
  **Never add a `Co-Authored-By` trailer** for an AI assistant.
- Keep modules as listed above. New news sources implement the source abstraction in `src/news/`.
- Platform-specific code goes behind `#[cfg(windows)]` so the app still runs on macOS for development.
