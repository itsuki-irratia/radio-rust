# radio-fm MCP guide

`radio-fm` exposes one MCP tool over a local Streamable HTTP endpoint. An AI
agent can use that tool to execute the supported `radio-fm` CLI commands and
read their normal text or JSON output.

This guide is the MCP-specific reference. General command syntax and local
operation are documented in [CLI.md](CLI.md).

## Server lifecycle

MCP is disabled by default. Configure it, create a least-privilege token, then
start the listener in a terminal that remains running:

```bash
radio-fm mcp configure --port 3333 --enabled true
radio-fm mcp token create --name "agent-readonly" --scope read
radio-fm mcp run
```

The server listens only on `http://127.0.0.1:3333/mcp`. The `--port` value for
`mcp run` is a one-time override and does not save a configuration change:

```bash
radio-fm mcp run --port 4444
```

Use the normal CLI to inspect or manage the server:

```bash
radio-fm mcp status --json
radio-fm mcp enable
radio-fm mcp disable
radio-fm mcp token list --json
radio-fm mcp token revoke TOKEN_ID
```

`mcp run` is intentionally unavailable through MCP. A local process manager,
terminal, or service unit must start it.

## Authentication and token scopes

Every MCP `POST` request requires this header:

```text
Authorization: Bearer rfm_<token value>
```

The full token is printed exactly once by `mcp token create`; only a SHA-256
hash is stored in the configuration file. Treat the printed value as a secret:
put it in the MCP client's secret store or environment, never in prompts,
repositories, tool arguments, logs, or issue comments.

Create the narrowest scope that an agent needs:

```bash
radio-fm mcp token create --name "observer" --scope read
radio-fm mcp token create --name "scheduler" --scope control
radio-fm mcp token create --name "operator" --scope admin
```

| Scope | MCP permissions |
| --- | --- |
| `read` | `schedule list`, `cron list`, `streams list`, `time-signal status`, `icecast status`, `service status`, and `mcp status`. |
| `control` | All non-blocking commands under `schedule`, `cron`, `streams`, `time-signal`, `icecast`, and `service`. It cannot manage MCP configuration or tokens. |
| `admin` | Every non-blocking CLI command, including `scan` and MCP configuration/token management. |

All scopes reject commands that would run indefinitely: `service run`,
`schedule run`, `icecast start`, `icecast stream`, and `mcp run`.

Tokens created before scopes were added retain `admin` access for compatibility.
Revoke and recreate them to apply least privilege. Changes to tokens take effect
on the next request; restarting the MCP server is not required.

## Client connection

Configure a Streamable HTTP-capable MCP client with:

| Setting | Value |
| --- | --- |
| URL | `http://127.0.0.1:3333/mcp` |
| HTTP method | `POST` |
| Authentication | `Authorization: Bearer <token>` |
| Content type | `application/json` |
| MCP protocol version | `2025-03-26` |

Client configuration formats vary. Conceptually, the connection should look
like this; substitute the token through the client's secret mechanism:

```json
{
  "mcpServers": {
    "radio-fm": {
      "url": "http://127.0.0.1:3333/mcp",
      "headers": {
        "Authorization": "Bearer ${RADIO_FM_MCP_TOKEN}"
      }
    }
  }
}
```

The endpoint is loopback-only. Do not expose it through a proxy or bind it to a
public interface without adding strong network access controls; an authorized
MCP client can control radio playback and configuration.

## MCP protocol

The endpoint accepts JSON-RPC 2.0 requests and supports these methods:

| Method | Purpose |
| --- | --- |
| `initialize` | Negotiates MCP protocol and reports the tool capability. |
| `notifications/initialized` | Notification sent after initialization; it receives HTTP `202 Accepted` and no JSON body. |
| `ping` | Health check. |
| `tools/list` | Lists tools available to the token's scope. |
| `tools/call` | Invokes a tool. |

An agent should initialize once, then call `tools/list` before invoking a tool.
It may send JSON-RPC batches, but should normally use one request at a time so
errors can be associated with the requested action.

### Initialize

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "protocolVersion": "2025-03-26",
    "capabilities": {},
    "clientInfo": { "name": "radio-operator", "version": "1.0" }
  }
}
```

The result advertises `tools.listChanged: false` and the `radio-fm` server
version. Send `notifications/initialized` after receiving the result.

### Discover tools

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/list"
}
```

The server currently returns one tool named `radio_fm_command`. Its description
is generated for the token scope, so an agent must not assume that the same
command is available with a different token.

## `radio_fm_command`

This tool executes CLI arguments exactly as they would appear after
`radio-fm`. It has this input schema:

```json
{
  "type": "object",
  "properties": {
    "arguments": {
      "type": "array",
      "items": { "type": "string" },
      "minItems": 1
    }
  },
  "required": ["arguments"],
  "additionalProperties": false
}
```

For example, to list the schedule as JSON:

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "tools/call",
  "params": {
    "name": "radio_fm_command",
    "arguments": {
      "arguments": ["schedule", "list", "--json"]
    }
  }
}
```

The result is MCP tool content in this form:

```json
{
  "content": [{ "type": "text", "text": "...CLI output..." }],
  "isError": false
}
```

Check `isError` before acting on the text. A failed CLI command is still a
valid MCP tool result and returns `isError: true`; use the text to diagnose the
command, then retry with corrected arguments.

### Supported radio operations

Use ordinary CLI argument order. Quoting is not needed inside the JSON array:
each argument is already a separate string.

| Task | `arguments` example | Minimum scope |
| --- | --- | --- |
| Inspect scheduled playback | `["schedule", "list", "--json"]` | `read` |
| Add a scheduled item | `["schedule", "add", "/music/news.ogg", "--at", "2026-08-20 12:00", "--fade-in", "2"]` | `control` |
| Inspect cron entries | `["cron", "list", "--json"]` | `read` |
| Add or remove a cron rule | `["cron", "add", "/music/jingle.mp3", "--expr", "0 * * * *"]` or `["cron", "remove", "12"]` | `control` |
| Inspect named streams | `["streams", "list", "--json"]` | `read` |
| Add/update a named stream | `["streams", "add", "news", "News Radio", "https://example.invalid/live.ogg"]` | `control` |
| Inspect the time signal | `["time-signal", "status", "--json"]` | `read` |
| Configure the time signal | `["time-signal", "set-audio", "/music/pips.ogg"]`, `["time-signal", "enable"]`, or `["time-signal", "streams", "false"]` | `control` |
| Inspect Icecast | `["icecast", "status", "--json"]` | `read` |
| Configure Icecast | `["icecast", "configure", "--server", "127.0.0.1:8000", "--mount", "/radio", "--password", "…"]` | `control` |
| List Icecast devices | `["icecast", "devices"]` | `control` |
| Control the service | `["service", "status"]`, `["service", "set-volume", "0.5"]`, `["service", "mute"]`, `["service", "skip"]`, or `["service", "shutdown"]` | `read` for status; `control` for the rest |
| Inspect MCP status | `["mcp", "status", "--json"]` | `read` |
| Manage MCP or scan media | `["mcp", "token", "list", "--json"]` or `["scan", "/music", "--json"]` | `admin` |

Use `service shutdown` only when the operator has explicitly requested it. An
agent should prefer `status` and `list --json` before making a state-changing
call, report the result, and avoid repeated retries for destructive operations.

### Complete command surface

The following matrix lists every CLI operation that the MCP tool can invoke.
Use the command's normal options from [CLI.md](CLI.md) as additional strings in
the `arguments` array.

| Command group | Allowed through MCP | Scope |
| --- | --- | --- |
| `scan` | `scan FOLDER [--json]` | `admin` |
| `schedule` | `add`, `list` | `control` for `add`; `read` for `list` |
| `cron` | `add`, `list`, `remove` | `control` for `add`/`remove`; `read` for `list` |
| `streams` | `add`, `list` | `control` for `add`; `read` for `list` |
| `time-signal` | `set-audio`, `enable`, `disable`, `disable-during-streams`, `enable-during-streams`, `streams`, `status` | `control` except `status`, which is `read` |
| `icecast` | `configure`, `enable`, `disable`, `status`, `test`, `devices`, `set-device` | `control` except `status`, which is `read` |
| `service` | `play`, `status`, `set-volume`, `fade-in`, `fade-out`, `mute`, `unmute`, `skip`, `stop`, `shutdown` | `control` except `status`, which is `read` |
| `mcp` | `configure`, `enable`, `disable`, `status`, `token create`, `token list`, `token revoke` | `admin` except `status`, which is `read` |

The tool rejects `schedule run`, `service run`, `icecast start`, `icecast
stream`, and `mcp run`. They are foreground/long-running commands and must be
started directly by a local operator.

### Configuration and database paths

Config-aware commands always receive the configuration file used by the MCP
server. Passing `--config` or `--config=...` from a tool call is rejected, so a
client cannot redirect configuration writes to another file.

Database options are regular CLI arguments. `schedule` and `cron` commands use
the default schedule database unless `--db PATH` is provided. Grant MCP access
only to agents trusted to use any paths that their token scope permits.

## Direct HTTP examples

For troubleshooting, store the token outside shell history where possible:

```bash
export RADIO_FM_MCP_TOKEN='rfm_...'
```

List tools:

```bash
curl --silent --show-error http://127.0.0.1:3333/mcp \
  -H "Authorization: Bearer $RADIO_FM_MCP_TOKEN" \
  -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

Call `service status`:

```bash
curl --silent --show-error http://127.0.0.1:3333/mcp \
  -H "Authorization: Bearer $RADIO_FM_MCP_TOKEN" \
  -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"radio_fm_command","arguments":{"arguments":["service","status"]}}}'
```

## Limits and error handling

The server closes each HTTP connection after one response and enforces these
limits:

| Limit | Value |
| --- | --- |
| Active connections | 16 |
| Read/write timeout | 5 seconds |
| Request body | 1 MiB |
| JSON-RPC batch size | 32 requests |
| Tool arguments | 64 strings and 8 KiB combined |
| Captured stdout and stderr | 256 KiB each; excess output is drained and marked as truncated |

Common HTTP responses are `401 Unauthorized` for a missing, invalid, revoked,
or disabled token/server; `403 Forbidden` for a non-local `Origin`; `404 Not
Found` for paths other than `/mcp`; `405 Method Not Allowed` for non-POST
requests; and `413 Payload Too Large` for oversized bodies. `OPTIONS` receives
`204 No Content`.

JSON-RPC errors use `-32700` for malformed JSON, `-32600` for invalid requests
or batches, `-32601` for an unknown method, and `-32602` for invalid tool input,
scope denial, or CLI command failure. Re-read `tools/list`, verify the token
scope, and correct the arguments rather than retrying blindly.

## Agent operating checklist

1. Confirm that the MCP endpoint is local and that the token has the minimum
   scope required.
2. Initialize the MCP session and call `tools/list`.
3. Use status/list calls to establish current state before changing it.
4. Call `radio_fm_command` with a separate string for every CLI argument.
5. Check `isError` and the returned text. Do not assume a state change succeeded
   merely because HTTP returned `200`.
6. Never send a token as a tool argument or return it in agent output.
7. Do not try to start blocking commands through MCP; launch those locally when
   an operator explicitly requests them.
