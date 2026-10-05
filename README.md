# Ephemeris

A minimal Windows screensaver: a dark, flat world map with the live day/night terminator, city lights on the
night side, and the most important news of the last few days pinned where they happen.
Stories from NZZ, BBC and The New York Times are merged when they cover the same event.

Built in Rust with a single OpenGL shader. It is one small `.scr` file, idles at ~1 fps and fetches feeds once an hour.

## Install (Windows)
1. Download `ephemeris.scr` from the latest CI run (Actions → build → artifacts).
2. Right-click → **Install**, or copy it to `C:\Windows\System32`.
3. Choose *Ephemeris* in *Screen Saver Settings*.

## Using it
- Moving the mouse reveals the cursor. Hovering a marker shows that event's card; clicking a marker
  or a headline in the card opens the article.
- A key press or a click on the empty map ends the screensaver.
- Without interaction, the most important events rotate through the card every few seconds.
- Settings live in `%APPDATA%\Ephemeris\config.toml` (*Screen Saver Settings → Settings…* opens it).
  They cover map centre, refresh interval, max news age, number of events, clock and skipped NZZ kickers.
- **Topics:** `topics = ["world", "sport"]` picks any of `top`, `world`, `politics`, `business`, `sport`,
  `science`, `tech`, `culture`. `publishers = ["NZZ", "BBC", "NYT"]` picks sources; the first one's headline
  leads a card. More RSS feeds can be added under `[[extra_feeds]]` (see the comment at the top of the file).
- News refreshes at most once an hour, using conditional requests. Articles older than 72 h are dropped.

## How it works
- **Map:** one fragment shader with a Miller projection, a land signed-distance field (crisp coastlines at any
  resolution), the live solar terminator with twilight, and NASA night lights faded in where it is dark.
- **News:** RSS from NZZ, BBC and NYT. Articles are geolocated offline with an embedded gazetteer
  (Natural Earth, German and English names, demonyms, regions). Reports of the same event are merged across
  sources and languages (IDF-weighted similarity, a shared rare name/place requirement, average-linkage
  clustering), then ranked by number of sources, recency and feed position.

## Develop
```sh
cargo run --release -- --window       # windowed (Esc to quit)
cargo run --release -- --dump-news    # inspect geolocated, clustered events
cargo test
```
See `CLAUDE.md` for the architecture and conventions.

## Credits
- Map data: [Natural Earth](https://www.naturalearthdata.com/) (public domain)
- City lights: NASA Earth Observatory, Black Marble 2016 (public domain)
- Font: Inter (SIL Open Font License)
- News: NZZ, BBC News, The New York Times RSS feeds. Headlines and links only, for personal use.
