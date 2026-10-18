# Guide figures

The screenshots in the user guide (`docs/user-guide/images/`) are made by
`make.py` from the figure descriptions in `figures/`, so they can be made
again whenever the app changes.

    cargo build --release -p timegrapher-app
    python3 guide-figures/make.py              # every figure
    python3 guide-figures/make.py paper-strip  # one figure

Each figure is a TOML file:

- `output`: the image's name in `docs/user-guide/images/`.
- `layout`: the panes and where they sit, as `["row" | "column", [share, child], ...]`,
  `["tabs", "Pane", ...]` or `"Pane"`. Panes left out are hidden.
- `[data]`: the recording. `corpus` and `file` name a take in
  [timegrapher-corpus](https://github.com/peterhudson/timegrapher-corpus);
  `start` and `seconds` cut a clip of it, kept in `data/<figure>/` so the
  figure builds without a download; a clip over 5 MB stays in `.cache/` and
  is cut again from a downloaded take. `[data.session]` sets the watch,
  calibre, beat rate, lift angle and position when the take has no
  session file. With no `seconds` the whole take is
  fetched. `mode = "replay"` replays it instead of analysing it;
  `mode = "microphone"` plays it into a virtual microphone (a PulseAudio
  server of the figure's own) for the app to listen to, looping, with
  `[data.stand]` naming a file for each position (the clip is Dial Up).
  With no `[data]` the app opens on its welcome screen.
- `[window]`: `size` in points, on a 1920x1080 screen.
- `[settings]`: the app's settings, over the base ones (light, the strip across).
- `actions`: steps before the picture: `["click", x, y]` (in the window,
  below its title bar), `["key", "ctrl+o"]`, `["type", "text"]`,
  `["sleep", s]`, and with a microphone `["stand", "DD"]` to turn the
  watch or `["stand", "off"]` to take it off. A figure whose actions use
  the stand starts with the stand empty.
- `[wait]`: `at_least` and `at_most` seconds for the screen to settle, or
  `exact` seconds for a live screen, which never settles.

The script starts Xvfb with a white background and openbox with a light
title bar (`openbox-themerc`, `openbox-rc.xml`), opens the app on the
recording with the settings in a fresh home folder, waits until the screen
stops changing, and saves the window with a soft shadow.

`--clips` cuts the clips again from the corpus; `--keep` leaves the raw
screen and the app's log in `.cache/`.

Needs Linux, a built `timegrapher-app` (or `APP_BIN`), Xvfb, openbox,
xdotool, ImageMagick and pngquant; ffmpeg for `--clips`; PulseAudio
(`pulseaudio`, `paplay`) for a microphone figure. One run at a time:
the screen is display :42.
