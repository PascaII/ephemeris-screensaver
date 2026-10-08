<img src="ephemeris.png" alt="ephemeris — a dictionary definition" width="320">

# ephemeris

a small windows screensaver written in rust. a quiet world map follows the sun, lights up cities after dark, and places recent news where it happens.

## why i built it

my father wanted an old windows screensaver he used to have, with a world map and news on it. i couldn't find it anymore, so i built him this one.

## what it does

- renders a flat world map with nasa blue marble imagery by day, soft twilight, and city lights after dark.
- shows today’s sunrise and sunset for a configurable city, calculated offline.
- locates news from nzz, bbc, and the new york times using an embedded gazetteer.
- groups reports about the same event across publishers and languages, showing the last 72 hours by default.
- shows newspaper-style headline cards, placed over nearby open water where possible and connected to their markers.
- cycles through featured events while idle; hover to explore a marker, or click to open an article. a key press or a click on the empty map exits.

## under the hood

the interesting part is turning a handful of news feeds into a readable map without making the screensaver heavy.

```text
rss feeds -> local cache -> geolocation -> deduplication -> ranked events
                                                               |
utc time -> solar position -> day/night map --------------------+-> screen
```

rust handles the news pipeline on one background thread. place names and aliases resolve locally; weighted text similarity and shared names or places help merge related reports. opengl 3.3 draws the map in one shader pass, with a separate batch for text and markers.

sunrise and sunset are calculated offline once per city-local date using the [NOAA/Meeus equations](https://gml.noaa.gov/grad/solcalc/calcdetails.html), with timezone and daylight-saving rules bundled into the app. Zürich is the default; change `city`, `latitude`, `longitude`, and `timezone` in the config's `[sun]` section to use another location, or set `enabled = false` to hide the times.

## performance

ephemeris stays small by doing less work. when nothing changes, the renderer can sleep for up to 10 seconds; fades and transitions run at roughly 30 fps. coastline and night-light textures use one byte per pixel; the daytime imagery uses rgb. night lights are downsampled to suit the display, and rendered glyphs are cached. embedded imagery, place data, and subsetted fonts total about 2.6 mb. release builds optimize for size, use link-time optimization, and strip symbols. everything ships in one `.scr`, with no runtime map downloads; news refreshes hourly by default using conditional requests.

the targets are a binary around 6 mb, idle cpu below 1%, and ram below 60 mb. measured on windows with `/s` fullscreen, sampled for 60 s after a 12 s warm-up. size and cpu are within target; ram is at the limit, almost all of it the gpu driver:

| windows benchmark | measured |
| --- | --- |
| release `.scr` size | 3.9 mb (4,086,272 bytes) |
| idle cpu | 0.03% of the machine (0.65% of one core) |
| ram during idle | 61 mb private working set (task manager), 90 mb including shared driver code; the app itself needs about 10 mb, the rest is the nvidia opengl driver |
| test hardware / resolution | amd ryzen 9 9900x, nvidia geforce rtx 5070 ti, windows 11 pro (build 26300) / 1920×1200 |

## try it

on windows, download `ephemeris.scr` from a successful [build artifact](../../actions/workflows/build.yml), extract it, right-click it and choose install, then select ephemeris in screen saver settings. the settings button opens the config file, where you can choose topics, publishers, map centre, and timing.

for development, rust and an opengl 3.3 capable system are required. the windowed mode also runs on macos.

```sh
cargo run --release -- --window                 # run in a window
cargo run --release -- --screenshot out.png     # capture a frame
cargo run --release -- --dump-news              # inspect clustered events
cargo test                                    # run unit tests
```

## credits

coastline and place data: [natural earth](https://www.naturalearthdata.com/), public domain. imagery: nasa earth observatory, blue marble next generation (july 2004) and black marble 2016, public domain. typography: adobe source sans 3 and source serif 4, under the sil open font license.

news comes from nzz, bbc, and the new york times rss feeds, with attribution and links to the originals. this project is for personal, non-commercial use; cached news expires after 72 hours by default.
