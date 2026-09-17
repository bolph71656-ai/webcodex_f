use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use url::{Host, Url};

const MAX_STDIN_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOKEN_BYTES: usize = 64 * 1024;
const MAX_DESKTOP_STATE_BYTES: usize = 256 * 1024;
const MAX_OPERATIONS: usize = 128;
const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_TIMEOUT_MS: u64 = 300_000;
const DEFAULT_AWAIT_TIMEOUT_MS: u64 = 120_000;
const DEFAULT_AWAIT_INTERVAL_MS: u64 = 1_000;
const MIN_AWAIT_INTERVAL_MS: u64 = 100;
const CLIENT_PROTOCOL_VERSION: &str = "2025-06-18";
const SESSION_HEADER: &str = "mcp-session-id";
const PROTOCOL_HEADER: &str = "mcp-protocol-version";
const DESKTOP_APP_ID: &str = "dev.webcodex.desktop";
const DESKTOP_STATE_FILE: &str = "desktop-state.json";

const USAGE: &str = "Usage: webcodex mcp-bridge\n\n\
Read one local MCP bridge job as JSON from stdin, execute its tools/call operations\n\
sequentially against a loopback WebCodex Streamable HTTP MCP endpoint, and write one\n\
JSON result to stdout.\n\n\
Connection forms:\n\
  {\"connection\":\"desktop\", ...}\n\
  {\"endpoint\":\"http://127.0.0.1:<port>/mcp\",\"bearerToken\":\"<secret>\", ...}\n\n\
Desktop mode reads the Desktop-owned local state and credential file. Manual mode\n\
keeps backward compatibility and accepts the Bearer token only through stdin.\n\n\
Operation extras:\n\
  \"capture\":{\"jobId\":\"/structuredContent/job/id\"}\n\
  arguments may use {\"$ref\":\"jobId\"} for typed substitution or \"${jobId}\" in strings.\n\
  \"await\" can poll another MCP tool until a JSON-pointer predicate matches.\n\n\
Security:\n\
  The endpoint must be plain HTTP on 127.0.0.1, ::1, or localhost.\n\
  The Bearer token is never included in bridge output or diagnostics.\n\
  Execution stops at the first transport, JSON-RPC, MCP tool, reference, or await error.\n";

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ConnectionMode {
    Desktop,
    Manual,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BridgeJob {
    #[serde(default)]
    connection: Option<ConnectionMode>,
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default)]
    bearer_token: Option<String>,
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
    #[serde(default)]
    capture: BTreeMap<String, String>,
    #[serde(default, rename = "await")]
    wait: Option<AwaitSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AwaitSpec {
    tool: String,
    #[serde(default = "empty_object")]
    arguments: Value,
    until: Predicate,
    #[serde(default)]
    success: Option<Predicate>,
    #[serde(default)]
    capture: BTreeMap<String, String>,
    #[serde(default = "default_await_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_await_timeout_ms")]
    timeout_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Predicate {
    pointer: String,
    #[serde(default)]
    equals: Option<Value>,
    #[serde(default)]
    any_of: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct DesktopStoredConfig {
    runtime: Option<DesktopStoredRuntime>,
}

#[derive(Debug, Deserialize)]
struct DesktopStoredRuntime {
    server_url: String,
    user_token_file: Option<PathBuf>,
}

#[derive(Debug)]
struct ResolvedConnection {
    endpoint: Url,
    bearer_token: String,
    source: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeOutput {
    ok: bool,
    connection: Option<&'static str>,
    protocol_version: Option<String>,
    completed: usize,
    results: Vec<BridgeOperationResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<BridgeError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeOperationResult {
    index: usize,
    tool: String,
    ok: bool,
    result: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    await_result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    await_polls: Option<usize>,
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
        return write_input_error(
            "mcp-bridge accepts no command-line options; provide the job on stdin",
        );
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
            connection: None,
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
    let connection = resolve_connection(&job)?;
    let connection_source = connection.source;
    let mut client = McpClient::new(connection.endpoint, connection.bearer_token, job.timeout_ms)?;

    if let Err(mut error) = client.initialize().await {
        redact_error(&mut error, &client.bearer_token);
        return Ok(BridgeOutput {
            ok: false,
            connection: Some(connection_source),
            protocol_version: client.protocol_version.clone(),
            completed: 0,
            results: Vec::new(),
            error: Some(error),
        });
    }

    let mut results = Vec::with_capacity(job.operations.len());
    let mut variables = BTreeMap::<String, Value>::new();

    for (index, operation) in job.operations.into_iter().enumerate() {
        let tool = operation.tool;
        let arguments = match resolve_references(operation.arguments, &variables) {
            Ok(arguments) => arguments,
            Err(error) => {
                client.close().await;
                return Ok(failed_output(
                    connection_source,
                    &client,
                    results,
                    error.for_operation(index, &tool),
                ));
            }
        };

        let mut result = match client.call_tool(&tool, arguments).await {
            Ok(result) => result,
            Err(mut error) => {
                redact_error(&mut error, &client.bearer_token);
                client.close().await;
                return Ok(failed_output(
                    connection_source,
                    &client,
                    results,
                    error.for_operation(index, &tool),
                ));
            }
        };

        redact_value(&mut result, &client.bearer_token);
        if is_tool_error(&result) {
            results.push(BridgeOperationResult {
                index,
                tool: tool.clone(),
                ok: false,
                result,
                await_result: None,
                await_polls: None,
            });
            client.close().await;
            return Ok(failed_output(
                connection_source,
                &client,
                results,
                BridgeError::runtime(
                    "tool_error",
                    "MCP tool returned isError=true; later operations were not executed",
                )
                .for_operation(index, &tool),
            ));
        }

        if let Err(error) = capture_values(&operation.capture, &result, &mut variables) {
            results.push(BridgeOperationResult {
                index,
                tool: tool.clone(),
                ok: false,
                result,
                await_result: None,
                await_polls: None,
            });
            client.close().await;
            return Ok(failed_output(
                connection_source,
                &client,
                results,
                error.for_operation(index, &tool),
            ));
        }

        let (await_result, await_polls) = if let Some(wait) = operation.wait {
            match execute_await(&mut client, index, &tool, wait, &mut variables).await {
                Ok(value) => value,
                Err(mut error) => {
                    redact_error(&mut error, &client.bearer_token);
                    results.push(BridgeOperationResult {
                        index,
                        tool: tool.clone(),
                        ok: false,
                        result,
                        await_result: None,
                        await_polls: None,
                    });
                    client.close().await;
                    return Ok(failed_output(
                        connection_source,
                        &client,
                        results,
                        error.for_operation(index, &tool),
                    ));
                }
            }
        } else {
            (None, None)
        };

        results.push(BridgeOperationResult {
            index,
            tool,
            ok: true,
            result,
            await_result,
            await_polls,
        });
    }

    client.close().await;
    Ok(BridgeOutput {
        ok: true,
        connection: Some(connection_source),
        protocol_version: client.protocol_version.clone(),
        completed: results.len(),
        results,
        error: None,
    })
}

fn failed_output(
    connection: &'static str,
    client: &McpClient,
    results: Vec<BridgeOperationResult>,
    error: BridgeError,
) -> BridgeOutput {
    BridgeOutput {
        ok: false,
        connection: Some(connection),
        protocol_version: client.protocol_version.clone(),
        completed: results.len(),
        results,
        error: Some(error),
    }
}

async fn execute_await(
    client: &mut McpClient,
    operation_index: usize,
    parent_tool: &str,
    wait: AwaitSpec,
    variables: &mut BTreeMap<String, Value>,
) -> Result<(Option<Value>, Option<usize>), BridgeError> {
    validate_await(&wait, operation_index)?;
    let started = Instant::now();
    let timeout = Duration::from_millis(wait.timeout_ms);
    let interval = Duration::from_millis(wait.interval_ms);
    let mut polls = 0usize;

    loop {
        if started.elapsed() >= timeout {
            return Err(BridgeError::runtime(
                "await_timeout",
                format!(
                    "await for operation {operation_index} ({parent_tool}) exceeded {} ms",
                    wait.timeout_ms
                ),
            ));
        }
        if polls > 0 {
            tokio::time::sleep(interval).await;
        }

        let arguments = resolve_references(wait.arguments.clone(), variables)?;
        let mut result = client.call_tool(&wait.tool, arguments).await?;
        redact_value(&mut result, &client.bearer_token);
        polls += 1;

        if is_tool_error(&result) {
            return Err(BridgeError::runtime(
                "tool_error",
                format!(
                    "await tool {} returned isError=true; polling stopped",
                    wait.tool
                ),
            ));
        }

        if predicate_matches(&wait.until, &result)? {
            if let Some(success) = wait.success.as_ref() {
                if !predicate_matches(success, &result)? {
                    return Err(BridgeError {
                        kind: "await_failed".to_string(),
                        message: format!(
                            "await condition completed but success predicate did not match for {}",
                            wait.tool
                        ),
                        operation_index: None,
                        tool: None,
                        rpc_code: None,
                        data: Some(result),
                    });
                }
            }
            capture_values(&wait.capture, &result, variables)?;
            return Ok((Some(result), Some(polls)));
        }
    }
}

fn validate_job(job: &BridgeJob) -> Result<(), BridgeError> {
    if job.operations.is_empty() {
        return Err(BridgeError::input(
            "operations must contain at least one tool call",
        ));
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

    match job.connection {
        Some(ConnectionMode::Desktop) => {
            if job.endpoint.is_some() || job.bearer_token.is_some() {
                return Err(BridgeError::input(
                    "connection=desktop must not include endpoint or bearerToken",
                ));
            }
        }
        Some(ConnectionMode::Manual) | None => {
            let endpoint_present = job
                .endpoint
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
            let token_present = job
                .bearer_token
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
            if !endpoint_present || !token_present {
                return Err(BridgeError::input(
                    "manual bridge jobs require endpoint and bearerToken; use connection=desktop for automatic Desktop discovery",
                ));
            }
            if job
                .bearer_token
                .as_ref()
                .is_some_and(|value| value.len() > MAX_TOKEN_BYTES)
            {
                return Err(BridgeError::input(format!(
                    "bearerToken exceeds the {MAX_TOKEN_BYTES}-byte limit"
                )));
            }
        }
    }

    for (index, operation) in job.operations.iter().enumerate() {
        validate_tool_name(&operation.tool, &format!("operations[{index}].tool"))?;
        if !operation.arguments.is_object() {
            return Err(BridgeError::input(format!(
                "operations[{index}].arguments must be a JSON object"
            )));
        }
        validate_capture_map(&operation.capture, &format!("operations[{index}].capture"))?;
        if let Some(wait) = operation.wait.as_ref() {
            validate_await(wait, index)?;
        }
    }
    Ok(())
}

fn validate_await(wait: &AwaitSpec, index: usize) -> Result<(), BridgeError> {
    validate_tool_name(&wait.tool, &format!("operations[{index}].await.tool"))?;
    if !wait.arguments.is_object() {
        return Err(BridgeError::input(format!(
            "operations[{index}].await.arguments must be a JSON object"
        )));
    }
    if !(MIN_AWAIT_INTERVAL_MS..=60_000).contains(&wait.interval_ms) {
        return Err(BridgeError::input(format!(
            "operations[{index}].await.intervalMs must be between {MIN_AWAIT_INTERVAL_MS} and 60000"
        )));
    }
    if !(1_000..=MAX_TIMEOUT_MS).contains(&wait.timeout_ms) {
        return Err(BridgeError::input(format!(
            "operations[{index}].await.timeoutMs must be between 1000 and {MAX_TIMEOUT_MS}"
        )));
    }
    validate_predicate(&wait.until, &format!("operations[{index}].await.until"))?;
    if let Some(success) = wait.success.as_ref() {
        validate_predicate(success, &format!("operations[{index}].await.success"))?;
    }
    validate_capture_map(&wait.capture, &format!("operations[{index}].await.capture"))?;
    Ok(())
}

fn validate_tool_name(value: &str, field: &str) -> Result<(), BridgeError> {
    if value.trim().is_empty() || value.len() > 256 {
        return Err(BridgeError::input(format!(
            "{field} must contain 1..=256 bytes"
        )));
    }
    Ok(())
}

fn validate_capture_map(
    capture: &BTreeMap<String, String>,
    field: &str,
) -> Result<(), BridgeError> {
    for (name, pointer) in capture {
        if !valid_variable_name(name) {
            return Err(BridgeError::input(format!(
                "{field} contains invalid variable name {name:?}"
            )));
        }
        if !pointer.is_empty() && !pointer.starts_with('/') {
            return Err(BridgeError::input(format!(
                "{field}.{name} must be an RFC 6901 JSON pointer"
            )));
        }
    }
    Ok(())
}

fn validate_predicate(predicate: &Predicate, field: &str) -> Result<(), BridgeError> {
    if !predicate.pointer.is_empty() && !predicate.pointer.starts_with('/') {
        return Err(BridgeError::input(format!(
            "{field}.pointer must be an RFC 6901 JSON pointer"
        )));
    }
    let modes = (predicate.equals.is_some() as usize) + (!predicate.any_of.is_empty() as usize);
    if modes != 1 {
        return Err(BridgeError::input(format!(
            "{field} must specify exactly one of equals or anyOf"
        )));
    }
    Ok(())
}

fn resolve_connection(job: &BridgeJob) -> Result<ResolvedConnection, BridgeError> {
    match job.connection {
        Some(ConnectionMode::Desktop) => discover_desktop_connection(),
        Some(ConnectionMode::Manual) | None => {
            let endpoint = validate_endpoint(job.endpoint.as_deref().unwrap_or_default())?;
            let bearer_token = job.bearer_token.clone().unwrap_or_default();
            if bearer_token.trim().is_empty() {
                return Err(BridgeError::input("bearerToken must not be empty"));
            }
            Ok(ResolvedConnection {
                endpoint,
                bearer_token,
                source: "manual",
            })
        }
    }
}

fn discover_desktop_connection() -> Result<ResolvedConnection, BridgeError> {
    let state_path = desktop_state_path()?;
    discover_desktop_connection_from_state_path(&state_path)
}

fn discover_desktop_connection_from_state_path(
    path: &Path,
) -> Result<ResolvedConnection, BridgeError> {
    let bytes = read_bounded_file(path, MAX_DESKTOP_STATE_BYTES, "Desktop state")?;
    let config: DesktopStoredConfig = serde_json::from_slice(&bytes).map_err(|_| {
        BridgeError::runtime(
            "desktop_state",
            "Desktop state is unreadable; start or reconfigure WebCodex Desktop",
        )
    })?;
    let runtime = config.runtime.ok_or_else(|| {
        BridgeError::runtime("desktop_state", "Desktop local runtime is not configured")
    })?;

    let endpoint = desktop_mcp_url(&runtime.server_url)?;
    let token_path = runtime.user_token_file.ok_or_else(|| {
        BridgeError::runtime(
            "desktop_state",
            "Desktop local runtime credential path is unavailable",
        )
    })?;
    let token_bytes = read_bounded_file(&token_path, MAX_TOKEN_BYTES, "Desktop MCP credential")?;
    let token = String::from_utf8(token_bytes).map_err(|_| {
        BridgeError::runtime("desktop_state", "Desktop MCP credential is not valid UTF-8")
    })?;
    let bearer_token = token.trim().to_string();
    if bearer_token.is_empty() {
        return Err(BridgeError::runtime(
            "desktop_state",
            "Desktop MCP credential is empty",
        ));
    }

    Ok(ResolvedConnection {
        endpoint,
        bearer_token,
        source: "desktop",
    })
}

fn desktop_state_path() -> Result<PathBuf, BridgeError> {
    if let Some(path) = std::env::var_os("WEBCODEX_DESKTOP_STATE_FILE") {
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }

    #[cfg(target_os = "windows")]
    {
        let root = std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
            BridgeError::runtime(
                "desktop_state",
                "LOCALAPPDATA is unavailable; set WEBCODEX_DESKTOP_STATE_FILE explicitly",
            )
        })?;
        return Ok(PathBuf::from(root)
            .join(DESKTOP_APP_ID)
            .join(DESKTOP_STATE_FILE));
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").ok_or_else(|| {
            BridgeError::runtime(
                "desktop_state",
                "HOME is unavailable; set WEBCODEX_DESKTOP_STATE_FILE explicitly",
            )
        })?;
        return Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join(DESKTOP_APP_ID)
            .join(DESKTOP_STATE_FILE));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(root) = std::env::var_os("XDG_DATA_HOME") {
            if !root.is_empty() {
                return Ok(PathBuf::from(root)
                    .join(DESKTOP_APP_ID)
                    .join(DESKTOP_STATE_FILE));
            }
        }
        let home = std::env::var_os("HOME").ok_or_else(|| {
            BridgeError::runtime(
                "desktop_state",
                "HOME is unavailable; set WEBCODEX_DESKTOP_STATE_FILE explicitly",
            )
        })?;
        return Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join(DESKTOP_APP_ID)
            .join(DESKTOP_STATE_FILE));
    }

    #[allow(unreachable_code)]
    Err(BridgeError::runtime(
        "desktop_state",
        "automatic Desktop discovery is not supported on this platform; set WEBCODEX_DESKTOP_STATE_FILE explicitly",
    ))
}

fn read_bounded_file(path: &Path, max_bytes: usize, label: &str) -> Result<Vec<u8>, BridgeError> {
    let file = File::open(path)
        .map_err(|_| BridgeError::runtime("desktop_state", format!("{label} is unavailable")))?;
    let mut bytes = Vec::new();
    file.take((max_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| BridgeError::runtime("desktop_state", format!("failed to read {label}")))?;
    if bytes.is_empty() || bytes.len() > max_bytes {
        return Err(BridgeError::runtime(
            "desktop_state",
            format!("{label} is empty or exceeds the {max_bytes}-byte limit"),
        ));
    }
    Ok(bytes)
}

fn desktop_mcp_url(server_url: &str) -> Result<Url, BridgeError> {
    let mut url = Url::parse(server_url)
        .map_err(|_| BridgeError::runtime("desktop_state", "Desktop server URL is invalid"))?;
    if url.scheme() != "http" || !is_loopback_host(&url) {
        return Err(BridgeError::runtime(
            "desktop_state",
            "Desktop refused non-loopback MCP discovery data",
        ));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(BridgeError::runtime(
            "desktop_state",
            "Desktop server URL contains unsupported credentials, query, or fragment",
        ));
    }
    url.set_path("/mcp");
    validate_endpoint(url.as_str())
}

fn validate_endpoint(value: &str) -> Result<Url, BridgeError> {
    let url = Url::parse(value)
        .map_err(|error| BridgeError::input(format!("endpoint is not a valid URL: {error}")))?;
    if url.scheme() != "http" {
        return Err(BridgeError::input(
            "endpoint must use plain HTTP for the Desktop loopback MCP endpoint",
        ));
    }
    if !is_loopback_host(&url) {
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

fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

fn capture_values(
    capture: &BTreeMap<String, String>,
    result: &Value,
    variables: &mut BTreeMap<String, Value>,
) -> Result<(), BridgeError> {
    for (name, pointer) in capture {
        let value = result.pointer(pointer).cloned().ok_or_else(|| {
            BridgeError::runtime(
                "reference",
                format!("capture {name:?} could not resolve JSON pointer {pointer:?}"),
            )
        })?;
        variables.insert(name.clone(), value);
    }
    Ok(())
}

fn resolve_references(
    value: Value,
    variables: &BTreeMap<String, Value>,
) -> Result<Value, BridgeError> {
    match value {
        Value::Object(mut object) => {
            if object.len() == 1 {
                if let Some(reference) = object.remove("$ref") {
                    let name = reference.as_str().ok_or_else(|| {
                        BridgeError::runtime("reference", "$ref value must be a string")
                    })?;
                    return variables.get(name).cloned().ok_or_else(|| {
                        BridgeError::runtime(
                            "reference",
                            format!("unknown captured variable {name:?}"),
                        )
                    });
                }
            }
            for value in object.values_mut() {
                let current = std::mem::replace(value, Value::Null);
                *value = resolve_references(current, variables)?;
            }
            Ok(Value::Object(object))
        }
        Value::Array(mut values) => {
            for value in &mut values {
                let current = std::mem::replace(value, Value::Null);
                *value = resolve_references(current, variables)?;
            }
            Ok(Value::Array(values))
        }
        Value::String(text) => Ok(Value::String(interpolate_string(&text, variables)?)),
        scalar => Ok(scalar),
    }
}

fn interpolate_string(
    text: &str,
    variables: &BTreeMap<String, Value>,
) -> Result<String, BridgeError> {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| BridgeError::runtime("reference", "unterminated ${...} reference"))?;
        let name = &after[..end];
        if !valid_variable_name(name) {
            return Err(BridgeError::runtime(
                "reference",
                format!("invalid variable reference {name:?}"),
            ));
        }
        let value = variables.get(name).ok_or_else(|| {
            BridgeError::runtime("reference", format!("unknown captured variable {name:?}"))
        })?;
        output.push_str(&scalar_to_string(value).ok_or_else(|| {
            BridgeError::runtime(
                "reference",
                format!(
                    "variable {name:?} is not scalar; use {{\"$ref\":\"{name}\"}} for typed substitution"
                ),
            )
        })?);
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    Ok(output)
}

fn scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null => Some("null".to_string()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

fn valid_variable_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    if value.len() > 64 {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn predicate_matches(predicate: &Predicate, value: &Value) -> Result<bool, BridgeError> {
    let actual = value.pointer(&predicate.pointer).ok_or_else(|| {
        BridgeError::runtime(
            "await_condition",
            format!(
                "await predicate could not resolve JSON pointer {:?}",
                predicate.pointer
            ),
        )
    })?;
    if let Some(expected) = predicate.equals.as_ref() {
        return Ok(actual == expected);
    }
    Ok(predicate.any_of.iter().any(|expected| actual == expected))
}

fn is_tool_error(result: &Value) -> bool {
    result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

impl McpClient {
    fn new(endpoint: Url, bearer_token: String, timeout_ms: u64) -> Result<Self, BridgeError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .no_proxy()
            .build()
            .map_err(|error| {
                BridgeError::runtime("client", format!("failed to build HTTP client: {error}"))
            })?;
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
            .ok_or_else(|| {
                BridgeError::runtime("protocol", "initialize response is missing result")
            })?;
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
        response.get("result").cloned().ok_or_else(|| {
            BridgeError::runtime("protocol", "tools/call response is missing result")
        })
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

        let response = request.json(payload).send().await.map_err(|error| {
            BridgeError::runtime("transport", format!("MCP request failed: {error}"))
        })?;

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
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
        {
            return Err(BridgeError::runtime(
                "response_too_large",
                format!("MCP response exceeds the {MAX_RESPONSE_BYTES}-byte limit"),
            ));
        }
        let bytes = response.bytes().await.map_err(|error| {
            BridgeError::runtime("transport", format!("failed to read MCP response: {error}"))
        })?;
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
            return Err(BridgeError::runtime(
                "protocol",
                "MCP response body is empty",
            ));
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
    text.chars()
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

fn write_input_error(message: impl Into<String>) -> i32 {
    let output = BridgeOutput {
        ok: false,
        connection: None,
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
            "{{\"ok\":false,\"connection\":null,\"protocolVersion\":null,\"completed\":0,\"results\":[],\"error\":{{\"kind\":\"serialization\",\"message\":{}}}}}",
            serde_json::to_string(&error.to_string())
                .unwrap_or_else(|_| "\"serialization failure\"".to_string())
        ),
    }
}

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

fn default_await_timeout_ms() -> u64 {
    DEFAULT_AWAIT_TIMEOUT_MS
}

fn default_await_interval_ms() -> u64 {
    DEFAULT_AWAIT_INTERVAL_MS
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
    fn preserves_manual_job_compatibility_and_accepts_desktop_mode() {
        let manual: BridgeJob = serde_json::from_value(json!({
            "endpoint": "http://127.0.0.1:1234/mcp",
            "bearerToken": "secret",
            "operations": [{"tool": "list_jobs", "arguments": {}}]
        }))
        .unwrap();
        assert!(validate_job(&manual).is_ok());

        let desktop: BridgeJob = serde_json::from_value(json!({
            "connection": "desktop",
            "operations": [{"tool": "list_jobs", "arguments": {}}]
        }))
        .unwrap();
        assert!(validate_job(&desktop).is_ok());

        let mixed: BridgeJob = serde_json::from_value(json!({
            "connection": "desktop",
            "endpoint": "http://127.0.0.1:1234/mcp",
            "operations": [{"tool": "list_jobs", "arguments": {}}]
        }))
        .unwrap();
        assert!(validate_job(&mixed).is_err());
    }

    #[test]
    fn resolves_typed_and_string_references() {
        let variables = BTreeMap::from([
            ("jobId".to_string(), Value::String("job-123".to_string())),
            ("count".to_string(), Value::from(3)),
            ("payload".to_string(), json!({"nested": true})),
        ]);
        let value = json!({
            "job": {"$ref": "jobId"},
            "label": "job=${jobId};count=${count}",
            "payload": {"$ref": "payload"}
        });
        let resolved = resolve_references(value, &variables).unwrap();
        assert_eq!(resolved["job"], "job-123");
        assert_eq!(resolved["label"], "job=job-123;count=3");
        assert_eq!(resolved["payload"]["nested"], true);
    }

    #[test]
    fn captures_json_pointer_values() {
        let mut variables = BTreeMap::new();
        let capture =
            BTreeMap::from([("jobId".to_string(), "/structuredContent/job/id".to_string())]);
        let result = json!({"structuredContent":{"job":{"id":"job-9"}}});
        capture_values(&capture, &result, &mut variables).unwrap();
        assert_eq!(variables["jobId"], "job-9");
    }

    #[test]
    fn predicates_match_equals_and_any_of() {
        let value = json!({"structuredContent":{"job":{"status":"completed"}}});
        let equals: Predicate = serde_json::from_value(json!({
            "pointer": "/structuredContent/job/status",
            "equals": "completed"
        }))
        .unwrap();
        assert!(predicate_matches(&equals, &value).unwrap());

        let any_of: Predicate = serde_json::from_value(json!({
            "pointer": "/structuredContent/job/status",
            "anyOf": ["completed", "failed"]
        }))
        .unwrap();
        assert!(predicate_matches(&any_of, &value).unwrap());
    }

    #[test]
    fn discovers_desktop_connection_from_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("token.txt");
        std::fs::write(&token_path, "secret-token\n").unwrap();
        let state_path = dir.path().join("desktop-state.json");
        std::fs::write(
            &state_path,
            serde_json::to_vec(&json!({
                "runtime": {
                    "server_url": "http://127.0.0.1:58208",
                    "user_token_file": token_path,
                    "server_env_file": null,
                    "runner_config": null,
                    "project_id": null,
                    "runtime_project_id": null
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let connection = discover_desktop_connection_from_state_path(&state_path).unwrap();
        assert_eq!(connection.endpoint.as_str(), "http://127.0.0.1:58208/mcp");
        assert_eq!(connection.bearer_token, "secret-token");
        assert_eq!(connection.source, "desktop");
    }

    #[test]
    fn parses_streamable_http_sse_response() {
        let body =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n\n";
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
