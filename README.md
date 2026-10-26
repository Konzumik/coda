# coda

**CO**ver-**D**riven **A**udio visualizer — a terminal spectrum visualizer for
[Kitty](https://sw.kovidgoyal.net/kitty/) that paints itself in the colors of
the album art of whatever is playing.

When the track changes, coda grabs the cover over MPRIS, extracts a clean
3–5 color palette from it and smoothly blends the bars into the new colors.
The cover itself is shown in the corner via the Kitty graphics protocol.

## Features

- **Live system audio** — captures what is playing through PipeWire/PulseAudio
  (the default output's monitor), not the microphone.
- **Palette from the album art** — k-means in OKLab; muddy, dark and grey
  clusters are dropped, saturation is lifted where needed, colors are ordered
  into a smooth gradient. Black-and-white covers get greys plus one neon accent.
- **Smooth color transitions** between tracks (1.5 s, in OKLab, without the
  grey dip you get when mixing opposite hues).
- **cava-style motion** — parabolic fall plus an output integrator, peaks with
  1/8-cell precision.
- **4 display modes**, switched with `1`–`4`.
- **Pixel-perfect glyphs** — only block elements, eighth-lines and braille,
  which Kitty draws itself instead of taking them from the font.
- Redraws correctly on any window size; 90 fps with synchronized output
  (no tearing).

## Modes

| Key | Mode | |
|---|---|---|
| `1` | **Bars** | classic spectrum bars with falling peaks |
| `2` | **Mirror** | bars over an axis with a dim reflection, like neon on wet asphalt |
| `3` | **Mountains** | a smooth filled silhouette with a lit ridge and a dark far range made of peaks |
| `4` | **Ring** | the cover in the center with the spectrum radiating around it, slowly spinning |

Other keys: `q` / `Esc` / `Ctrl+C` — quit.

## Requirements

- Linux with PipeWire (`pipewire-pulseaudio`) or PulseAudio, and `parec`
  (Fedora: `pulseaudio-utils`, Debian/Ubuntu: `pulseaudio-utils`).
- [Kitty](https://sw.kovidgoyal.net/kitty/) — other terminals will show the
  spectrum but not the cover.
- An MPRIS2 player. Defaults to [Strawberry](https://www.strawberrymusicplayer.org/);
  any other works via `CODA_PLAYER` (see below).
- Rust 1.85+ to build.

Tested on Fedora 44, Hyprland, Kitty 0.47.1, PipeWire 1.6.8.

## Install

```sh
git clone https://github.com/Konzumik/coda
cd coda
cargo install --path .
```

This puts the `coda` binary into `~/.cargo/bin`. Then just run:

```sh
coda
```

### Other players

`CODA_PLAYER` is the part of the MPRIS bus name after
`org.mpris.MediaPlayer2.`:

```sh
CODA_PLAYER=spotify coda
CODA_PLAYER=mpv coda      # needs mpv-mpris
```

`busctl --user list | grep mpris` shows what is running.

## How it works

- **Audio:** `parec` streams float32 from `@DEFAULT_MONITOR@` into a ring
  buffer. PipeWire delivers audio in uneven bursts, so a playhead advances at
  wall-clock speed and the FFT window moves by the same amount every frame.
- **Spectrum:** 4096-point FFT with a Hann window, log-spaced bands
  40 Hz–12.5 kHz, dB scale with automatic gain.
- **MPRIS:** raw D-Bus signals — `PropertiesChanged` (filtered by the player's
  unique name, so other players on the same object path are ignored) and
  `NameOwnerChanged` to notice the player quitting or restarting. The cover is
  read right away, since players often keep it in a temporary file.
- **Cover:** sent once per track as PNG via the Kitty graphics protocol
  (`a=t`, chunked base64), then placed with `a=p`; re-placing with the same
  image and placement id moves it without flicker.

## Development tools

`examples/` has the tools used to tune coda without a terminal:

| Example | What it does |
|---|---|
| `tune` | measures smoothness (jitter, reversals, lag) on a raw recording |
| `palette_preview` | contact sheet: covers → extracted palettes → gradients |
| `snapshot` | renders every mode to a PNG from a raw recording |
| `mpris_probe` | prints what the player sends over MPRIS |

Recordings are raw `f32le` stereo at 48 kHz, e.g.
`ffmpeg -i song.flac -f f32le -ac 2 -ar 48000 song.raw`.

Code comments are in Russian.

## License

[MIT](LICENSE)
