# radio-fm CLI

## Build and run

```bash
cd /home/projects/radio-fm
cargo build
```

Run commands with:

```bash
cargo run -- <COMMAND>
```

Or use the built binary directly:

```bash
./target/debug/radio-fm <COMMAND>
```

## Commands

### Global help

```bash
cargo run -- --help
```

### Scan audio files

Recursively scans a folder for supported audio files:
`mp3`, `aac`, `flac`, `ogg`, `opus`, `wav`, `m4a`, `xspf`.

```bash
cargo run -- scan /path/to/music
```

JSON output:

```bash
cargo run -- scan /path/to/music --json
```

### Scheduler

The default schedule database is:

```text
radio-fm-schedule.sqlite
```

You can override it with `--db /path/to/schedule.sqlite`.
If the default database does not exist yet and a legacy `radio-fm-schedule.json`
file is present beside it, the entries are imported once into SQLite.

#### Add scheduled song

```bash
cargo run -- schedule add "/path/to/song.mp3" --at "2026-05-10 22:30:00"
```

With custom fade/volume/mute:

```bash
cargo run -- schedule add "/path/to/song.mp3" \
  --at "2026-05-10T22:30:00+02:00" \
  --fade-in 5 \
  --fade-out 7 \
  --volume 0.4 \
  --mute
```

XSPF playlists can be scheduled too. Local relative entries inside the playlist are resolved
relative to the playlist file:

```bash
cargo run -- schedule add "/path/to/playlist.xspf" --at "2026-05-10 22:45:00"
```

#### List schedule

```bash
cargo run -- schedule list
```

Show one local calendar day:

```bash
cargo run -- schedule list --day today
cargo run -- schedule list --day 2026-05-10
```

Show a local date range:

```bash
cargo run -- schedule list --from 2026-05-10 --to 2026-05-12
```

JSON output:

```bash
cargo run -- schedule list --json
cargo run -- schedule list --day today --json
cargo run -- schedule list --from 2026-05-10 --to 2026-05-12 --json
```

#### Run scheduler

Starts a loop that:
1. waits for the next schedule time
2. plays the track with its fade/volume/mute settings
3. removes it from the schedule database

```bash
cargo run -- schedule run
```

### Cron schedules

Cron schedules use the same five fields as a Linux crontab:

```text
minute hour day-of-month month day-of-week
```

Supported field forms include `*`, comma lists, ranges, and steps such as
`*/15`, `1,2,5`, `mon-fri`, and `9-17/2`. Month and weekday names are accepted.

Add the Bizkaia stream every Monday and Tuesday at 13:00:

```bash
cargo run -- cron add "https://server12.mediasector.es/listen/bizkaia_irratia/bizkaiairratia.mp3" \
  --expr "0 13 * * mon-tue"
```

List cron schedules:

```bash
cargo run -- cron list
```

Remove a cron schedule:

```bash
cargo run -- cron remove 1
```

Cron items are stored in the same SQLite database. The service materializes the
next matching cron occurrence into the normal one-shot schedule queue, so
scheduled replacement/fade behavior stays the same.

### Streams

Named streams are stored in the same SQLite database as schedules and cron rules.

List streams:

```bash
cargo run -- streams list
```

JSON output:

```bash
cargo run -- streams list --json
```

### Greenwich time signal

The service can play a configured audio source at second 00 of each minute. The
source is stored in the same SQLite database as schedules, cron rules, and
streams.

Set the signal audio:

```bash
cargo run -- time-signal set-audio "/path/to/pips.mp3"
```

Enable or disable the minute signal:

```bash
cargo run -- time-signal enable
cargo run -- time-signal disable
```

Show the current setting:

```bash
cargo run -- time-signal status
cargo run -- time-signal status --json
```

The aliases `greenwich` and `greenwitch` also work for the top-level command.

### Service mode (recommended for background use)

`service run` keeps a daemon process running and lets you control it from other terminals.

Default socket path:

```text
/tmp/radio-fm.sock
```

Start service in foreground:

```bash
cargo run -- service run
```

Start service with custom schedule database and socket:

```bash
cargo run -- service run --db /path/to/radio-fm-schedule.sqlite --socket /tmp/radio-fm-custom.sock
```

Service status:

```bash
cargo run -- service status
```

Enable scheduled audio playback in the running service:

```bash
cargo run -- service play
```

The service only plays scheduled items. If playback was stopped and a scheduled item is due,
`service play` starts it from the schedule.

Set live output volume (applies to current and next tracks while service is running):

```bash
cargo run -- service set-volume 0.50
```

Mute and unmute live output:

```bash
cargo run -- service mute
cargo run -- service unmute
```

Skip current track:

```bash
cargo run -- service skip
```

Stop audio playback while keeping the service running:

```bash
cargo run -- service stop
```

Stopped audio stays stopped until `service play`; scheduled items remain in the schedule.

Shut down the service process:

```bash
cargo run -- service shutdown
```

Use a custom socket with control commands:

```bash
cargo run -- service status --socket /tmp/radio-fm-custom.sock
```

When service mode is active, you can still use normal schedule commands from another terminal:

```bash
cargo run -- schedule add "/path/to/song.mp3" --at "2026-05-10 23:00:00"
cargo run -- schedule list
```

The running service reloads the schedule database continuously, so new entries are picked up automatically.

#### Run as a background systemd user service

Build release binary first:

```bash
cargo build --release
```

Create `~/.config/systemd/user/radio-fm.service`:

```ini
[Unit]
Description=radio-fm scheduler service
After=default.target

[Service]
Type=simple
WorkingDirectory=/home/zital/projects/radio-fm
ExecStart=/home/zital/projects/radio-fm/target/release/radio-fm service run --db /home/zital/projects/radio-fm/radio-fm-schedule.sqlite --socket /tmp/radio-fm.sock
Restart=always
RestartSec=2

[Install]
WantedBy=default.target
```

Enable and start:

```bash
systemctl --user daemon-reload
systemctl --user enable --now radio-fm.service
```

Check logs:

```bash
journalctl --user -u radio-fm.service -f
```

Stop:

```bash
systemctl --user stop radio-fm.service
```

### GUI

Opens the GTK window:

```bash
cargo run -- gui
```

## Datetime formats for `--at`

Accepted formats:

- RFC3339:
  - `2026-05-10T22:30:00+02:00`
- Local datetime:
  - `2026-05-10 22:30:00`
  - `2026-05-10 22:30`
  - `2026-05-10T22:30:00`
  - `2026-05-10T22:30`

For local datetimes, your machine timezone is used.

## Tips

- Paths with spaces should be quoted:
  - `"/home/zital/The Old Ways (Nights from the Alhambra Live) [3rztcvAlfFw].mp3"`
- Show help for any subcommand:
  - `cargo run -- schedule --help`
  - `cargo run -- schedule add --help`
  - `cargo run -- service --help`
