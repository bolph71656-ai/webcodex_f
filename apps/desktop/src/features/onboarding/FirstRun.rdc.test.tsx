import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DesktopState, ProjectSelection } from "../../models/topology";
import { LocaleProvider } from "../../i18n/locale";

const api = vi.hoisted(() => ({
  inspectProject: vi.fn(),
  configureLocal: vi.fn(),
  getLocalMcpHandoff: vi.fn(),
  startRegularTunnel: vi.fn(),
  configureRemote: vi.fn(),
  startQuickShare: vi.fn(),
  openPowerShellInstallGuide: vi.fn(),
  refresh: vi.fn(),
}));

const dialog = vi.hoisted(() => ({ open: vi.fn() }));

vi.mock("../../lib/desktop-api", () => ({
  desktopApi: api,
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: dialog.open,
}));

import { FirstRun } from "./FirstRun";

const project: ProjectSelection = {
  path: "C:\\fixture\\repo",
  allowed_root: "C:\\fixture",
  is_git_repository: true,
  runtime_project_id: "agent:desktop:repo",
};

const initialState = {
  readiness: {
    server: "unknown",
    runner: "unknown",
    exposure: "unknown",
    project: "none",
    runtime_ready: false,
    ready_for_chatgpt: false,
    summary: "WebCodex Service needs attention",
    summary_kind: "service_needs_attention",
    next_action: "Start or reconnect the WebCodex Service.",
    next_action_kind: "start_or_reconnect_service",
  },
  activity_sequence: 0,
  openai_tunnel_configured: false,
  openai_tunnel_config: {
    tunnel_id_present: false,
    api_key_present: false,
    source: "invalid",
    saved_tunnel_id: null,
  },
  regular_tunnel_available: true,
  runtime_autostart: false,
  preferred_connection: "no_chat_gpt",
  tunnel_proxy: {
    mode: "auto",
    custom_url: null,
    effective_source: "direct",
    effective_url: null,
    detected_url: null,
  },
} as DesktopState;

const readyState = {
  ...initialState,
  topology: {
    experience: "full",
    server: { kind: "local" },
    runner: { kind: "local" },
    exposure: { kind: "none" },
    enrollment: { kind: "managed_pairing" },
  },
  readiness: {
    server: "ready",
    runner: "ready",
    exposure: "local_ready",
    project: "ready",
    runtime_ready: true,
    ready_for_chatgpt: false,
    summary: "Runtime ready on this computer",
    summary_kind: "runtime_ready_local_only",
    next_action: "Choose a ChatGPT connection in Connection.",
    next_action_kind: "choose_connection",
  },
  project,
  regular_tunnel: null,
} as DesktopState;

function renderSetup(onState = vi.fn(), onComplete = vi.fn()) {
  render(
    <LocaleProvider>
      <FirstRun
        state={initialState}
        onState={onState}
        chooseModeFirst
        onComplete={onComplete}
      />
    </LocaleProvider>,
  );
  return { onState, onComplete };
}

describe("ChatGPT Plus + RDC automatic setup", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.openPowerShellInstallGuide.mockResolvedValue(undefined);
    api.refresh.mockResolvedValue(initialState);
    api.inspectProject.mockResolvedValue(project);
    api.configureLocal.mockResolvedValue(readyState);
    api.getLocalMcpHandoff.mockResolvedValue({
      mcpUrl: "http://127.0.0.1:58208/mcp",
      authentication: "bearer",
      loopbackOnly: true,
      credentialAvailable: true,
    });
  });

  it("configures the local runtime and verifies Bridge auto-discovery without exposing a token", async () => {
    const { onState, onComplete } = renderSetup();
    const entry = document.querySelector<HTMLButtonElement>('[data-webcodex-action="choose-rdc-setup"]');
    expect(entry).not.toBeNull();
    fireEvent.click(entry!);

    dialog.open.mockResolvedValue(project.path);
    fireEvent.click(document.querySelector<HTMLButtonElement>('[data-webcodex-action="choose-project"]')!);
    await waitFor(() => expect(api.inspectProject).toHaveBeenCalledWith(project.path));

    const setup = document.querySelector<HTMLButtonElement>('[data-webcodex-action="configure-rdc-auto"]');
    expect(setup).toBeEnabled();
    fireEvent.click(setup!);

    await waitFor(() => expect(api.configureLocal).toHaveBeenCalledWith(project.path));
    await waitFor(() => expect(api.getLocalMcpHandoff).toHaveBeenCalledOnce());
    expect(onState).toHaveBeenCalledWith(readyState);
    expect(onComplete).not.toHaveBeenCalled();

    const status = document.querySelector('[data-webcodex-rdc-ready="true"]');
    expect(status).not.toBeNull();
    expect(status).toHaveTextContent("Service");
    expect(status).toHaveTextContent("Runner");
    expect(status).toHaveTextContent("Local MCP");
    expect(document.body.textContent).not.toContain("58208");
    expect(document.body.textContent).not.toContain("Bearer token copied");

    fireEvent.click(document.querySelector<HTMLButtonElement>('[data-webcodex-action="complete-rdc-setup"]')!);
    expect(onComplete).toHaveBeenCalledOnce();
  });

  it("fails closed when the local MCP handoff is unavailable", async () => {
    api.getLocalMcpHandoff.mockRejectedValue({
      code: "local_mcp_unavailable",
      message: "Local MCP connection information is unavailable",
      next_action: "Retry automatic setup.",
    });
    renderSetup();
    fireEvent.click(document.querySelector<HTMLButtonElement>('[data-webcodex-action="choose-rdc-setup"]')!);
    dialog.open.mockResolvedValue(project.path);
    fireEvent.click(document.querySelector<HTMLButtonElement>('[data-webcodex-action="choose-project"]')!);
    await waitFor(() => expect(api.inspectProject).toHaveBeenCalledOnce());
    fireEvent.click(document.querySelector<HTMLButtonElement>('[data-webcodex-action="configure-rdc-auto"]')!);

    expect(await screen.findByRole("alert")).toHaveTextContent("local_mcp_unavailable");
    expect(document.querySelector('[data-webcodex-rdc-ready="true"]')).toBeNull();
  });
});
