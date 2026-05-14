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
$HOME/.config/radio-rust/schedule.sqlite
```

You can override it with `--db /path/to/schedule.sqlite`.
App configuration is stored separately in:

```text
$HOME/.config/radio-rust/radio-rust.json
```

You can override it for config-aware commands with `--config /path/to/radio-rust.json`.
The default scheduled fade duration is stored separately from playback volume
settings:

```json
{
  "fade": {
    "duration": 5
  },
  "playback": {
    "default_volume": 1.0,
    "default_mute": false
  }
}
```
If a schedule database does not exist yet and a legacy JSON schedule file is present
beside it, the entries are imported once into SQLite.

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

Cron schedules are stored in the same SQLite database. The service materializes the
next matching cron occurrence into the normal one-shot schedule queue, so
scheduled replacement/fade behavior stays the same.

### Streams

Named streams are stored in `radio-rust.json`, not in the schedule database.

Add or update a stream:

```bash
cargo run -- streams add itsuki-irratia "Itsuki Irratia" "https://irratia.itsuki.freemyip.com/itsuki.opus"
```

List streams:

```bash
cargo run -- streams list
```

JSON output:

```bash
cargo run -- streams list --json
```

### Greenwich time signal

The service can play a configured audio source at minute 00 of each hour. The
source and stream playback behavior are stored in `radio-rust.json`.

Set the signal audio:

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
```

Set the same behavior directly. When `streams` is `true`, the time signal plays
over remote streams; when `false`, it is skipped while a remote stream is playing.

```bash
cargo run -- time-signal streams true
cargo run -- time-signal streams false
```

Show the current setting:

```bash
cargo run -- time-signal status
cargo run -- time-signal status --json
```

The aliases `greenwich` and `greenwitch` also work for the top-level command.

### Service controls

Start the foreground scheduler service:

```bash
cargo run -- service run
```

Control the running service through its Unix socket:

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

`fade-out` ramps the active playback from its current audible volume to silence.
`fade-in` ramps back from the current audible volume to the previous non-zero
live volume, or to the current scheduled/default volume when there is no previous
live fade target. Both commands default to 5 seconds when no duration is passed.

### Icecast

Icecast connection settings are stored in `radio-rust.json` under the `icecast`
key. The intended production path is to capture the monitor source of the output
device used by the radio and publish that audio to Icecast.

Configure Icecast:

```bash
cargo run -- icecast configure \
  --server http://example.org:8000 \
  --mount /radio.ogg \
  --username source \
  --password hackme \
  --device alsa_output.name.monitor \
  --name "Radio FM" \
  --description "Scheduled radio output" \
  --genre "Radio" \
  --public
```

Show status:

```bash
cargo run -- icecast status
cargo run -- icecast status --json
```

Enable, disable, or test connectivity:

```bash
cargo run -- icecast enable
cargo run -- icecast disable
cargo run -- icecast test
```

List available monitor sources and set the device to capture:

```bash
cargo run -- icecast devices
cargo run -- icecast set-device alsa_output.name.monitor
```

Start publishing the selected output device to the configured Icecast mount. The
command runs until stopped.

```bash
cargo run -- icecast start
```

When `service run` starts and Icecast is enabled with a device set, the service
starts the same device capture automatically. The `icecast stream <source>`
command is also available for testing a single file or remote URL directly.

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
cargo run -- service run --db /path/to/schedule.sqlite --config /path/to/radio-rust.json --socket /tmp/radio-fm-custom.sock
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
ExecStart=/home/zital/projects/radio-fm/target/release/radio-fm service run --socket /tmp/radio-fm.sock
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
