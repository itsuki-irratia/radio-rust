# radio-fm

`radio-fm` is a Rust audio scheduler for local audio files, XSPF playlists, and remote streams.
It can run one-shot scheduled playback, recurring cron-style radio programming, and a foreground
service with CLI controls for play, stop, mute, volume, and skip.

## Features

- Schedule local audio files, XSPF playlists, and HTTP/HTTPS streams.
- Store schedules and cron rules in SQLite under `$HOME/.config/radio-rust`.
- Store app configuration, named streams, fade defaults, and playback defaults in `radio-rust.json`.
- Fade out the currently playing item before scheduled replacement and fade in the new item.
- Fade the running service volume in or out from its current volume.
- Start late scheduled local audio from the calculated playback position.
- Use Linux-style cron expressions for recurring programming.
- Play a configurable Greenwich time signal at minute 00 of each hour.
- Capture the radio output device and publish it to an Icecast server.
- Control a running service through a Unix socket.

## Requirements

- Rust toolchain with Cargo.
- GStreamer runtime and plugins.
- GTK 4 development/runtime libraries if you use the GUI.

Debian/Ubuntu:

```bash
sudo apt update
sudo apt install --yes \
  build-essential git curl pkg-config clang cmake meson ninja-build \
  libgtk-4-dev libadwaita-1-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  gstreamer1.0-tools gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav \
  gstreamer1.0-pipewire \
  pipewire pipewire-pulse libasound2-dev \
  sqlite3 libsqlite3-dev libssl-dev jq
```

Manjaro/Arch:

```bash
sudo pacman -S --needed \
  base-devel git rustup pkgconf clang cmake meson ninja \
  gtk4 libadwaita \
  gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad \
  gst-plugins-ugly gst-libav gst-plugin-pipewire gst-plugin-gtk4 \
  pipewire pipewire-pulse alsa-lib \
  sqlite openssl curl jq
```

## Build

```bash
cargo build
```

For an optimized binary:

```bash
cargo build --release
```

Run commands during development with:

```bash
cargo run -- <COMMAND>
```

Or use the built binary directly:

```bash
./target/debug/radio-fm <COMMAND>
./target/release/radio-fm <COMMAND>
```

## Schedule Playback

Add a one-shot item:

```bash
cargo run -- schedule add "/path/to/song.mp3" --at "2026-05-10 22:30"
```

Schedule a remote stream:

```bash
cargo run -- schedule add "https://server12.mediasector.es/listen/bizkaia_irratia/bizkaiairratia.mp3" --at "19:00"
```

Schedule an XSPF playlist:

```bash
cargo run -- schedule add "/path/to/playlist.xspf" --at "19:14"
```

List scheduled items:

```bash
cargo run -- schedule list
```

## Streams

Add named streams to `radio-rust.json`:

```bash
cargo run -- streams add itsuki-irratia "Itsuki Irratia" "https://irratia.itsuki.freemyip.com/itsuki.opus"
```

List named streams:

```bash
cargo run -- streams list
```

## Greenwich Time Signal

Set the audio used for the hourly time signal:

```bash
cargo run -- time-signal set-audio "/path/to/pips.mp3"
```

Enable or disable the hourly signal:

```bash
cargo run -- time-signal enable
cargo run -- time-signal disable
```

Disable or re-enable the signal while a remote stream is playing:

```bash
cargo run -- time-signal disable-during-streams
cargo run -- time-signal enable-during-streams
cargo run -- time-signal streams true
cargo run -- time-signal streams false
```

Check the current setting:

```bash
cargo run -- time-signal status
```

## Icecast

Configure an Icecast server in `radio-rust.json`:

```bash
cargo run -- icecast configure \
  --server http://example.org:8000 \
  --mount /radio.ogg \
  --username source \
  --password hackme \
  --name "Radio FM" \
  --genre "Radio"
```

Check the stored settings or test basic TCP connectivity:

```bash
cargo run -- icecast status
cargo run -- icecast test
```

Pick the monitor source for the output device used by the radio, then start
publishing that output to the configured mount:

```bash
cargo run -- icecast devices
cargo run -- icecast set-device alsa_output.name.monitor
cargo run -- icecast start
```

When `radio-fm service run` starts and Icecast is enabled with a device set, the
service starts this device-to-Icecast pipeline automatically.

## Cron Playback

Cron expressions use five Linux crontab-style fields:

```text
minute hour day-of-month month day-of-week
```

Add the Bizkaia stream every Monday and Tuesday at 13:00:

```bash
cargo run -- cron add "https://server12.mediasector.es/listen/bizkaia_irratia/bizkaiairratia.mp3" --expr "0 13 * * mon-tue"
```

Add an XSPF playlist every day at 19:27:

```bash
cargo run -- cron add "/home/zital/the-old-ways.xspf" --expr "27 19 * * *"
```

List cron rules:

```bash
cargo run -- cron list
```

Remove a cron rule:

```bash
cargo run -- cron remove 1
```

## Service Mode

Start the scheduler service:

```bash
cargo run -- service run
```

Control it from another terminal:

```bash
cargo run -- service status
cargo run -- service play
cargo run -- service stop
cargo run -- service set-volume 0.50
cargo run -- service fade-out 5
cargo run -- service fade-in 5
cargo run -- service mute
cargo run -- service unmute
cargo run -- service skip
cargo run -- service shutdown
```

By default, the app uses `$HOME/.config/radio-rust/schedule.sqlite` for schedules and
`$HOME/.config/radio-rust/radio-rust.json` for configuration. The default control socket is
`/tmp/radio-fm.sock`. These can be changed with `--db`, `--config`, and `--socket`.

## GUI

```bash
cargo run -- gui
```

## More CLI Details

See `CLI.md` for the fuller command reference, accepted datetime formats, and a systemd user
service example.

## License

This project is licensed under the GNU General Public License version 3.0 only. See `LICENSE`.
