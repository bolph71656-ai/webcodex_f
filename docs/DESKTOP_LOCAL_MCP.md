# WebCodex Desktop: local MCP without a tunnel

WebCodex Desktop can expose the Desktop-owned regular Server to an MCP client running on the **same computer** without Cloudflare or OpenAI Secure Tunnel.

This mode is intentionally local-only:

- the Desktop-owned Server remains bound to loopback (`127.0.0.1` / `localhost`);
- Bearer authentication remains required;
- Desktop does not create a public endpoint, reverse proxy, or tunnel;
- the Bearer token is treated as a secret and is only retrieved by the UI when the user explicitly asks to copy it.

This is useful for desktop MCP hosts, automation tools, or other local AI clients that can connect to a Streamable HTTP MCP endpoint. It does **not** make `127.0.0.1` reachable from a hosted web client. A client running outside this computer still needs a trusted reachable network path.

## Setup

1. Install and open WebCodex Desktop.
2. Choose **Use WebCodex on this computer** and select the real project directory.
3. Wait until Service, Runner, and Project are ready.
4. Open **Connection**.
5. Select **Local only** / **ローカルのみ**.
6. Desktop displays the local `MCP URL` and offers two explicit copy actions:
   - **Copy MCP URL**;
   - **Copy Bearer token**.
7. Configure the local MCP client with the displayed URL and Bearer token.

The URL is expected to look like:

```text
http://127.0.0.1:<dynamic-port>/mcp
```

The port is allocated by Desktop. Do not hard-code it; use the value shown in the Connection page for the current Desktop-managed runtime.

## Security properties

Desktop refuses to produce local MCP handoff data unless all of the following are true:

- the selected topology uses the Desktop-owned local Server;
- Service, Runner, and Project are all ready;
- the saved Server URL uses plain HTTP on a loopback host;
- the saved user credential file exists.

A saved `https://` endpoint, LAN address such as `192.168.x.x`, or other non-loopback host is rejected by the local-only handoff path. This prevents the local-only control from silently becoming a network exposure feature.

The Bearer token grants the authority associated with the enrolled WebCodex user and configured project/Runner policy. Do not paste it into Git, issue trackers, logs, or shared chat messages. If it is exposed, replace the affected WebCodex credential.

## Relationship to tunnels

Tunnel and local MCP modes use the same regular WebCodex Server + Runner runtime. A tunnel changes **reachability**, not WebCodex execution permissions.

- **Local only:** local MCP client → loopback WebCodex Server → Runner → project.
- **OpenAI Secure Tunnel:** remote supported client → Secure Tunnel → local WebCodex Server → Runner → project.

Selecting local-only mode does not disable Server authentication and does not broaden project filesystem authority.
