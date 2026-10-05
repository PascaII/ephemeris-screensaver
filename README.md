# Ephemeris

A minimal Windows screensaver: a dark, flat world map with the live day/night terminator, city lights on the
night side, and the most important news of the last few days pinned where they happen.
Stories from NZZ, BBC and The New York Times are merged when they cover the same event.

Built in Rust with a single OpenGL shader. It is one small `.scr` file, idles at ~1 fps and fetches feeds once an hour.

## Install (Windows)
1. Download `ephemeris.scr` from the latest CI run (Actions → build → artifacts).
2. Right-click → **Install**, or copy it to `C:\Windows\System32`.
3. Choose *Ephemeris* in *Screen Saver Settings*.

## Develop
```sh
cargo run --release -- --window
cargo test
```

## Credits
- Map data: [Natural Earth](https://www.naturalearthdata.com/) (public domain)
- City lights: NASA Earth Observatory, Black Marble 2016 (public domain)
- Font: Inter (SIL Open Font License)
- News: NZZ, BBC News, The New York Times RSS feeds. Headlines and links only, for personal use.
