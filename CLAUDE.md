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
src/main.rs          arg parsing (/s /c /p <hwnd>, --window) + event loop
src/app.rs           frame pacing, input handling (hybrid screensaver input), state
src/config.rs        TOML config (%APPDATA%\Ephemeris\config.toml), defaults
src/astronomy.rs     subsolar point from UTC (pure, unit-tested)
src/renderer/        GL setup, map shader, text atlas, markers/cards
src/news/            Source abstraction, RSS parser, feed definitions, refresh thread
src/geolocation.rs   article -> coordinates via embedded gazetteer (+ NYT geo tags)
src/dedup.rs         cluster articles from several sources into one Event, score events
src/cache.rs         local JSON cache with 72 h expiry + HTTP validators
assets/              generated, committed: land SDF, night lights, gazetteer
tools/assetgen/      offline preprocessing (downloads Natural Earth + NASA Black Marble)
```

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
cargo run --release -- --window      # dev window (macOS / Windows)
cargo test                           # unit tests (astronomy, rss, geolocation, dedup)
cargo run -p assetgen --release      # regenerate assets/ (downloads source data into tools/assetgen/cache)
```
Windows `.scr`: CI builds `target/release/ephemeris.exe` on `windows-latest` and uploads it as `ephemeris.scr`.

## Conventions
- **Commits:** commit regularly, use [Conventional Commits](https://www.conventionalcommits.org/)
  (`feat(renderer): …`, `fix(news): …`, `chore: …`, `docs: …`, `test: …`, `perf: …`, `refactor: …`).
  **Never add a `Co-Authored-By` trailer** for an AI assistant.
- Keep modules as listed above. New news sources implement the source abstraction in `src/news/`.
- Platform-specific code goes behind `#[cfg(windows)]` so the app still runs on macOS for development.
