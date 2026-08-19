use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;

use crate::config::{load_app_config, update_app_config};
use crate::types::McpToken;

const MCP_PATH: &str = "/mcp";
const MCP_PROTOCOL_VERSION: &str = "2025-03-26";
const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;

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

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                if let Err(error) = handle_connection(&mut stream, config_path) {
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
            }
            Err(error) => eprintln!("MCP connection failed: {error}"),
        }
    }
    Ok(())
}

pub fn run_mcp_token_create(config_path: &Path, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        bail!("MCP token name must not be empty");
    }

    let token = generate_token()?;
    let token_hash = hash_token(&token);
    let id = token_hash[..16].to_string();
    let token_entry = McpToken {
        id: id.clone(),
        name: name.to_string(),
        token_hash,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    update_app_config(config_path, |config| {
        if config.mcp.tokens.iter().any(|existing| existing.id == id) {
            bail!("Failed to create a unique MCP token. Please try again");
        }
        config.mcp.tokens.push(token_entry);
        Ok(())
    })?;

    println!("MCP token created: id={id} name={name}");
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
            "id={} name={} created_at={}",
            token.id, token.name, token.created_at
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
    let mut reader = BufReader::new(stream.try_clone().context("Failed to read MCP request")?);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .context("Failed reading MCP request line")?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    if method.is_empty() || path.is_empty() {
        bail!("Malformed HTTP request");
    }

    let mut content_length = 0usize;
    let mut origin = None;
    let mut authorization = None;
    loop {
        let mut header = String::new();
        reader
            .read_line(&mut header)
            .context("Failed reading MCP request header")?;
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
    if !is_authorized(config_path, authorization.as_deref())? {
        return write_http_response(stream, 401, "Unauthorized", None);
    }
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

    let response = handle_json_rpc_message(&request)?;
    match response {
        Some(response) => write_http_response(stream, 200, "OK", Some(&response)),
        None => write_http_response(stream, 202, "Accepted", None),
    }
}

fn generate_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    File::open("/dev/urandom")
        .context("Failed to open the system random source")?
        .read_exact(&mut bytes)
        .context("Failed to generate an MCP token")?;
    Ok(format!(
        "rfm_{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_authorized(config_path: &Path, authorization: Option<&str>) -> Result<bool> {
    let Some(token) = bearer_token(authorization) else {
        return Ok(false);
    };
    let token_hash = hash_token(token);
    let config = load_app_config(config_path)?.mcp;
    Ok(config.enabled
        && config.tokens.iter().any(|stored| {
            constant_time_equals(stored.token_hash.as_bytes(), token_hash.as_bytes())
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

fn handle_json_rpc_message(request: &Value) -> Result<Option<Value>> {
    if let Some(requests) = request.as_array() {
        if requests.is_empty() {
            return Ok(Some(json_rpc_error(Value::Null, -32600, "Invalid Request")));
        }
        let responses = requests
            .iter()
            .map(handle_json_rpc_request)
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        return Ok((!responses.is_empty()).then(|| Value::Array(responses)));
    }
    handle_json_rpc_request(request)
}

fn handle_json_rpc_request(request: &Value) -> Result<Option<Value>> {
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
        "tools/list" => Ok(json!({ "tools": mcp_tools() })),
        "tools/call" => call_tool(request.get("params").unwrap_or(&Value::Null)),
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

fn mcp_tools() -> Vec<Value> {
    vec![json!({
        "name": "radio_fm_command",
        "description": "Run a radio-fm CLI command. Pass its arguments exactly as they would follow `radio-fm`, for example [\"schedule\", \"list\"] or [\"service\", \"set-volume\", \"0.5\"]. Long-running server commands (`service run`, `icecast start`, and `mcp run`) are not available through this request/response tool.",
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

fn call_tool(params: &Value) -> Result<Value> {
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
    if is_long_running_command(&arguments) {
        bail!("Long-running commands must be started directly from a terminal");
    }

    let executable = std::env::current_exe().context("Failed to locate radio-fm executable")?;
    let output = Command::new(executable)
        .args(&arguments)
        .output()
        .context("Failed to run radio-fm command")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(json!({
        "content": [{ "type": "text", "text": if text.is_empty() { "Command completed without output" } else { &text } }],
        "isError": !output.status.success()
    }))
}

fn is_long_running_command(arguments: &[String]) -> bool {
    matches!(
        (
            arguments.first().map(String::as_str),
            arguments.get(1).map(String::as_str)
        ),
        (Some("service"), Some("run"))
            | (Some("icecast"), Some("start"))
            | (Some("mcp"), Some("run"))
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

    #[test]
    fn exposes_the_cli_tool() {
        let tools = mcp_tools();
        assert_eq!(tools[0]["name"], "radio_fm_command");
    }

    #[test]
    fn rejects_server_commands_from_tool_calls() {
        assert!(is_long_running_command(&["service".into(), "run".into()]));
        assert!(is_long_running_command(&["icecast".into(), "start".into()]));
        assert!(is_long_running_command(&["mcp".into(), "run".into()]));
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
        let response = handle_json_rpc_message(&request)
            .expect("batch parses")
            .expect("request response exists");

        assert_eq!(response.as_array().expect("response is an array").len(), 1);
    }

    #[test]
    fn initialization_negotiates_the_supported_protocol_version() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "older-version" }
        });
        let response = handle_json_rpc_message(&request)
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
}
