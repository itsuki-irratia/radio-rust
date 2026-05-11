# radio-fm

`radio-fm` is a Rust audio scheduler for local audio files, XSPF playlists, and remote streams.
It can run one-shot scheduled playback, recurring cron-style radio programming, and a foreground
service with CLI controls for play, stop, mute, volume, and skip.

## Features

- Schedule local audio files, XSPF playlists, and HTTP/HTTPS streams.
- Store schedules, cron rules, and named streams in `radio-fm-schedule.sqlite`.
- Fade out the currently playing item before scheduled replacement and fade in the new item.
- Start late scheduled local audio from the calculated playback position.
- Use Linux-style cron expressions for recurring programming.
- Play a configurable Greenwich time signal at second 00 of each minute.
- Control a running service through a Unix socket.

## Requirements

- Rust toolchain with Cargo.
- GStreamer runtime and plugins.
- GTK 4 development/runtime libraries if you use the GUI.

On Manjaro/Arch-style systems, install the packages listed in `requirements-manjaro.txt`.

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

List named streams stored in the SQLite database:

```bash
cargo run -- streams list
```

## Greenwich Time Signal

Set the audio used for the minute time signal:

```bash
cargo run -- time-signal set-audio "/path/to/pips.mp3"
```

Enable or disable the minute signal:

```bash
cargo run -- time-signal enable
cargo run -- time-signal disable
```

Check the current setting:

```bash
cargo run -- time-signal status
```

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
cargo run -- service mute
cargo run -- service unmute
cargo run -- service skip
cargo run -- service shutdown
```

The default database is `radio-fm-schedule.sqlite`, and the default control socket is
`/tmp/radio-fm.sock`. Both can be changed with `--db` and `--socket`.

## GUI

```bash
cargo run -- gui
```

## More CLI Details

See `CLI.md` for the fuller command reference, accepted datetime formats, and a systemd user
service example.

## License

This project is licensed under the GNU General Public License version 3.0 only. See `LICENSE`.
