use std::io::Read;
use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use url::{Host, Url};

const MAX_STDIN_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOKEN_BYTES: usize = 64 * 1024;
const MAX_OPERATIONS: usize = 128;
const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_TIMEOUT_MS: u64 = 300_000;
const CLIENT_PROTOCOL_VERSION: &str = "2025-06-18";
const SESSION_HEADER: &str = "mcp-session-id";
const PROTOCOL_HEADER: &str = "mcp-protocol-version";

const USAGE: &str = "Usage: webcodex mcp-bridge\n\n\
Read one local MCP bridge job as JSON from stdin, execute its tools/call operations\n\
sequentially against a loopback WebCodex Streamable HTTP MCP endpoint, and write one\n\
JSON result to stdout. The Bearer token must be supplied in stdin, never argv.\n\n\
Input:\n\
  {\n\
    \"endpoint\": \"http://127.0.0.1:<port>/mcp\",\n\
    \"bearerToken\": \"<secret>\",\n\
    \"operations\": [\n\
      {\"tool\": \"<tool-name>\", \"arguments\": {}}\n\
    ],\n\
    \"timeoutMs\": 120000\n\
  }\n\n\
Security:\n\
  The endpoint must be plain HTTP on 127.0.0.1, ::1, or localhost.\n\
  The Bearer token is never included in bridge output or diagnostics.\n\
  Execution stops at the first transport, JSON-RPC, or MCP tool error.\n";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BridgeJob {
    endpoint: String,
    bearer_token: String,
    operations: Vec<BridgeOperation>,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BridgeOperation {
    tool: String,
    #[serde(default = "empty_object")]
    arguments: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeOutput {
    ok: bool,
    protocol_version: Option<String>,
    completed: usize,
    results: Vec<BridgeOperationResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<BridgeError>,
}

#[derive(Debug, Serialize)]
struct BridgeOperationResult {
    index: usize,
    tool: String,
    ok: bool,
    result: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeError {
    kind: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rpc_code: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl BridgeError {
    fn input(message: impl Into<String>) -> Self {
        Self {
            kind: "input".to_string(),
            message: message.into(),
            operation_index: None,
            tool: None,
            rpc_code: None,
            data: None,
        }
    }

    fn runtime(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            operation_index: None,
            tool: None,
            rpc_code: None,
            data: None,
        }
    }

    fn for_operation(mut self, index: usize, tool: &str) -> Self {
        self.operation_index = Some(index);
        self.tool = Some(tool.to_string());
        self
    }
}

struct McpClient {
    http: reqwest::Client,
    endpoint: Url,
    bearer_token: String,
    session_id: Option<String>,
    protocol_version: Option<String>,
    next_id: u64,
}

pub(crate) async fn run() -> i32 {
    let trailing: Vec<String> = std::env::args().skip(2).collect();
    if trailing.len() == 1 && matches!(trailing[0].as_str(), "-h" | "--help") {
        print!("{USAGE}");
        return 0;
    }
    if !trailing.is_empty() {
        return write_input_error("mcp-bridge accepts no command-line options; provide the job on stdin");
    }

    let mut input = Vec::new();
    if let Err(error) = std::io::stdin()
        .take((MAX_STDIN_BYTES + 1) as u64)
        .read_to_end(&mut input)
    {
        return write_input_error(format!("failed to read stdin: {error}"));
    }
    if input.len() > MAX_STDIN_BYTES {
        return write_input_error(format!(
            "stdin JSON exceeds the {MAX_STDIN_BYTES}-byte limit"
        ));
    }

    let job: BridgeJob = match serde_json::from_slice(&input) {
        Ok(job) => job,
        Err(error) => return write_input_error(format!("invalid bridge JSON: {error}")),
    };

    let output = match execute_job(job).await {
        Ok(output) => output,
        Err(error) => BridgeOutput {
            ok: false,
            protocol_version: None,
            completed: 0,
            results: Vec::new(),
            error: Some(error),
        },
    };
    let code = if output.ok {
        0
    } else if output
        .error
        .as_ref()
        .is_some_and(|error| error.kind == "input")
    {
        2
    } else {
        1
    };
    write_output(&output);
    code
}

async fn execute_job(job: BridgeJob) -> Result<BridgeOutput, BridgeError> {
    validate_job(&job)?;
    let endpoint = validate_endpoint(&job.endpoint)?;
    let mut client = McpClient::new(endpoint, job.bearer_token, job.timeout_ms)?;

    if let Err(error) = client.initialize().await {
        return Ok(BridgeOutput {
            ok: false,
            protocol_version: client.protocol_version.clone(),
            completed: 0,
            results: Vec::new(),
            error: Some(error),
        });
    }

    let mut results = Vec::with_capacity(job.operations.len());
    for (index, operation) in job.operations.into_iter().enumerate() {
        let tool = operation.tool;
        match client.call_tool(&tool, operation.arguments).await {
            Ok(mut result) => {
                redact_value(&mut result, &client.bearer_token);
                let tool_ok = !result
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                results.push(BridgeOperationResult {
                    index,
                    tool: tool.clone(),
                    ok: tool_ok,
                    result,
                });
                if !tool_ok {
                    client.close().await;
                    return Ok(BridgeOutput {
                        ok: false,
                        protocol_version: client.protocol_version.clone(),
                        completed: results.len(),
                        results,
                        error: Some(
                            BridgeError::runtime(
                                "tool_error",
                                "MCP tool returned isError=true; later operations were not executed",
                            )
                            .for_operation(index, &tool),
                        ),
                    });
                }
            }
            Err(mut error) => {
                redact_error(&mut error, &client.bearer_token);
                let error = error.for_operation(index, &tool);
                client.close().await;
                return Ok(BridgeOutput {
                    ok: false,
                    protocol_version: client.protocol_version.clone(),
                    completed: results.len(),
                    results,
                    error: Some(error),
                });
            }
        }
    }

    client.close().await;
    Ok(BridgeOutput {
        ok: true,
        protocol_version: client.protocol_version.clone(),
        completed: results.len(),
        results,
        error: None,
    })
}

fn validate_job(job: &BridgeJob) -> Result<(), BridgeError> {
    if job.bearer_token.trim().is_empty() {
        return Err(BridgeError::input("bearerToken must not be empty"));
    }
    if job.bearer_token.len() > MAX_TOKEN_BYTES {
        return Err(BridgeError::input(format!(
            "bearerToken exceeds the {MAX_TOKEN_BYTES}-byte limit"
        )));
    }
    if job.operations.is_empty() {
        return Err(BridgeError::input("operations must contain at least one tool call"));
    }
    if job.operations.len() > MAX_OPERATIONS {
        return Err(BridgeError::input(format!(
            "operations exceeds the {MAX_OPERATIONS}-operation limit"
        )));
    }
    if !(1_000..=MAX_TIMEOUT_MS).contains(&job.timeout_ms) {
        return Err(BridgeError::input(format!(
            "timeoutMs must be between 1000 and {MAX_TIMEOUT_MS}"
        )));
    }
    for (index, operation) in job.operations.iter().enumerate() {
        if operation.tool.trim().is_empty() || operation.tool.len() > 256 {
            return Err(BridgeError::input(format!(
                "operations[{index}].tool must contain 1..=256 bytes"
            )));
        }
        if !operation.arguments.is_object() {
            return Err(BridgeError::input(format!(
                "operations[{index}].arguments must be a JSON object"
            )));
        }
    }
    Ok(())
}

fn validate_endpoint(value: &str) -> Result<Url, BridgeError> {
    let url = Url::parse(value).map_err(|error| {
        BridgeError::input(format!("endpoint is not a valid URL: {error}"))
    })?;
    if url.scheme() != "http" {
        return Err(BridgeError::input(
            "endpoint must use plain HTTP for the Desktop loopback MCP endpoint",
        ));
    }
    let loopback = match url.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    };
    if !loopback {
        return Err(BridgeError::input(
            "endpoint host must be 127.0.0.1, ::1, or localhost",
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(BridgeError::input(
            "endpoint must not contain URL userinfo credentials",
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(BridgeError::input(
            "endpoint must not contain a query string or fragment",
        ));
    }
    if !matches!(url.path(), "/mcp" | "/mcp/") {
        return Err(BridgeError::input(
            "endpoint path must be /mcp (use the URL shown by WebCodex Desktop)",
        ));
    }
    Ok(url)
}

impl McpClient {
    fn new(endpoint: Url, bearer_token: String, timeout_ms: u64) -> Result<Self, BridgeError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .no_proxy()
            .build()
            .map_err(|error| BridgeError::runtime("client", format!("failed to build HTTP client: {error}")))?;
        Ok(Self {
            http,
            endpoint,
            bearer_token,
            session_id: None,
            protocol_version: None,
            next_id: 1,
        })
    }

    async fn initialize(&mut self) -> Result<(), BridgeError> {
        let id = self.take_id();
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": CLIENT_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "webcodex-mcp-bridge",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        });
        let response = self.post_json(&payload, Some(id), false).await?;
        let result = response
            .and_then(|value| value.get("result").cloned())
            .ok_or_else(|| BridgeError::runtime("protocol", "initialize response is missing result"))?;
        let protocol_version = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                BridgeError::runtime("protocol", "initialize result is missing protocolVersion")
            })?;
        self.protocol_version = Some(protocol_version.to_string());

        let initialized = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        self.post_json(&initialized, None, true).await?;
        Ok(())
    }

    async fn call_tool(&mut self, tool: &str, arguments: Value) -> Result<Value, BridgeError> {
        let id = self.take_id();
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": tool,
                "arguments": arguments
            }
        });
        let response = self
            .post_json(&payload, Some(id), false)
            .await?
            .ok_or_else(|| BridgeError::runtime("protocol", "tools/call returned no response"))?;
        response
            .get("result")
            .cloned()
            .ok_or_else(|| BridgeError::runtime("protocol", "tools/call response is missing result"))
    }

    async fn post_json(
        &mut self,
        payload: &Value,
        expected_id: Option<u64>,
        allow_empty: bool,
    ) -> Result<Option<Value>, BridgeError> {
        let mut request = self
            .http
            .post(self.endpoint.clone())
            .header(ACCEPT, "application/json, text/event-stream")
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, format!("Bearer {}", self.bearer_token));
        if let Some(session_id) = self.session_id.as_deref() {
            request = request.header(SESSION_HEADER, session_id);
        }
        if let Some(protocol_version) = self.protocol_version.as_deref() {
            request = request.header(PROTOCOL_HEADER, protocol_version);
        }

        let response = request
            .json(payload)
            .send()
            .await
            .map_err(|error| BridgeError::runtime("transport", format!("MCP request failed: {error}")))?;

        if let Some(value) = response.headers().get(SESSION_HEADER) {
            let session_id = value.to_str().map_err(|_| {
                BridgeError::runtime("protocol", "MCP session header is not valid ASCII")
            })?;
            if self.session_id.is_none() && !session_id.trim().is_empty() {
                self.session_id = Some(session_id.to_string());
            }
        }

        let status = response.status();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if response.content_length().is_some_and(|size| size > MAX_RESPONSE_BYTES as u64) {
            return Err(BridgeError::runtime(
                "response_too_large",
                format!("MCP response exceeds the {MAX_RESPONSE_BYTES}-byte limit"),
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| BridgeError::runtime("transport", format!("failed to read MCP response: {error}")))?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(BridgeError::runtime(
                "response_too_large",
                format!("MCP response exceeds the {MAX_RESPONSE_BYTES}-byte limit"),
            ));
        }
        let body = String::from_utf8_lossy(&bytes);

        if !status.is_success() {
            let excerpt = redact_text(&bounded_excerpt(&body, 4096), &self.bearer_token);
            return Err(BridgeError::runtime(
                "http",
                if excerpt.is_empty() {
                    format!("MCP endpoint returned HTTP {status}")
                } else {
                    format!("MCP endpoint returned HTTP {status}: {excerpt}")
                },
            ));
        }
        if body.trim().is_empty() {
            if allow_empty {
                return Ok(None);
            }
            return Err(BridgeError::runtime("protocol", "MCP response body is empty"));
        }
        if expected_id.is_none() && allow_empty {
            return Ok(None);
        }

        let expected_id = expected_id.ok_or_else(|| {
            BridgeError::runtime("protocol", "internal bridge request id is missing")
        })?;
        let value = if content_type.contains("text/event-stream") {
            parse_sse_response(&body, expected_id)?
        } else {
            match serde_json::from_str::<Value>(&body) {
                Ok(value) => value,
                Err(_) => parse_sse_response(&body, expected_id)?,
            }
        };
        validate_rpc_response(value, expected_id)
    }

    async fn close(&self) {
        let Some(session_id) = self.session_id.as_deref() else {
            return;
        };
        let mut request = self
            .http
            .delete(self.endpoint.clone())
            .header(AUTHORIZATION, format!("Bearer {}", self.bearer_token))
            .header(SESSION_HEADER, session_id);
        if let Some(protocol_version) = self.protocol_version.as_deref() {
            request = request.header(PROTOCOL_HEADER, protocol_version);
        }
        let _ = request.send().await;
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

fn validate_rpc_response(value: Value, expected_id: u64) -> Result<Option<Value>, BridgeError> {
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(BridgeError::runtime(
            "protocol",
            "MCP response is not JSON-RPC 2.0",
        ));
    }
    if value.get("id") != Some(&Value::from(expected_id)) {
        return Err(BridgeError::runtime(
            "protocol",
            format!("MCP response id does not match request id {expected_id}"),
        ));
    }
    if let Some(error) = value.get("error") {
        let code = error.get("code").and_then(Value::as_i64);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("MCP JSON-RPC error")
            .to_string();
        return Err(BridgeError {
            kind: "json_rpc".to_string(),
            message,
            operation_index: None,
            tool: None,
            rpc_code: code,
            data: error.get("data").cloned(),
        });
    }
    Ok(Some(value))
}

fn parse_sse_response(body: &str, expected_id: u64) -> Result<Value, BridgeError> {
    let mut data_lines = Vec::new();
    for line in body.lines().chain(std::iter::once("")) {
        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.trim_start());
            continue;
        }
        if line.is_empty() && !data_lines.is_empty() {
            let data = data_lines.join("\n");
            data_lines.clear();
            if data == "[DONE]" {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<Value>(&data) {
                if value.get("id") == Some(&Value::from(expected_id)) {
                    return Ok(value);
                }
            }
        }
    }
    Err(BridgeError::runtime(
        "protocol",
        format!("SSE response did not contain JSON-RPC id {expected_id}"),
    ))
}

fn redact_error(error: &mut BridgeError, token: &str) {
    error.message = redact_text(&error.message, token);
    if let Some(data) = error.data.as_mut() {
        redact_value(data, token);
    }
}

fn redact_value(value: &mut Value, token: &str) {
    match value {
        Value::String(text) => *text = redact_text(text, token),
        Value::Array(values) => {
            for value in values {
                redact_value(value, token);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                redact_value(value, token);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn redact_text(text: &str, token: &str) -> String {
    if token.is_empty() {
        text.to_string()
    } else {
        text.replace(token, "[REDACTED]")
    }
}

fn bounded_excerpt(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect::<String>().trim().to_string()
}

fn write_input_error(message: impl Into<String>) -> i32 {
    let output = BridgeOutput {
        ok: false,
        protocol_version: None,
        completed: 0,
        results: Vec::new(),
        error: Some(BridgeError::input(message)),
    };
    write_output(&output);
    2
}

fn write_output(output: &BridgeOutput) {
    match serde_json::to_string(output) {
        Ok(json) => println!("{json}"),
        Err(error) => println!(
            "{{\"ok\":false,\"protocolVersion\":null,\"completed\":0,\"results\":[],\"error\":{{\"kind\":\"serialization\",\"message\":{}}}}}",
            serde_json::to_string(&error.to_string()).unwrap_or_else(|_| "\"serialization failure\"".to_string())
        ),
    }
}

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_only_loopback_mcp_http_urls() {
        assert!(validate_endpoint("http://127.0.0.1:58208/mcp").is_ok());
        assert!(validate_endpoint("http://localhost:58208/mcp").is_ok());
        assert!(validate_endpoint("http://[::1]:58208/mcp").is_ok());
        assert!(validate_endpoint("https://127.0.0.1:58208/mcp").is_err());
        assert!(validate_endpoint("http://192.168.1.10:58208/mcp").is_err());
        assert!(validate_endpoint("http://127.0.0.1:58208/readyz").is_err());
        assert!(validate_endpoint("http://127.0.0.1:58208/mcp?token=secret").is_err());
    }

    #[test]
    fn bridge_job_requires_object_arguments_and_nonempty_secret() {
        let job: BridgeJob = serde_json::from_value(json!({
            "endpoint": "http://127.0.0.1:1234/mcp",
            "bearerToken": "secret",
            "operations": [{"tool": "list_jobs", "arguments": {}}]
        }))
        .unwrap();
        assert!(validate_job(&job).is_ok());

        let bad: BridgeJob = serde_json::from_value(json!({
            "endpoint": "http://127.0.0.1:1234/mcp",
            "bearerToken": "secret",
            "operations": [{"tool": "list_jobs", "arguments": []}]
        }))
        .unwrap();
        assert!(validate_job(&bad).is_err());
    }

    #[test]
    fn parses_streamable_http_sse_response() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n\n";
        let value = parse_sse_response(body, 7).unwrap();
        assert_eq!(value["result"]["ok"], Value::Bool(true));
    }

    #[test]
    fn redacts_bearer_token_recursively() {
        let mut value = json!({
            "message": "credential=top-secret",
            "nested": ["top-secret", {"value": "prefix top-secret suffix"}]
        });
        redact_value(&mut value, "top-secret");
        let rendered = serde_json::to_string(&value).unwrap();
        assert!(!rendered.contains("top-secret"));
        assert!(rendered.contains("[REDACTED]"));
    }
}