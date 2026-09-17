# WebCodex local MCP JSON Bridge

`webcodex mcp-bridge` is a small local automation adapter for WebCodex Desktop. It reads one JSON job from standard input, opens one authenticated Streamable HTTP MCP session to the Desktop-owned loopback endpoint, runs the requested `tools/call` operations **in order**, and writes one machine-readable JSON result to standard output.

It is intended for a local agent or automation process that needs one deterministic process invocation instead of implementing MCP session handling itself.

## Security boundary

The bridge deliberately keeps the same local-only boundary as WebCodex Desktop:

- the endpoint must use plain HTTP on `127.0.0.1`, `::1`, or `localhost`;
- the endpoint path must be `/mcp`;
- URL credentials, query strings, and fragments are rejected;
- Bearer authentication remains mandatory;
- the Bearer token is accepted only through stdin JSON, not command-line arguments;
- the bridge disables proxy discovery for the local MCP connection;
- the Bearer token is redacted from bridge-generated error/result strings before output;
- execution stops after the first transport, JSON-RPC, or MCP tool error, so later effectful calls are not silently attempted after an uncertain result.

The bridge does not widen WebCodex project, Runner, tool, or scope permissions. Every call still passes through the ordinary WebCodex MCP/runtime authorization path.

## Input contract

```json
{
  "endpoint": "http://127.0.0.1:58208/mcp",
  "bearerToken": "<secret>",
  "operations": [
    {
      "tool": "<mcp-tool-name>",
      "arguments": {}
    },
    {
      "tool": "<next-mcp-tool-name>",
      "arguments": {
        "example": "value"
      }
    }
  ],
  "timeoutMs": 120000
}
```

Fields:

- `endpoint` — the current MCP URL shown by WebCodex Desktop. The Desktop port is dynamic; do not hard-code it.
- `bearerToken` — the current Desktop local MCP Bearer token.
- `operations` — 1 to 128 MCP tool calls, executed in array order. `arguments` defaults to `{}` and must be a JSON object.
- `timeoutMs` — optional per-request HTTP timeout. Default: `120000`; allowed range: `1000..=300000`.

Unknown fields are rejected so automation mistakes fail closed.

## Windows PowerShell example

Keep the token in a private local file rather than putting it in the `webcodex` command line:

```powershell
$endpoint = "http://127.0.0.1:58208/mcp"
$token = (Get-Content -Raw "$env:USERPROFILE\.webcodex-local-mcp-token").Trim()

$job = @{
  endpoint = $endpoint
  bearerToken = $token
  operations = @(
    @{ tool = "list_jobs"; arguments = @{} }
  )
  timeoutMs = 120000
} | ConvertTo-Json -Depth 20 -Compress

$job | webcodex mcp-bridge
```

Use the actual MCP URL and token from Desktop. The token-file path above is only an example; the bridge does not create or manage that file.

## Output contract

Successful execution returns exit code `0` and one JSON object:

```json
{
  "ok": true,
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

`result` is the MCP `tools/call` result. For machine-readable WebCodex fields, prefer `structuredContent` as described in [`MCP.md`](MCP.md).

Failures still write one JSON object to stdout. Input/configuration failures use exit code `2`; transport, protocol, and tool failures use exit code `1`. The `completed` count and `results` array contain only calls that produced an MCP tool result. When a tool returns `isError: true`, that result is preserved, the top-level `ok` becomes `false`, and later operations are not executed.

Example failure shape:

```json
{
  "ok": false,
  "protocolVersion": "2025-06-18",
  "completed": 0,
  "results": [],
  "error": {
    "kind": "json_rpc",
    "message": "tool not found",
    "operationIndex": 0,
    "tool": "missing_tool",
    "rpcCode": -32601
  }
}
```

## MCP lifecycle

For each invocation the bridge performs:

1. `initialize` using Streamable HTTP MCP;
2. `notifications/initialized`;
3. each requested `tools/call`, sequentially, reusing the returned MCP session id when the Server assigns one;
4. a best-effort HTTP `DELETE` for the MCP session when the job finishes or stops.

Both JSON and `text/event-stream` MCP responses are accepted. The negotiated protocol version returned by the Server is used for subsequent requests.