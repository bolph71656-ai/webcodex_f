import { useEffect, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { desktopApi, type LocalMcpHandoff } from "../../lib/desktop-api";
import type { DesktopError, DesktopState } from "../../models/topology";
import { useLocale } from "../../i18n/locale";
import {
  desktopErrorPresentation,
  normalizeDesktopError,
  runtimeLabel,
} from "../../i18n/presentation";
import { TunnelConfigDiagnostics } from "./TunnelConfigDiagnostics";

type RegularProvider = "local" | "openai";
type LocalCopyStatus = "idle" | "url" | "credential" | "failed";

const LOCAL_MCP_LABELS = {
  "ja-JP": {
    title: "ローカル MCP 接続",
    description: "このコンピューター上の MCP クライアントから、トンネルを使わずに WebCodex へ接続できます。接続先は 127.0.0.1 / localhost に限定され、Bearer 認証は維持されます。",
    loading: "ローカル MCP の接続情報を確認中…",
    copyUrl: "MCP URL をコピー",
    copyCredential: "Bearer トークンをコピー",
    copiedUrl: "MCP URL をコピーしました。",
    copiedCredential: "Bearer トークンをコピーしました。機密情報として扱ってください。",
    copyFailed: "コピーできませんでした。再試行してください。",
    unavailable: "ローカル MCP の接続情報を取得できません。実行環境の状態を確認してください。",
    localOnly: "ローカル専用",
    auth: "認証",
  },
  "zh-CN": {
    title: "本地 MCP 连接",
    description: "此计算机上的 MCP 客户端无需隧道即可连接 WebCodex。端点仅限 127.0.0.1 / localhost，并继续使用 Bearer 身份验证。",
    loading: "正在检查本地 MCP 连接信息…",
    copyUrl: "复制 MCP URL",
    copyCredential: "复制 Bearer 令牌",
    copiedUrl: "已复制 MCP URL。",
    copiedCredential: "已复制 Bearer 令牌。请将其视为机密信息。",
    copyFailed: "复制失败。请重试。",
    unavailable: "无法获取本地 MCP 连接信息。请检查运行环境状态。",
    localOnly: "仅本地",
    auth: "身份验证",
  },
  "en-US": {
    title: "Local MCP connection",
    description: "MCP clients on this computer can connect to WebCodex without a tunnel. The endpoint stays limited to 127.0.0.1 / localhost and Bearer authentication remains required.",
    loading: "Checking local MCP connection information…",
    copyUrl: "Copy MCP URL",
    copyCredential: "Copy Bearer token",
    copiedUrl: "MCP URL copied.",
    copiedCredential: "Bearer token copied. Treat it as a secret.",
    copyFailed: "Could not copy. Try again.",
    unavailable: "Local MCP connection information is unavailable. Check runtime status.",
    localOnly: "Local only",
    auth: "Authentication",
  },
  "ko-KR": {
    title: "로컬 MCP 연결",
    description: "이 컴퓨터의 MCP 클라이언트는 터널 없이 WebCodex에 연결할 수 있습니다. 엔드포인트는 127.0.0.1 / localhost로 제한되며 Bearer 인증을 계속 사용합니다.",
    loading: "로컬 MCP 연결 정보를 확인하는 중…",
    copyUrl: "MCP URL 복사",
    copyCredential: "Bearer 토큰 복사",
    copiedUrl: "MCP URL을 복사했습니다.",
    copiedCredential: "Bearer 토큰을 복사했습니다. 비밀 정보로 취급하세요.",
    copyFailed: "복사하지 못했습니다. 다시 시도하세요.",
    unavailable: "로컬 MCP 연결 정보를 가져올 수 없습니다. 런타임 상태를 확인하세요.",
    localOnly: "로컬 전용",
    auth: "인증",
  },
  "de-DE": {
    title: "Lokale MCP-Verbindung",
    description: "MCP-Clients auf diesem Computer können ohne Tunnel eine Verbindung zu WebCodex herstellen. Der Endpunkt bleibt auf 127.0.0.1 / localhost beschränkt und Bearer-Authentifizierung bleibt erforderlich.",
    loading: "Lokale MCP-Verbindungsdaten werden geprüft…",
    copyUrl: "MCP-URL kopieren",
    copyCredential: "Bearer-Token kopieren",
    copiedUrl: "MCP-URL kopiert.",
    copiedCredential: "Bearer-Token kopiert. Als Geheimnis behandeln.",
    copyFailed: "Kopieren fehlgeschlagen. Erneut versuchen.",
    unavailable: "Lokale MCP-Verbindungsdaten sind nicht verfügbar. Laufzeitstatus prüfen.",
    localOnly: "Nur lokal",
    auth: "Authentifizierung",
  },
  "fr-FR": {
    title: "Connexion MCP locale",
    description: "Les clients MCP de cet ordinateur peuvent se connecter à WebCodex sans tunnel. Le point de terminaison reste limité à 127.0.0.1 / localhost et l’authentification Bearer reste obligatoire.",
    loading: "Vérification des informations de connexion MCP locale…",
    copyUrl: "Copier l’URL MCP",
    copyCredential: "Copier le jeton Bearer",
    copiedUrl: "URL MCP copiée.",
    copiedCredential: "Jeton Bearer copié. Traitez-le comme un secret.",
    copyFailed: "Échec de la copie. Réessayez.",
    unavailable: "Les informations de connexion MCP locale sont indisponibles. Vérifiez l’état de l’environnement.",
    localOnly: "Local uniquement",
    auth: "Authentification",
  },
} as const;

export function ConnectionPanel({
  state,
  onState,
}: {
  state: DesktopState;
  onState: (state: DesktopState) => void;
}) {
  const { t, locale } = useLocale();
  const localLabels = LOCAL_MCP_LABELS[locale];
  const [provider, setProvider] = useState<RegularProvider>(
    state.regular_tunnel || state.preferred_connection === "open_ai_tunnel" ? "openai" : "local",
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<DesktopError | null>(null);
  const [copyStatus, setCopyStatus] = useState<"idle" | "copied" | "failed">("idle");
  const [localHandoff, setLocalHandoff] = useState<LocalMcpHandoff | null>(null);
  const [localHandoffError, setLocalHandoffError] = useState<DesktopError | null>(null);
  const [localHandoffLoading, setLocalHandoffLoading] = useState(false);
  const [localCopyStatus, setLocalCopyStatus] = useState<LocalCopyStatus>("idle");
  const tunnelId = state.openai_tunnel_config.effective_tunnel_id ?? state.openai_tunnel_config.saved_tunnel_id;

  useEffect(() => {
    if (copyStatus !== "copied") return;
    const timer = window.setTimeout(() => setCopyStatus("idle"), 2500);
    return () => window.clearTimeout(timer);
  }, [copyStatus]);
  useEffect(() => setCopyStatus("idle"), [tunnelId]);
  useEffect(() => {
    if (localCopyStatus === "idle") return;
    const timer = window.setTimeout(() => setLocalCopyStatus("idle"), 3000);
    return () => window.clearTimeout(timer);
  }, [localCopyStatus]);
  useEffect(() => {
    let cancelled = false;
    if (provider !== "local" || !state.readiness.runtime_ready || Boolean(state.regular_tunnel)) {
      setLocalHandoff(null);
      setLocalHandoffError(null);
      setLocalHandoffLoading(false);
      return () => { cancelled = true; };
    }
    if (typeof desktopApi.getLocalMcpHandoff !== "function") {
      setLocalHandoff(null);
      setLocalHandoffError(null);
      setLocalHandoffLoading(false);
      return () => { cancelled = true; };
    }
    setLocalHandoffLoading(true);
    setLocalHandoffError(null);
    void desktopApi.getLocalMcpHandoff()
      .then((handoff) => {
        if (!cancelled) setLocalHandoff(handoff);
      })
      .catch((value) => {
        if (!cancelled) {
          setLocalHandoff(null);
          setLocalHandoffError(normalizeDesktopError(value));
        }
      })
      .finally(() => {
        if (!cancelled) setLocalHandoffLoading(false);
      });
    return () => { cancelled = true; };
  }, [provider, state.readiness.runtime_ready, state.project?.runtime_project_id, Boolean(state.regular_tunnel)]);

  const copyTunnelId = async () => {
    if (!tunnelId) return;
    try { await writeText(tunnelId); setCopyStatus("copied"); }
    catch { setCopyStatus("failed"); }
  };
  const copyLocalMcpUrl = async () => {
    if (!localHandoff) return;
    try {
      await writeText(localHandoff.mcpUrl);
      setLocalCopyStatus("url");
    } catch {
      setLocalCopyStatus("failed");
    }
  };
  const copyLocalMcpCredential = async () => {
    if (!localHandoff?.credentialAvailable) return;
    try {
      const credential = await desktopApi.getLocalMcpCredential();
      await writeText(credential);
      setLocalCopyStatus("credential");
    } catch {
      setLocalCopyStatus("failed");
    }
  };
  const topology = state.topology;
  const mutationBusy = busy || Boolean(state.current_operation);

  const run = async (operation: () => Promise<DesktopState>) => {
    if (mutationBusy) return;
    setBusy(true);
    setError(null);
    try {
      onState(await operation());
    } catch (value) {
      setError(normalizeDesktopError(value));
    } finally {
      setBusy(false);
    }
  };

  const chooseProvider = (value: RegularProvider) => {
    setProvider(value);
  };

  if (topology?.server.kind === "remote") {
    return (
      <section className="page-section" aria-labelledby="connection-title" data-webcodex-page="connection">
        <PageHeading />
        <article className="detail-card" aria-labelledby="remote-server-title">
          <h2 id="remote-server-title">{t("connection.remoteServer")}</h2>
          <strong>{topology.server.url}</strong>
          <dl className="detail-list">
            <div><dt>{t("home.runner")}</dt><dd>{t("connection.runnerThisComputer")}</dd></div>
            <div><dt>{t("connection.methods")}</dt><dd>{t("connection.externalManagedRemote")}</dd></div>
          </dl>
        </article>
      </section>
    );
  }

  if (topology?.experience === "quick_share") {
    return (
      <section className="page-section" aria-labelledby="connection-title" data-webcodex-page="connection">
        <PageHeading />
        <article className="detail-card">
          <span className="section-kicker">{t("activity.source.quick_share")}</span>
          <strong>{currentConnection(state, t)}</strong>
          <p>{t("connection.quickShareManaged")}</p>
        </article>
      </section>
    );
  }

  const tunnelEstablished = state.regular_tunnel?.status === "ready";
  const tunnelLocallyReady = tunnelEstablished && Boolean(state.regular_tunnel?.ready_for_chatgpt);
  const tunnelError = state.regular_tunnel?.status === "error";
  const localModeReady = provider === "local" && state.readiness.runtime_ready && !state.regular_tunnel;
  const chatgptObserved = state.readiness.runtime_ready &&
    Boolean(state.chatgpt_activity?.observed) &&
    !tunnelError;
  const canStart = state.readiness.runtime_ready && state.openai_tunnel_configured && provider === "openai";

  return (
    <section
      className="page-section"
      aria-labelledby="connection-title"
      aria-busy={mutationBusy}
      data-webcodex-page="connection"
    >
      <PageHeading />

      <article className="connection-current detail-card" aria-labelledby="connection-current-title">
        <h2 id="connection-current-title" className="section-title">{t("connection.current")}</h2>
        <div className="status-value">
          <i className={`status-dot ${chatgptObserved ? "ready" : localModeReady ? "ready" : tunnelLocallyReady ? "ready" : tunnelError ? "error" : state.regular_tunnel ? "pending" : "unknown"}`} aria-hidden="true" />
          <strong>{!state.readiness.runtime_ready ? runtimeLabel(state, t) : chatgptObserved ? t("connection.observed") : localModeReady ? t("connection.noDesktopTunnel") : tunnelLocallyReady ? t("connection.tunnelReady") : currentConnection(state, t)}</strong>
        </div>
        <p>{!state.readiness.runtime_ready ? t("workspace.afterStart") : chatgptObserved ? t("connection.observedDescription") : localModeReady ? t("connection.localDescription") : tunnelLocallyReady ? t("connection.waitingForChatGpt") : tunnelEstablished ? t("connection.tunnelHandoffNeedsAction") : t("connection.notVerified")}</p>
      </article>

      {error && <LocalizedError error={error} />}

      {state.regular_tunnel ? (
        <article className="handoff-card" aria-label={t("activity.source.regular_tunnel")}>
          <div>
            <span className="section-kicker">{t("activity.source.regular_tunnel")}</span>
            <span>{tunnelError ? t("workspace.stopToRetry") : !tunnelEstablished ? t("connection.tunnelStarting") : null}</span>
          </div>
          <button
            className="danger-button"
            disabled={mutationBusy}
            onClick={() => void run(desktopApi.stopRegularTunnel)}
            data-webcodex-action="stop-regular-tunnel"
          >
            {mutationBusy ? t("common.checking") : t("connection.stopTunnel")}
          </button>
        </article>
      ) : (
        <div className="connection-form">
          <fieldset className="provider-row provider-fieldset" role="radiogroup" aria-labelledby="regular-provider-legend">
            <legend id="regular-provider-legend">{t("connection.methods")}</legend>
            <ProviderOption
              id="regular-provider-local"
              value="local"
              checked={provider === "local"}
              onChange={chooseProvider}
              title={t("connection.noDesktopTunnel")}
              description={t("connection.localDescription")}
              disabled={mutationBusy}
            />
            <ProviderOption
              id="regular-provider-openai"
              value="openai"
              checked={provider === "openai"}
              onChange={chooseProvider}
              title={t("activity.source.regular_tunnel")}
              description={state.openai_tunnel_configured ? t("connection.openaiDescription") : t("connection.openaiNotConfigured")}
              disabled={mutationBusy || !state.openai_tunnel_configured}
            />
          </fieldset>

          {!state.readiness.runtime_ready && <p className="inline-note">{t("connection.runtimeRequired")}</p>}

          {provider === "openai" && (
            <button className="primary-button" disabled={mutationBusy || !canStart} onClick={() => void run(desktopApi.startRegularTunnel)} data-webcodex-action="start-regular-tunnel">
              {mutationBusy ? t("connection.tunnelStarting") : t("home.connectChatGpt")}
            </button>
          )}
        </div>
      )}

      {!state.regular_tunnel && provider === "local" && state.readiness.runtime_ready && (
        <article className="detail-card local-mcp-handoff" aria-labelledby="local-mcp-title">
          <span className="section-kicker">MCP · 127.0.0.1</span>
          <h2 id="local-mcp-title">{localLabels.title}</h2>
          <p>{localLabels.description}</p>
          {localHandoffLoading && <p className="inline-note">{localLabels.loading}</p>}
          {localHandoffError && <LocalizedError error={localHandoffError} />}
          {!localHandoffLoading && !localHandoffError && !localHandoff && (
            <p className="inline-note">{localLabels.unavailable}</p>
          )}
          {localHandoff && (
            <>
              <label htmlFor="local-mcp-url">MCP URL</label>
              <input
                id="local-mcp-url"
                readOnly
                value={localHandoff.mcpUrl}
                onFocus={(event) => event.target.select()}
              />
              <div className="button-row">
                <button className="secondary-button" onClick={() => void copyLocalMcpUrl()}>
                  {localLabels.copyUrl}
                </button>
                <button
                  className="secondary-button"
                  disabled={!localHandoff.credentialAvailable}
                  onClick={() => void copyLocalMcpCredential()}
                >
                  {localLabels.copyCredential}
                </button>
              </div>
              <dl className="detail-list">
                <div><dt>{localLabels.auth}</dt><dd>Bearer token</dd></div>
                <div><dt>{localLabels.localOnly}</dt><dd>{localHandoff.loopbackOnly ? "127.0.0.1 / localhost" : "—"}</dd></div>
              </dl>
              <span role="status">
                {localCopyStatus === "url" ? localLabels.copiedUrl : localCopyStatus === "credential" ? localLabels.copiedCredential : localCopyStatus === "failed" ? localLabels.copyFailed : ""}
              </span>
            </>
          )}
        </article>
      )}

      {provider === "openai" && tunnelId && (
        <article className="detail-card tunnel-copy">
          <label htmlFor="active-tunnel-id">{t("tunnelConfig.tunnelId")}</label>
          <input id="active-tunnel-id" readOnly value={tunnelId} onFocus={(event) => event.target.select()} />
          <button className="secondary-button" onClick={() => void copyTunnelId()}>{t("connection.copyTunnelId")}</button>
          <span role="status">{copyStatus === "copied" ? t("connection.clipboardReady") : copyStatus === "failed" ? t("connection.copyFailed") : ""}</span>
        </article>
      )}
      {provider === "openai" && (
        <article className="connection-instructions detail-card">
          <h2>{t("workspace.handoffTitle")}</h2>
          <ol>
            <li>{t("workspace.handoffOne")}</li>
            <li>{t("workspace.handoffTwo")}</li>
            <li>{t("workspace.verifyHint")}</li>
          </ol>
        </article>
      )}
      <details className="setup-tunnel-details" open={!state.openai_tunnel_configured}>
        <summary>{t("workspace.optionalTunnel")}</summary>
        <p>{t("connection.description")}</p>
        <TunnelConfigDiagnostics state={state} onState={onState} />
      </details>
    </section>
  );
}

function PageHeading() {
  const { t } = useLocale();
  return (
    <>
      <div className="eyebrow">{t("connection.eyebrow")}</div>
      <h1 id="connection-title">{t("connection.title")}</h1>
    </>
  );
}

function ProviderOption({
  id,
  value,
  checked,
  onChange,
  title,
  description,
  disabled,
}: {
  id: string;
  value: RegularProvider;
  checked: boolean;
  onChange: (value: RegularProvider) => void;
  title: string;
  description: string;
  disabled: boolean;
}) {
  const descriptionId = `${id}-description`;
  return (
    <div className={`provider-option ${checked ? "selected" : ""}`}>
      <input
        id={id}
        type="radio"
        name="regular-connection-provider"
        value={value}
        checked={checked}
        onChange={() => onChange(value)}
        aria-describedby={descriptionId}
        disabled={disabled}
      />
      <label htmlFor={id}>
        <strong>{title}</strong>
        <span id={descriptionId}>{description}</span>
      </label>
    </div>
  );
}

function LocalizedError({ error }: { error: DesktopError }) {
  const { t } = useLocale();
  const presentation = desktopErrorPresentation(error, t);
  return (
    <div className="error-card" role="alert">
      <strong>{presentation.title}</strong>
      <span>{presentation.action}</span>
      <details>
        <summary>{t("common.details")}</summary>
        <code>{error.code}</code>
        <p>{error.message}</p>
      </details>
    </div>
  );
}

function currentConnection(state: DesktopState, t: ReturnType<typeof useLocale>["t"]) {
  if (state.regular_tunnel) return t("activity.source.regular_tunnel");
  const exposure = state.topology?.exposure;
  if (!exposure || exposure.kind === "none") return t("connection.noDesktopTunnel");
  if (exposure.kind === "existing_https") return `HTTPS · ${exposure.url}`;
  if (exposure.kind === "cloudflare") return `Cloudflare · ${t("activity.source.quick_share")}`;
  return t("activity.source.regular_tunnel");
}
