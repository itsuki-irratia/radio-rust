use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::thread;
use std::time::Duration;

use crate::config::{load_app_config, update_app_config};
use crate::types::{McpToken, McpTokenScope};

const MCP_PATH: &str = "/mcp";
const MCP_PROTOCOL_VERSION: &str = "2025-03-26";
const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;
const MAX_HEADER_LINE_BYTES: usize = 8 * 1024;
const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_CONCURRENT_CONNECTIONS: usize = 16;
const MCP_SOCKET_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_COMMAND_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_JSON_RPC_BATCH_REQUESTS: usize = 32;
const MAX_COMMAND_ARGUMENTS: usize = 64;
const MAX_COMMAND_ARGUMENT_BYTES: usize = 8 * 1024;

pub fn run_mcp_configure(config_path: &Path, port: u16, enabled: bool) -> Result<()> {
    validate_port(port)?;
    update_app_config(config_path, |config| {
        config.mcp.port = port;
        config.mcp.enabled = enabled;
        Ok(())
    })?;
    println!("MCP configured: enabled={enabled} port={port}");
    Ok(())
}

pub fn run_mcp_enable(config_path: &Path) -> Result<()> {
    let config = update_app_config(config_path, |config| {
        config.mcp.enabled = true;
        Ok(())
    })?;
    println!("MCP enabled on http://127.0.0.1:{}/mcp", config.mcp.port);
    Ok(())
}

pub fn run_mcp_disable(config_path: &Path) -> Result<()> {
    update_app_config(config_path, |config| {
        config.mcp.enabled = false;
        Ok(())
    })?;
    println!("MCP disabled");
    Ok(())
}

pub fn run_mcp_status(config_path: &Path, json_output: bool) -> Result<()> {
    let config = load_app_config(config_path)?.mcp;
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "enabled": config.enabled,
                "port": config.port,
                "token_count": config.tokens.len(),
            }))?
        );
    } else {
        let state = if config.enabled {
            "enabled"
        } else {
            "disabled"
        };
        println!(
            "MCP {state}: http://127.0.0.1:{}/mcp tokens={}",
            config.port,
            config.tokens.len()
        );
    }
    Ok(())
}

pub fn run_mcp_server(config_path: &Path, port_override: Option<u16>) -> Result<()> {
    let config = load_app_config(config_path)?.mcp;
    if !config.enabled {
        bail!("MCP is disabled. Run `radio-fm mcp enable` before starting the server");
    }
    let port = port_override.unwrap_or(config.port);
    validate_port(port)?;

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .with_context(|| format!("Failed to bind MCP server to 127.0.0.1:{port}"))?;
    println!("MCP server running at http://127.0.0.1:{port}{MCP_PATH}");
    println!("Press Ctrl-C to stop the MCP server.");

    let active_connections = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let active_connections = Arc::clone(&active_connections);
                if active_connections
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                        (count < MAX_CONCURRENT_CONNECTIONS).then_some(count + 1)
                    })
                    .is_err()
                {
                    let _ = write_http_response(&mut stream, 503, "Service Unavailable", None);
                    continue;
                }

                let config_path = config_path.to_path_buf();
                thread::spawn(move || {
                    if let Err(error) = handle_connection(&mut stream, &config_path) {
                        eprintln!("MCP request failed: {error:#}");
                        let _ = write_http_response(
                            &mut stream,
                            500,
                            "Internal Server Error",
                            Some(&json_rpc_error(
                                Value::Null,
                                -32603,
                                "Internal server error",
                            )),
                        );
                    }
                    active_connections.fetch_sub(1, Ordering::Release);
                });
            }
            Err(error) => eprintln!("MCP connection failed: {error}"),
        }
    }
    Ok(())
}

pub fn run_mcp_token_create(config_path: &Path, name: &str, scope: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        bail!("MCP token name must not be empty");
    }
    let scope = McpTokenScope::parse(scope)
        .context("Invalid MCP token scope. Use read, control, or admin")?;

    let token = generate_token()?;
    let token_hash = hash_token(&token);
    let id = token_hash[..16].to_string();
    let token_entry = McpToken {
        id: id.clone(),
        name: name.to_string(),
        token_hash,
        created_at: chrono::Utc::now().to_rfc3339(),
        scope,
    };
    update_app_config(config_path, |config| {
        if config.mcp.tokens.iter().any(|existing| existing.id == id) {
            bail!("Failed to create a unique MCP token. Please try again");
        }
        config.mcp.tokens.push(token_entry);
        Ok(())
    })?;

    println!(
        "MCP token created: id={id} name={name} scope={}",
        scope.as_str()
    );
    println!("Store this token securely; it will not be shown again:");
    println!("{token}");
    Ok(())
}

pub fn run_mcp_token_list(config_path: &Path, json_output: bool) -> Result<()> {
    let tokens = load_app_config(config_path)?.mcp.tokens;
    if json_output {
        let tokens = tokens
            .iter()
            .map(|token| {
                json!({
                    "id": token.id,
                    "name": token.name,
                    "created_at": token.created_at,
                    "scope": token.scope,
                })
            })
            .collect::<Vec<_>>();
        println!("{}", serde_json::to_string_pretty(&tokens)?);
        return Ok(());
    }
    if tokens.is_empty() {
        println!("No MCP tokens configured");
        return Ok(());
    }
    for token in tokens {
        println!(
            "id={} name={} scope={} created_at={}",
            token.id,
            token.name,
            token.scope.as_str(),
            token.created_at
        );
    }
    Ok(())
}

pub fn run_mcp_token_revoke(config_path: &Path, id: &str) -> Result<()> {
    let id = id.trim();
    if id.is_empty() {
        bail!("MCP token id must not be empty");
    }
    update_app_config(config_path, |config| {
        let initial_count = config.mcp.tokens.len();
        config.mcp.tokens.retain(|token| token.id != id);
        if config.mcp.tokens.len() == initial_count {
            bail!("MCP token not found: {id}");
        }
        Ok(())
    })?;
    println!("MCP token revoked: {id}");
    Ok(())
}

fn validate_port(port: u16) -> Result<()> {
    if port == 0 {
        bail!("Invalid MCP port 0. Use a port between 1 and 65535");
    }
    Ok(())
}

fn handle_connection(stream: &mut TcpStream, config_path: &Path) -> Result<()> {
    stream
        .set_read_timeout(Some(MCP_SOCKET_TIMEOUT))
        .context("Failed to set MCP socket read timeout")?;
    stream
        .set_write_timeout(Some(MCP_SOCKET_TIMEOUT))
        .context("Failed to set MCP socket write timeout")?;
    let mut reader = BufReader::new(stream.try_clone().context("Failed to read MCP request")?);
    let request_line =
        read_limited_http_line(&mut reader, MAX_REQUEST_LINE_BYTES, "MCP request line")?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    if method.is_empty() || path.is_empty() {
        bail!("Malformed HTTP request");
    }

    let mut content_length = 0usize;
    let mut origin = None;
    let mut authorization = None;
    let mut header_bytes = 0usize;
    loop {
        let header =
            read_limited_http_line(&mut reader, MAX_HEADER_LINE_BYTES, "MCP request header")?;
        header_bytes += header.len();
        if header_bytes > MAX_HEADER_BYTES {
            bail!("MCP request headers are too large");
        }
        if header == "\r\n" || header == "\n" {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value
                    .trim()
                    .parse()
                    .context("Invalid MCP Content-Length header")?;
            } else if name.eq_ignore_ascii_case("origin") {
                origin = Some(value.trim().to_string());
            } else if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value.trim().to_string());
            }
        }
    }

    if !is_allowed_origin(origin.as_deref()) {
        return write_http_response(stream, 403, "Forbidden", None);
    }
    if method == "OPTIONS" {
        return write_http_response(stream, 204, "No Content", None);
    }
    if path != MCP_PATH {
        return write_http_response(stream, 404, "Not Found", None);
    }
    if method == "GET" || method == "DELETE" {
        return write_http_response(stream, 405, "Method Not Allowed", None);
    }
    if method != "POST" {
        return write_http_response(stream, 405, "Method Not Allowed", None);
    }
    let Some(scope) = authorize(config_path, authorization.as_deref())? else {
        return write_http_response(stream, 401, "Unauthorized", None);
    };
    if content_length > MAX_REQUEST_BODY_BYTES {
        return write_http_response(stream, 413, "Payload Too Large", None);
    }

    let mut body = vec![0; content_length];
    reader
        .read_exact(&mut body)
        .context("Failed reading MCP request body")?;
    let request: Value = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(_) => {
            return write_http_response(
                stream,
                400,
                "Bad Request",
                Some(&json_rpc_error(Value::Null, -32700, "Parse error")),
            );
        }
    };

    let response = handle_json_rpc_message(&request, scope, config_path)?;
    match response {
        Some(response) => write_http_response(stream, 200, "OK", Some(&response)),
        None => write_http_response(stream, 202, "Accepted", None),
    }
}

fn read_limited_http_line(
    reader: &mut BufReader<TcpStream>,
    max_bytes: usize,
    label: &str,
) -> Result<String> {
    let mut bytes = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .with_context(|| format!("Failed reading {label}"))?;
        if available.is_empty() {
            break;
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        if bytes.len() + take > max_bytes {
            bail!("{label} exceeds {max_bytes} bytes");
        }
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);
        if bytes.last() == Some(&b'\n') {
            break;
        }
    }
    String::from_utf8(bytes).with_context(|| format!("{label} is not valid UTF-8"))
}

fn generate_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    File::open("/dev/urandom")
        .context("Failed to open the system random source")?
        .read_exact(&mut bytes)
        .context("Failed to generate an MCP token")?;
    Ok(format!("rfm_{}", hex_encode(&bytes)))
}

fn hash_token(token: &str) -> String {
    hex_encode(&Sha256::digest(token.as_bytes()))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn authorize(config_path: &Path, authorization: Option<&str>) -> Result<Option<McpTokenScope>> {
    let Some(token) = bearer_token(authorization) else {
        return Ok(None);
    };
    let token_hash = hash_token(token);
    let config = load_app_config(config_path)?.mcp;
    if !config.enabled {
        return Ok(None);
    }
    Ok(config.tokens.iter().find_map(|stored| {
        constant_time_equals(stored.token_hash.as_bytes(), token_hash.as_bytes())
            .then_some(stored.scope)
    }))
}

fn bearer_token(authorization: Option<&str>) -> Option<&str> {
    let (scheme, token) = authorization?.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

fn constant_time_equals(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| {
            difference | (*left ^ *right)
        })
        == 0
}

fn is_allowed_origin(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let origin = origin.trim_end_matches('/');
    let authority = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"));
    let Some(authority) = authority else {
        return false;
    };
    if matches!(authority, "localhost" | "127.0.0.1" | "[::1]") {
        return true;
    }
    let Some((host, port)) = authority.rsplit_once(':') else {
        return false;
    };
    matches!(host, "localhost" | "127.0.0.1" | "[::1]") && port.parse::<u16>().is_ok()
}

fn handle_json_rpc_message(
    request: &Value,
    scope: McpTokenScope,
    config_path: &Path,
) -> Result<Option<Value>> {
    if let Some(requests) = request.as_array() {
        if requests.is_empty() {
            return Ok(Some(json_rpc_error(Value::Null, -32600, "Invalid Request")));
        }
        if requests.len() > MAX_JSON_RPC_BATCH_REQUESTS {
            return Ok(Some(json_rpc_error(
                Value::Null,
                -32600,
                "Too many requests in JSON-RPC batch",
            )));
        }
        let responses = requests
            .iter()
            .map(|request| handle_json_rpc_request(request, scope, config_path))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        return Ok((!responses.is_empty()).then_some(Value::Array(responses)));
    }
    handle_json_rpc_request(request, scope, config_path)
}

fn handle_json_rpc_request(
    request: &Value,
    scope: McpTokenScope,
    config_path: &Path,
) -> Result<Option<Value>> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str);
    let Some(method) = method else {
        return Ok(Some(json_rpc_error(
            id.unwrap_or(Value::Null),
            -32600,
            "Invalid Request",
        )));
    };

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "radio-fm", "version": env!("CARGO_PKG_VERSION") }
        })),
        "notifications/initialized" => return Ok(None),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": mcp_tools(scope) })),
        "tools/call" => call_tool(
            request.get("params").unwrap_or(&Value::Null),
            scope,
            config_path,
        ),
        _ => Err(anyhow::anyhow!("Method not found")),
    };

    let Some(id) = id else {
        return Ok(None);
    };
    Ok(Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) if error.to_string() == "Method not found" => {
            json_rpc_error(id, -32601, "Method not found")
        }
        Err(error) => json_rpc_error(id, -32602, &error.to_string()),
    }))
}

fn mcp_tools(scope: McpTokenScope) -> Vec<Value> {
    vec![json!({
        "name": "radio_fm_command",
        "description": format!("Run a radio-fm CLI command with a {}-scoped token. Pass its arguments exactly as they would follow `radio-fm`, for example [\"schedule\", \"list\"] or [\"service\", \"set-volume\", \"0.5\"]. Long-running server and playback commands are not available through this request/response tool.", scope.as_str()),
        "inputSchema": {
            "type": "object",
            "properties": {
                "arguments": {
                    "type": "array",
                    "description": "CLI arguments following radio-fm.",
                    "items": { "type": "string" },
                    "minItems": 1
                }
            },
            "required": ["arguments"],
            "additionalProperties": false
        }
    })]
}

fn call_tool(params: &Value, scope: McpTokenScope, config_path: &Path) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if name != "radio_fm_command" {
        bail!("Unknown tool: {name}");
    }
    let arguments = params
        .pointer("/arguments/arguments")
        .and_then(Value::as_array)
        .context("Tool argument `arguments` must be an array of strings")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .context("Tool argument `arguments` must contain only strings")
        })
        .collect::<Result<Vec<_>>>()?;
    if arguments.is_empty() {
        bail!("Tool argument `arguments` must not be empty");
    }
    if arguments.len() > MAX_COMMAND_ARGUMENTS {
        bail!("Tool argument `arguments` exceeds {MAX_COMMAND_ARGUMENTS} entries");
    }
    let argument_bytes = arguments.iter().map(String::len).sum::<usize>();
    if argument_bytes > MAX_COMMAND_ARGUMENT_BYTES {
        bail!("Tool argument `arguments` exceeds {MAX_COMMAND_ARGUMENT_BYTES} bytes");
    }
    if is_long_running_command(&arguments) {
        bail!("Long-running commands must be started directly from a terminal");
    }
    if !scope_allows_command(scope, &arguments) {
        bail!(
            "MCP token scope {} does not allow this command",
            scope.as_str()
        );
    }
    let arguments = add_server_config_argument(arguments, config_path)?;

    let executable = std::env::current_exe().context("Failed to locate radio-fm executable")?;
    let mut child = Command::new(executable)
        .args(&arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to run radio-fm command")?;
    let stdout = child
        .stdout
        .take()
        .context("Failed to capture radio-fm command output")?;
    let stderr = child
        .stderr
        .take()
        .context("Failed to capture radio-fm command errors")?;
    let stdout_reader = thread::spawn(move || read_capped_command_output(stdout));
    let stderr_reader = thread::spawn(move || read_capped_command_output(stderr));
    let status = child
        .wait()
        .context("Failed waiting for radio-fm command")?;
    let (stdout, stdout_truncated) = stdout_reader
        .join()
        .map_err(|_| anyhow::anyhow!("Failed to read radio-fm command output"))?
        .context("Failed to capture radio-fm command output")?;
    let (stderr, stderr_truncated) = stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("Failed to read radio-fm command errors"))?
        .context("Failed to capture radio-fm command errors")?;
    let mut text = format!(
        "{}{}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    if stdout_truncated || stderr_truncated {
        text.push_str("\n[Command output truncated to protect the MCP server]\n");
    }
    Ok(json!({
        "content": [{ "type": "text", "text": if text.is_empty() { "Command completed without output" } else { &text } }],
        "isError": !status.success()
    }))
}

fn add_server_config_argument(
    mut arguments: Vec<String>,
    config_path: &Path,
) -> Result<Vec<String>> {
    if arguments
        .iter()
        .any(|argument| argument == "--config" || argument.starts_with("--config="))
    {
        bail!("MCP commands must use the server configuration; do not pass --config");
    }
    let command = arguments.first().map(String::as_str);
    let subcommand = arguments.get(1).map(String::as_str);
    let accepts_config = matches!(
        (command, subcommand),
        (Some("schedule"), Some("add"))
            | (Some("cron"), Some("add"))
            | (Some("streams"), _)
            | (Some("time-signal"), _)
            | (
                Some("icecast"),
                Some(
                    "configure"
                        | "enable"
                        | "disable"
                        | "status"
                        | "test"
                        | "set-device"
                        | "start"
                        | "stream"
                )
            )
            | (Some("mcp"), _)
            | (Some("service"), Some("run"))
    );
    if accepts_config {
        arguments.push("--config".to_string());
        arguments.push(
            config_path
                .to_str()
                .context("MCP server config path is not valid UTF-8")?
                .to_string(),
        );
    }
    Ok(arguments)
}

fn read_capped_command_output(mut reader: impl Read) -> std::io::Result<(Vec<u8>, bool)> {
    let mut captured = Vec::new();
    let mut truncated = false;
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = MAX_COMMAND_OUTPUT_BYTES.saturating_sub(captured.len());
        let copied = remaining.min(read);
        captured.extend_from_slice(&buffer[..copied]);
        truncated |= copied < read;
    }
    Ok((captured, truncated))
}

fn scope_allows_command(scope: McpTokenScope, arguments: &[String]) -> bool {
    let command = arguments.first().map(String::as_str);
    let subcommand = arguments.get(1).map(String::as_str);
    match scope {
        McpTokenScope::Admin => true,
        McpTokenScope::Control => matches!(
            command,
            Some("schedule")
                | Some("cron")
                | Some("streams")
                | Some("time-signal")
                | Some("icecast")
                | Some("service")
        ),
        McpTokenScope::Read => matches!(
            (command, subcommand),
            (Some("schedule"), Some("list"))
                | (Some("cron"), Some("list"))
                | (Some("streams"), Some("list"))
                | (Some("time-signal"), Some("status"))
                | (Some("icecast"), Some("status"))
                | (Some("service"), Some("status"))
                | (Some("mcp"), Some("status"))
        ),
    }
}

fn is_long_running_command(arguments: &[String]) -> bool {
    matches!(
        (
            arguments.first().map(String::as_str),
            arguments.get(1).map(String::as_str)
        ),
        (Some("service"), Some("run"))
            | (Some("icecast"), Some("start"))
            | (Some("icecast"), Some("stream"))
            | (Some("mcp"), Some("run"))
            | (Some("schedule"), Some("run"))
    )
}

fn json_rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn write_http_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    body: Option<&Value>,
) -> Result<()> {
    let payload = body.map(serde_json::to_vec).transpose()?;
    let mut response =
        format!("HTTP/1.1 {status} {reason}\r\nConnection: close\r\nAllow: POST, GET\r\n");
    if status == 401 {
        response.push_str("WWW-Authenticate: Bearer\r\n");
    }
    if let Some(payload) = &payload {
        response.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            payload.len()
        ));
        stream.write_all(response.as_bytes())?;
        stream.write_all(payload)?;
    } else {
        response.push_str("Content-Length: 0\r\n\r\n");
        stream.write_all(response.as_bytes())?;
    }
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::save_app_config;
    use crate::types::AppConfig;
    use std::net::Shutdown;

    #[test]
    fn exposes_the_cli_tool() {
        let tools = mcp_tools(McpTokenScope::Read);
        assert_eq!(tools[0]["name"], "radio_fm_command");
    }

    #[test]
    fn rejects_server_commands_from_tool_calls() {
        assert!(is_long_running_command(&["service".into(), "run".into()]));
        assert!(is_long_running_command(&["icecast".into(), "start".into()]));
        assert!(is_long_running_command(&[
            "icecast".into(),
            "stream".into()
        ]));
        assert!(is_long_running_command(&["mcp".into(), "run".into()]));
        assert!(is_long_running_command(&["schedule".into(), "run".into()]));
        assert!(!is_long_running_command(&["mcp".into(), "status".into()]));
        assert!(!is_long_running_command(&[
            "schedule".into(),
            "list".into()
        ]));
    }

    #[test]
    fn permits_only_local_origins() {
        assert!(is_allowed_origin(None));
        assert!(is_allowed_origin(Some("http://localhost:3000")));
        assert!(is_allowed_origin(Some("http://127.0.0.1")));
        assert!(is_allowed_origin(Some("http://[::1]:5173")));
        assert!(!is_allowed_origin(Some("https://example.com")));
    }

    #[test]
    fn batches_only_return_request_responses() {
        let request = json!([
            { "jsonrpc": "2.0", "method": "notifications/initialized" },
            { "jsonrpc": "2.0", "id": 1, "method": "ping" }
        ]);
        let response =
            handle_json_rpc_message(&request, McpTokenScope::Read, Path::new("config.json"))
                .expect("batch parses")
                .expect("request response exists");

        assert_eq!(response.as_array().expect("response is an array").len(), 1);
    }

    #[test]
    fn rejects_oversized_json_rpc_batches() {
        let request = Value::Array(
            std::iter::repeat_with(|| json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }))
                .take(MAX_JSON_RPC_BATCH_REQUESTS + 1)
                .collect(),
        );

        let response =
            handle_json_rpc_message(&request, McpTokenScope::Read, Path::new("config.json"))
                .expect("batch parses")
                .expect("batch response exists");

        assert_eq!(response["error"]["code"], -32600);
    }

    #[test]
    fn initialization_negotiates_the_supported_protocol_version() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "older-version" }
        });
        let response =
            handle_json_rpc_message(&request, McpTokenScope::Read, Path::new("config.json"))
                .expect("initialization parses")
                .expect("initialization responds");

        assert_eq!(response["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
    }

    #[test]
    fn recognizes_bearer_tokens_and_compares_hashes() {
        assert_eq!(bearer_token(Some("Bearer token")), Some("token"));
        assert_eq!(bearer_token(Some("Basic token")), None);
        assert!(constant_time_equals(b"same", b"same"));
        assert!(!constant_time_equals(b"same", b"different"));
        assert_eq!(hash_token("token").len(), 64);
    }

    #[test]
    fn token_scopes_limit_cli_commands() {
        assert!(scope_allows_command(
            McpTokenScope::Read,
            &["schedule".into(), "list".into()]
        ));
        assert!(!scope_allows_command(
            McpTokenScope::Read,
            &["schedule".into(), "add".into()]
        ));
        assert!(scope_allows_command(
            McpTokenScope::Control,
            &["service".into(), "play".into()]
        ));
        assert!(!scope_allows_command(
            McpTokenScope::Control,
            &["mcp".into(), "token".into(), "create".into()]
        ));
        assert!(!scope_allows_command(
            McpTokenScope::Control,
            &["scan".into(), "/media".into()]
        ));
        assert!(scope_allows_command(
            McpTokenScope::Admin,
            &["mcp".into(), "token".into(), "create".into()]
        ));
    }

    #[test]
    fn command_output_is_capped_while_remaining_drained() {
        let output = vec![b'x'; MAX_COMMAND_OUTPUT_BYTES + 1];
        let (captured, truncated) =
            read_capped_command_output(std::io::Cursor::new(output)).expect("read output");

        assert_eq!(captured.len(), MAX_COMMAND_OUTPUT_BYTES);
        assert!(truncated);
    }

    #[test]
    fn config_aware_mcp_commands_use_the_server_config() {
        let arguments = add_server_config_argument(
            vec!["streams".into(), "list".into()],
            Path::new("/tmp/server.json"),
        )
        .expect("arguments are accepted");
        assert_eq!(
            arguments,
            ["streams", "list", "--config", "/tmp/server.json"]
        );
        assert!(
            add_server_config_argument(
                vec![
                    "streams".into(),
                    "list".into(),
                    "--config".into(),
                    "other.json".into()
                ],
                Path::new("/tmp/server.json"),
            )
            .is_err()
        );
    }

    #[test]
    fn http_mcp_requires_a_valid_bearer_token() {
        let directory = std::env::temp_dir().join(format!(
            "radio-fm-mcp-auth-test-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&directory).expect("create test directory");
        let config_path = directory.join("radio-rust.json");
        let mut config = AppConfig::default();
        config.mcp.enabled = true;
        config.mcp.tokens.push(McpToken {
            id: "test-token".to_string(),
            name: "test".to_string(),
            token_hash: hash_token("valid-token"),
            created_at: chrono::Utc::now().to_rfc3339(),
            scope: McpTokenScope::Read,
        });
        save_app_config(&config_path, &config).expect("save test config");

        let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        let authorized = send_http_request(
            &config_path,
            &format!(
                "POST /mcp HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer valid-token\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(authorized.starts_with("HTTP/1.1 200 OK"));

        let unauthorized = send_http_request(
            &config_path,
            &format!(
                "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(unauthorized.starts_with("HTTP/1.1 401 Unauthorized"));

        std::fs::remove_dir_all(&directory).expect("remove test directory");
    }

    fn send_http_request(config_path: &Path, request: &str) -> String {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind test listener");
        let address = listener.local_addr().expect("read listener address");
        let config_path = config_path.to_path_buf();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test connection");
            handle_connection(&mut stream, &config_path).expect("handle test request");
        });

        let mut stream = TcpStream::connect(address).expect("connect test listener");
        stream
            .write_all(request.as_bytes())
            .expect("send test request");
        stream
            .shutdown(Shutdown::Write)
            .expect("finish test request");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("read test response");
        worker.join().expect("join test server");
        response
    }
}
