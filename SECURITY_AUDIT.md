# Security and reliability audit

Reviewed: 2026-08-19

This document tracks the findings from the static review of the Rust sources,
the MCP HTTP server, local sockets, SQLite persistence, playback, and release
dependencies. It is a maintenance record, not a guarantee that the application
is free of defects.

## Corrected in this change

### Configuration persistence — high

`radio-rust.json` can contain the Icecast password. Historically it was written
with the process `umask` and by truncating the destination in place. This could
expose the password on multi-user systems and let concurrent readers observe an
empty or partially written config file.

**Fix:** write a `0600` temporary file, sync it, and atomically rename it over
the configuration file. A bounded exclusive lock also serializes configuration
updates. Configuration and legacy-import input sizes are capped. All new and
updated configurations receive restrictive file permissions, and existing
configurations are tightened to `0600` when loaded.

### Cron materialization — high

Cron added a row to `cron_runs` before adding the corresponding scheduled item.
If the second insert failed, the run marker made that occurrence permanently
ineligible for later retries. Also, run markers accumulated forever.

**Fix:** insert both rows in one SQLite transaction and prune markers older than
seven days. Cron removal now also deletes the cron rule, its queued occurrences,
and future markers in one transaction. Legacy JSON schedule imports are also
all-or-nothing.

### MCP HTTP resource exhaustion — high

The MCP server previously processed every connection inline, had no socket read
timeout, and placed no bound on its request line or headers. A local slow client
could block all MCP traffic indefinitely.

**Fix:** use a bounded number of connection workers, timeouts, and strict
request-line/header limits. Long-running CLI operations are refused through the
request/response tool. Captured child-process output is also capped at 256 KiB
per stream while excess data is drained. JSON-RPC batches and CLI argument
counts/lengths are bounded before command execution. Config-aware commands use
the MCP server's own configuration path and cannot override it in a tool call.

### Unix service socket resource exhaustion — medium

An accepted Unix socket stream was read without a timeout or command-size limit.
It could stall the scheduler loop. Socket permissions also depended on `umask`.

**Fix:** limit command reads, expire incomplete commands, and set the socket mode
to `0600`. Startup and shutdown refuse to delete a non-socket path, and the
service handles at most eight control connections per scheduling tick.

### Icecast transport and secrets — medium

The implementation uses GStreamer's `shout2send` with `protocol=http`; accepting
an `https://` server URL would misleadingly suggest transport encryption. The
Icecast password is also a command-line argument when initially configured.

**Fix:** reject unsupported HTTPS URLs instead of silently downgrading them.
The configuration-file fix protects the password at rest. Endpoints now reject
paths, credentials, malformed IPv6 hosts, and port zero instead of silently
discarding URL components. `pactl` device enumeration has bounded output
capture. Avoid entering the password in shared terminals or shell histories.
GStreamer `shout2send` exposes only Icecast HTTP/ICY/Xaudiocast protocols, not
TLS; publishing to an untrusted network therefore requires a TLS-terminating
proxy under the operator's control. [GStreamer shout2send documentation](https://gstreamer.freedesktop.org/documentation/shout2/index.html)

### Media scanning and playlists — medium

Recursive scanning followed directory symlinks, allowing a cycle to consume the
stack. Playlist parsing and expansion had no explicit input or track-count bound.

**Fix:** do not recurse into symbolic links. Reject XSPF playlists larger than
4 MiB or with more than 10,000 tracks. Directory scanning is iterative and has
explicit directory and media-file limits.

### Dependency advisory — medium

The project used `quick-xml 0.38.4`, while RustSec reports two denial-of-service
issues fixed in `0.41.0`. The current code uses `Reader`, not the documented
affected `NsReader` or attribute-iteration paths, but the dependency is updated
as defense in depth.

RustSec also reported `RUSTSEC-2026-0190`: `anyhow` before `1.0.103` has an
unsound `Error::downcast_mut` path. The dependency is now pinned at `1.0.104`.

References: [RUSTSEC-2026-0195](https://rustsec.org/advisories/RUSTSEC-2026-0195.html),
[RUSTSEC-2026-0194](https://rustsec.org/advisories/RUSTSEC-2026-0194.html),
[RUSTSEC-2026-0190](https://rustsec.org/advisories/RUSTSEC-2026-0190.html).

## Remaining follow-ups

- Add integration tests for MCP authentication, malformed/oversized HTTP
  requests, token revocation, scopes, and socket timeouts.
- Scoped tokens now default to `read`, with `control` and `admin` available.
  Rotate pre-existing tokens, which preserve legacy `admin` access.
- The default service socket now uses `XDG_RUNTIME_DIR`, with a config-directory
  fallback when it is unavailable.
- The GitHub Actions security workflow installs and runs `cargo audit` on pull
  requests, changes to `master`, and weekly. Consider adding `cargo deny` if
  license and source-policy checks become necessary.

## Verification

After remediation, `cargo test` passes 24 tests, including regression tests for
configuration permissions, cron transaction rollback, Icecast endpoint
validation, and MCP HTTP authentication. `cargo clippy --all-targets -- -D
warnings` and `cargo audit` also pass.
