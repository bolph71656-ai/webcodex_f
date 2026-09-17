# WebCodex local MCP JSON Bridge

`webcodex mcp-bridge` is a small local automation adapter for WebCodex Desktop. It reads one JSON job from standard input, opens one authenticated Streamable HTTP MCP session to the Desktop-owned loopback endpoint, runs the requested `tools/call` operations **in order**, and writes one machine-readable JSON result to standard output.

It is intended for a local agent or automation process that needs one deterministic process invocation instead of implementing MCP session handling itself. The bridge can discover the current Desktop endpoint and credential automatically, carry values from one tool result into later calls, and poll a follow-up tool until a bounded completion condition is met.

## Security boundary

The bridge deliberately keeps the same local-only boundary as WebCodex Desktop:

- the endpoint must use plain HTTP on `127.0.0.1`, `::1`, or `localhost`;
- the endpoint path must be `/mcp`;
- URL credentials, query strings, and fragments are rejected;
- Bearer authentication remains mandatory;
- manual Bearer credentials are accepted only through stdin JSON, not command-line arguments;
- Desktop discovery reads the existing Desktop state and its referenced local credential file instead of creating another persistent secret;
- the bridge disables proxy discovery for the local MCP connection;
- the Bearer token is redacted from bridge-generated error/result strings before output;
- execution stops after the first transport, JSON-RPC, MCP tool, reference, or await failure, so later effectful calls are not silently attempted after an uncertain result.

The bridge does not widen WebCodex project, Runner, tool, or scope permissions. Every call still passes through the ordinary WebCodex MCP/runtime authorization path.

## Recommended Desktop input

When WebCodex Desktop owns the local runtime, use automatic discovery:

```json
{
  "connection": "desktop",
  "operations": [
    {
      "tool": "list_jobs",
      "arguments": {}
    }
  ],
  "timeoutMs": 120000
}
```

The bridge reads the current Desktop `desktop-state.json`, derives the loopback `/mcp` URL from the saved local `server_url`, and reads the current `user_token_file`. The dynamic port therefore does not need to be copied or hard-coded.

Default Desktop state locations are platform-local application-data paths. On Windows the default is `%LOCALAPPDATA%\dev.webcodex.desktop\desktop-state.json`. `WEBCODEX_DESKTOP_STATE_FILE` can explicitly override the state-file path for testing or non-standard packaging.

`connection: "desktop"` must not be combined with `endpoint` or `bearerToken`. Discovery still rejects non-loopback server URLs.

## Manual input compatibility

The original explicit form remains supported:

```json
{
  "endpoint": "http://127.0.0.1:58208/mcp",
  "bearerToken": "<secret>",
  "operations": [
    {
      "tool": "<mcp-tool-name>",
      "arguments": {}
    }
  ],
  "timeoutMs": 120000
}
```

Manual fields:

- `endpoint` — the current loopback MCP URL shown by WebCodex Desktop;
- `bearerToken` — the current Desktop local MCP Bearer token;
- `operations` — 1 to 128 MCP tool calls, executed in array order;
- `timeoutMs` — optional per-request HTTP timeout. Default: `120000`; allowed range: `1000..=300000`.

Unknown fields are rejected so automation mistakes fail closed.

## Carrying values between operations

Each operation can capture values from its MCP result with RFC 6901 JSON Pointers:

```json
{
  "connection": "desktop",
  "operations": [
    {
      "tool": "first_tool",
      "arguments": {},
      "capture": {
        "jobId": "/structuredContent/job/id",
        "revision": "/structuredContent/revision"
      }
    },
    {
      "tool": "second_tool",
      "arguments": {
        "job_id": { "$ref": "jobId" },
        "label": "revision=${revision}"
      }
    }
  ]
}
```

`{"$ref":"name"}` performs typed substitution, so objects, arrays, numbers, booleans, strings, and null retain their JSON type. `${name}` interpolates a captured scalar into a string. Missing captures, unknown references, invalid pointers, and attempts to interpolate non-scalar values fail before later operations run.

Captured values are internal bridge variables and are not emitted as a separate secret-bearing variable table.

## Bounded await / Job polling

An operation can poll another MCP tool after its initial result. This is useful for WebCodex commands that return a Job identity and require `observe_jobs` or another observation tool before the final result is known.

The bridge intentionally keeps polling generic instead of hard-coding one historical Job schema. Use the current tool schema and result paths returned by the connected WebCodex runtime.

```json
{
  "connection": "desktop",
  "operations": [
    {
      "tool": "start_long_operation",
      "arguments": {},
      "capture": {
        "jobId": "/structuredContent/job/id"
      },
      "await": {
        "tool": "observe_jobs",
        "arguments": {
          "job_id": { "$ref": "jobId" }
        },
        "intervalMs": 1000,
        "timeoutMs": 180000,
        "until": {
          "pointer": "/structuredContent/job/status",
          "anyOf": ["completed", "failed", "cancelled"]
        },
        "success": {
          "pointer": "/structuredContent/job/status",
          "equals": "completed"
        },
        "capture": {
          "finalRevision": "/structuredContent/revision"
        }
      }
    },
    {
      "tool": "show_changes",
      "arguments": {
        "revision": { "$ref": "finalRevision" }
      }
    }
  ]
}
```

`await.until` ends polling when it matches. `await.success` is optional; when present, a terminal observation that does not match it fails the bridge and prevents later calls. Predicates support exactly one of `equals` or `anyOf` and resolve their `pointer` against the poll tool result.

`intervalMs` defaults to `1000` and must be `100..=60000`. `await.timeoutMs` defaults to `120000` and must be `1000..=300000`. Polling uses the same authenticated MCP session as the parent operation.

## Windows PowerShell example

With Desktop discovery, PowerShell no longer needs to copy the current MCP port or save the Bearer token separately:

```powershell
$job = @{
  connection = "desktop"
  operations = @(
    @{ tool = "list_jobs"; arguments = @{} }
  )
  timeoutMs = 120000
} | ConvertTo-Json -Depth 30 -Compress

$job | webcodex mcp-bridge
```

For older automation, the explicit `endpoint` + `bearerToken` stdin form remains valid.

## Output contract

Successful execution returns exit code `0` and one JSON object:

```json
{
  "ok": true,
  "connection": "desktop",
  "protocolVersion": "2025-06-18",
  "completed": 1,
  "results": [
    {
      "index": 0,
      "tool": "list_jobs",
      "ok": true,
      "result": {
        "content": [],
        "structuredContent": {}
      }
    }
  ]
}
```

`result` is the initial MCP `tools/call` result. When `await` is used, the same operation result also includes `awaitResult` and `awaitPolls` after the completion predicate matches. For machine-readable WebCodex fields, prefer `structuredContent` as described in [`MCP.md`](MCP.md).

Failures still write one JSON object to stdout. Input/configuration failures use exit code `2`; transport, protocol, reference, await, and tool failures use exit code `1`. The `completed` count and `results` array contain calls that produced an MCP tool result. When a tool returns `isError: true`, that result is preserved, the top-level `ok` becomes `false`, and later operations are not executed.

## MCP lifecycle

For each invocation the bridge performs:

1. resolve the manual connection or discover the current Desktop loopback connection;
2. `initialize` using Streamable HTTP MCP;
3. `notifications/initialized`;
4. each requested `tools/call`, sequentially, reusing captures and the returned MCP session id;
5. bounded `await` polling when requested;
6. a best-effort HTTP `DELETE` for the MCP session when the job finishes or stops.

Both JSON and `text/event-stream` MCP responses are accepted. The negotiated protocol version returned by the Server is used for subsequent requests.
