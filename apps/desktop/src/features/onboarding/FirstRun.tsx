import { useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { desktopApi, type QuickShareProvider } from "../../lib/desktop-api";
import { useLocale } from "../../i18n/locale";
import { TunnelConfigDiagnostics } from "../connection/TunnelConfigDiagnostics";
import { PowerShellInstallGuidance } from "../settings/PowerShellInstallGuidance";
import {
  desktopErrorPresentation,
  normalizeDesktopError,
} from "../../i18n/presentation";
import type {
  DesktopError,
  DesktopState,
  ProjectSelection,
} from "../../models/topology";

type SetupMode = "local" | "remote" | "share" | "rdc";
type RdcVerification = "idle" | "checking" | "ready";

const RDC_SETUP_LABELS = {
  "ja-JP": {
    badge: "ChatGPT Plus + RDC",
    entryTitle: "ChatGPT Plus + RDC を自動セットアップ",
    entryDescription: "Tunnel や Codex を使わず、RDC からローカル WebCodex Bridge を使う構成を自動で準備します。",
    label: "ローカル自動接続",
    title: "ChatGPT Plus + RDC をセットアップ",
    description: "プロジェクトを選ぶだけで Service / Runner / Local MCP を起動し、Bridge が自動検出できる状態まで確認します。",
    privacy: "Tunnel ID、OpenAI API key、Bearer token の手動入力やコピーは不要です。Bearer token はこの画面に表示しません。",
    autoSetup: "自動セットアップ",
    retest: "接続テストを再実行",
    complete: "セットアップ完了",
    readyTitle: "WebCodex 側の準備が完了しました",
    readyDescription: "RDC から webcodex mcp-bridge を呼び出すと、Desktop のローカル MCP 接続情報を自動検出できます。",
    checking: "ローカル MCP と Bridge 自動検出を確認しています…",
    service: "Service",
    runner: "Runner",
    project: "Project",
    localMcp: "Local MCP",
    bridge: "Bridge 自動検出",
    ready: "Ready",
    pending: "未確認",
    noTerminal: "端末で URL や token を設定する必要はありません。",
  },
  "zh-CN": {
    badge: "ChatGPT Plus + RDC",
    entryTitle: "自动设置 ChatGPT Plus + RDC",
    entryDescription: "无需 Tunnel 或 Codex，自动准备 RDC 使用本地 WebCodex Bridge 的配置。",
    label: "本地自动连接",
    title: "设置 ChatGPT Plus + RDC",
    description: "只需选择项目，即可启动 Service / Runner / Local MCP，并验证 Bridge 自动发现。",
    privacy: "无需手动输入或复制 Tunnel ID、OpenAI API key 或 Bearer token。此页面不会显示 Bearer token。",
    autoSetup: "自动设置",
    retest: "重新运行连接测试",
    complete: "完成设置",
    readyTitle: "WebCodex 已准备就绪",
    readyDescription: "RDC 调用 webcodex mcp-bridge 时，可以自动发现 Desktop 的本地 MCP 连接信息。",
    checking: "正在验证本地 MCP 和 Bridge 自动发现…",
    service: "Service",
    runner: "Runner",
    project: "Project",
    localMcp: "Local MCP",
    bridge: "Bridge 自动发现",
    ready: "Ready",
    pending: "未验证",
    noTerminal: "无需在终端配置 URL 或 token。",
  },
  "en-US": {
    badge: "ChatGPT Plus + RDC",
    entryTitle: "Auto-setup ChatGPT Plus + RDC",
    entryDescription: "Prepare RDC to use the local WebCodex Bridge without Tunnel or Codex.",
    label: "Local automatic connection",
    title: "Set up ChatGPT Plus + RDC",
    description: "Choose a project and Desktop starts Service / Runner / Local MCP, then verifies Bridge auto-discovery.",
    privacy: "No manual Tunnel ID, OpenAI API key, or Bearer token entry/copy is required. The Bearer token is never shown here.",
    autoSetup: "Auto setup",
    retest: "Run connection test again",
    complete: "Finish setup",
    readyTitle: "WebCodex is ready",
    readyDescription: "When RDC invokes webcodex mcp-bridge, it can auto-discover the Desktop local MCP connection.",
    checking: "Checking Local MCP and Bridge auto-discovery…",
    service: "Service",
    runner: "Runner",
    project: "Project",
    localMcp: "Local MCP",
    bridge: "Bridge auto-discovery",
    ready: "Ready",
    pending: "Not verified",
    noTerminal: "No terminal URL or token setup is required.",
  },
  "ko-KR": {
    badge: "ChatGPT Plus + RDC",
    entryTitle: "ChatGPT Plus + RDC 자동 설정",
    entryDescription: "Tunnel이나 Codex 없이 RDC에서 로컬 WebCodex Bridge를 사용하도록 자동 준비합니다.",
    label: "로컬 자동 연결",
    title: "ChatGPT Plus + RDC 설정",
    description: "프로젝트만 선택하면 Service / Runner / Local MCP를 시작하고 Bridge 자동 검색까지 확인합니다.",
    privacy: "Tunnel ID, OpenAI API key, Bearer token을 수동으로 입력하거나 복사할 필요가 없습니다. Bearer token은 이 화면에 표시되지 않습니다.",
    autoSetup: "자동 설정",
    retest: "연결 테스트 다시 실행",
    complete: "설정 완료",
    readyTitle: "WebCodex 준비가 완료되었습니다",
    readyDescription: "RDC에서 webcodex mcp-bridge를 호출하면 Desktop의 로컬 MCP 연결 정보를 자동으로 찾습니다.",
    checking: "Local MCP와 Bridge 자동 검색을 확인하는 중…",
    service: "Service",
    runner: "Runner",
    project: "Project",
    localMcp: "Local MCP",
    bridge: "Bridge 자동 검색",
    ready: "Ready",
    pending: "확인 안 됨",
    noTerminal: "터미널에서 URL이나 token을 설정할 필요가 없습니다.",
  },
  "de-DE": {
    badge: "ChatGPT Plus + RDC",
    entryTitle: "ChatGPT Plus + RDC automatisch einrichten",
    entryDescription: "RDC für die lokale WebCodex Bridge ohne Tunnel oder Codex automatisch vorbereiten.",
    label: "Automatische lokale Verbindung",
    title: "ChatGPT Plus + RDC einrichten",
    description: "Projekt auswählen; Desktop startet Service / Runner / Local MCP und prüft die Bridge-Autoerkennung.",
    privacy: "Tunnel ID, OpenAI API key und Bearer token müssen nicht manuell eingegeben oder kopiert werden. Der Bearer token wird hier nicht angezeigt.",
    autoSetup: "Automatisch einrichten",
    retest: "Verbindung erneut testen",
    complete: "Einrichtung abschließen",
    readyTitle: "WebCodex ist bereit",
    readyDescription: "Wenn RDC webcodex mcp-bridge aufruft, werden die lokalen MCP-Verbindungsdaten von Desktop automatisch erkannt.",
    checking: "Local MCP und Bridge-Autoerkennung werden geprüft…",
    service: "Service",
    runner: "Runner",
    project: "Project",
    localMcp: "Local MCP",
    bridge: "Bridge-Autoerkennung",
    ready: "Ready",
    pending: "Nicht geprüft",
    noTerminal: "Keine URL- oder Token-Konfiguration im Terminal erforderlich.",
  },
  "fr-FR": {
    badge: "ChatGPT Plus + RDC",
    entryTitle: "Configurer automatiquement ChatGPT Plus + RDC",
    entryDescription: "Préparer RDC pour utiliser le Bridge WebCodex local sans Tunnel ni Codex.",
    label: "Connexion locale automatique",
    title: "Configurer ChatGPT Plus + RDC",
    description: "Choisissez un projet : Desktop démarre Service / Runner / Local MCP puis vérifie la détection automatique du Bridge.",
    privacy: "Aucune saisie ou copie manuelle de Tunnel ID, OpenAI API key ou Bearer token n'est nécessaire. Le Bearer token n'est jamais affiché ici.",
    autoSetup: "Configuration automatique",
    retest: "Relancer le test de connexion",
    complete: "Terminer la configuration",
    readyTitle: "WebCodex est prêt",
    readyDescription: "Lorsque RDC appelle webcodex mcp-bridge, les informations MCP locales de Desktop sont détectées automatiquement.",
    checking: "Vérification de Local MCP et de la détection automatique du Bridge…",
    service: "Service",
    runner: "Runner",
    project: "Project",
    localMcp: "Local MCP",
    bridge: "Détection auto du Bridge",
    ready: "Ready",
    pending: "Non vérifié",
    noTerminal: "Aucune configuration d'URL ou de token dans le terminal n'est nécessaire.",
  },
} as const;

type RdcLabels = (typeof RDC_SETUP_LABELS)[keyof typeof RDC_SETUP_LABELS];

interface FirstRunProps {
  state: DesktopState;
  onState: (state: DesktopState) => void;
  chooseModeFirst?: boolean;
  onComplete?: () => void;
}

export function FirstRun({ state, onState, chooseModeFirst = false, onComplete }: FirstRunProps) {
  const { t, locale } = useLocale();
  const rdcLabels = RDC_SETUP_LABELS[locale];
  const initialMode = useMemo<SetupMode | null>(() => {
    if (chooseModeFirst) return null;
    if (state.topology?.experience === "quick_share") return "share";
    if (state.topology?.server.kind === "local") return "local";
    if (state.topology?.server.kind === "remote") return "remote";
    return null;
  }, [chooseModeFirst, state.topology]);
  const [mode, setMode] = useState<SetupMode | null>(initialMode);
  const [project, setProject] = useState<ProjectSelection | null>(
    state.project ?? null,
  );
  const [serverUrl, setServerUrl] = useState(
    state.topology?.server.kind === "remote" ? state.topology.server.url : "",
  );
  const [pairingCode, setPairingCode] = useState("");
  const [remoteEnrollmentNeedsRefresh, setRemoteEnrollmentNeedsRefresh] = useState(false);
  const [provider, setProvider] = useState<QuickShareProvider>("cloudflare");
  const [connectAfterSetup, setConnectAfterSetup] = useState(state.openai_tunnel_configured);
  const [rdcVerification, setRdcVerification] = useState<RdcVerification>("idle");
  const [rdcSnapshot, setRdcSnapshot] = useState<DesktopState | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<DesktopError | null>(null);
  const mutationBusy = busy || Boolean(state.current_operation);
  const canReuseRemoteEnrollment = Boolean(
    mode === "remote" &&
      !remoteEnrollmentNeedsRefresh &&
      state.project?.runtime_project_id &&
      state.topology?.experience === "full" &&
      state.topology.server.kind === "remote" &&
      sameServerOrigin(serverUrl, state.topology.server.url),
  );

  const chooseProject = async () => {
    setError(null);
    try {
      const selection = await open({
        directory: true,
        multiple: false,
        title: t("setup.chooseProject"),
      });
      if (typeof selection !== "string") return;
      setProject(await desktopApi.inspectProject(selection));
      setRdcVerification("idle");
      setRdcSnapshot(null);
    } catch (value) {
      setError(normalizeDesktopError(value));
    }
  };

  const run = async () => {
    if (!mode || !project || mutationBusy) return;
    setBusy(true);
    setError(null);
    try {
      if (mode === "rdc") {
        setRdcVerification("checking");
        const next = await desktopApi.configureLocal(project.path);
        onState(next);
        setRdcSnapshot(next);
        if (!next.readiness.runtime_ready) {
          throw {
            code: "local_mcp_unavailable",
            message: "Local runtime did not become ready during RDC automatic setup",
            next_action: "Retry automatic setup after Service, Runner, and Project are ready.",
          };
        }
        const handoff = await desktopApi.getLocalMcpHandoff();
        if (
          handoff.authentication !== "bearer" ||
          !handoff.loopbackOnly ||
          !handoff.credentialAvailable
        ) {
          throw {
            code: "local_mcp_unavailable",
            message: "Desktop local MCP handoff is not ready for Bridge auto-discovery",
            next_action: "Retry automatic setup.",
          };
        }
        setRdcVerification("ready");
      } else if (mode === "local") {
        let next = await desktopApi.configureLocal(project.path);
        onState(next);
        if (connectAfterSetup && next.openai_tunnel_configured && next.readiness.runtime_ready && !next.regular_tunnel) {
          next = await desktopApi.startRegularTunnel();
          onState(next);
        }
        onComplete?.();
      } else if (mode === "remote") {
        if (!project) return;
        const oneTimeCode = pairingCode;
        setPairingCode("");
        const next = await desktopApi.configureRemote(
          serverUrl,
          oneTimeCode,
          project.path,
        );
        setRemoteEnrollmentNeedsRefresh(false);
        onState(next);
        onComplete?.();
      } else {
        if (!project) return;
        onState(await desktopApi.startQuickShare(project.path, provider));
        onComplete?.();
      }
    } catch (value) {
      if (mode === "rdc") setRdcVerification("idle");
      const normalized = normalizeDesktopError(value);
      if (mode === "remote" && normalized.code === "pairing_code_invalid") {
        // The optimistic reuse hint is based on saved Desktop state. If the
        // backend proves that connection identity is no longer reusable, expose
        // the one-shot recovery field instead of trapping the user behind the
        // stale reuse hint.
        setRemoteEnrollmentNeedsRefresh(true);
      }
      setError(normalized);
    } finally {
      setBusy(false);
    }
  };

  if (!mode) {
    return (
      <section className="first-run" aria-labelledby="first-run-title" data-webcodex-page="first-run">
        <div className="eyebrow">{t("first.welcome")}</div>
        <h1 id="first-run-title">{t("first.title")}</h1>
        <p className="lede">{t("first.description")}</p>
        <ol className="setup-overview" aria-label={t("workspace.progress")}>
          <li><span>01</span>{t("workspace.prepare")}</li>
          <li><span>02</span>{t("workspace.connect")}</li>
          <li><span>03</span>{t("workspace.verify")}</li>
        </ol>
        <div className="entry-grid">
          <button className="entry-card" onClick={() => setMode("rdc")} data-webcodex-action="choose-rdc-setup">
            <span className="entry-badge">{rdcLabels.badge}</span>
            <strong>{rdcLabels.entryTitle}</strong>
            <span>{rdcLabels.entryDescription}</span>
          </button>
          <button className="entry-card recommended" onClick={() => setMode("local")} data-webcodex-action="choose-local-setup">
            <span className="entry-badge">{t("first.recommended")}</span>
            <strong>{t("first.localTitle")}</strong>
            <span>{t("first.localDescription")}</span>
          </button>
          <button className="entry-card" onClick={() => setMode("remote")} data-webcodex-action="choose-remote-setup">
            <strong>{t("first.remoteTitle")}</strong>
            <span>{t("first.remoteDescription")}</span>
          </button>
          <button className="entry-card" onClick={() => setMode("share")} data-webcodex-action="choose-quick-share-setup">
            <strong>{t("first.shareTitle")}</strong>
            <span>{t("first.shareDescription")}</span>
          </button>
        </div>
      </section>
    );
  }

  const presentation = error ? desktopErrorPresentation(error, t) : null;
  const serverInvalid = error?.code === "server_url_invalid" || error?.code === "server_unreachable";
  const pairingInvalid = error?.code === "pairing_code_invalid";
  const observedRdcState = rdcSnapshot ?? state;
  const rdcServiceReady = observedRdcState.readiness.server === "ready";
  const rdcRunnerReady = observedRdcState.readiness.runner === "ready";
  const rdcProjectReady = Boolean(
    project &&
      observedRdcState.readiness.project === "ready" &&
      observedRdcState.project?.path === project.path,
  );
  const rdcMcpReady = rdcVerification === "ready";

  return (
    <form
      className="setup-shell"
      aria-labelledby="setup-title"
      aria-busy={mutationBusy}
      data-webcodex-page="setup"
      onSubmit={(event) => {
        event.preventDefault();
        void run();
      }}
    >
      <button type="button" className="back-button" onClick={() => setMode(null)} data-webcodex-action="show-setup-options">
        {t("setup.back")}
      </button>
      <div className="eyebrow">{modeLabel(mode, t, rdcLabels)}</div>
      <h1 id="setup-title">{setupTitle(mode, t, rdcLabels)}</h1>
      <p className="lede">{setupDescription(mode, t, rdcLabels)}</p>
      <div className="project-picker-card">
        <div>
          <span className="section-kicker">{t("setup.project")}</span>
          <strong>{project ? project.path : t("setup.chooseProject")}</strong>
          {(mode === "local" || mode === "rdc") && !project && (
            <span className="project-meta">{t("setup.projectRequired")}</span>
          )}
          {project && (
            <span className="project-meta">
              {t("setup.allowedRoot", {
                root: project.allowed_root,
                kind: project.is_git_repository ? t("setup.gitRepository") : t("setup.folder"),
              })}
            </span>
          )}
        </div>
        <button type="button" className="secondary-button" onClick={chooseProject} disabled={mutationBusy} data-webcodex-action="choose-project">
          {project ? t("setup.changeFolder") : t("setup.chooseFolder")}
        </button>
      </div>

      <PowerShellInstallGuidance state={state} onState={onState} />

      {mode === "rdc" && (
        <article
          className="detail-card"
          aria-labelledby="rdc-setup-status-title"
          data-webcodex-rdc-ready={rdcVerification === "ready" ? "true" : "false"}
        >
          <span className="section-kicker">{rdcLabels.badge}</span>
          <h2 id="rdc-setup-status-title">
            {rdcVerification === "ready" ? rdcLabels.readyTitle : rdcLabels.title}
          </h2>
          <p>{rdcVerification === "ready" ? rdcLabels.readyDescription : rdcLabels.privacy}</p>
          {rdcVerification === "checking" && <p className="inline-note">{rdcLabels.checking}</p>}
          <dl className="detail-list">
            <SetupCheck label={rdcLabels.service} ready={rdcServiceReady} labels={rdcLabels} />
            <SetupCheck label={rdcLabels.runner} ready={rdcRunnerReady} labels={rdcLabels} />
            <SetupCheck label={rdcLabels.project} ready={rdcProjectReady} labels={rdcLabels} />
            <SetupCheck label={rdcLabels.localMcp} ready={rdcMcpReady} labels={rdcLabels} />
            <SetupCheck label={rdcLabels.bridge} ready={rdcMcpReady} labels={rdcLabels} />
          </dl>
        </article>
      )}

      {mode === "local" && (
        <details className="setup-tunnel-details">
          <summary>{t("workspace.optionalTunnel")}</summary>
          <TunnelConfigDiagnostics state={state} onState={onState} />
        </details>
      )}

      {mode === "remote" && (
        <div className="form-card">
          <div className="field-group">
            <label htmlFor="setup-server-url">{t("setup.serverUrl")}</label>
            <input
              id="setup-server-url"
              type="url"
              value={serverUrl}
              onChange={(event) => setServerUrl(event.target.value)}
              placeholder="https://webcodex.example.com"
              disabled={mutationBusy}
              aria-describedby="setup-server-url-help"
              aria-invalid={serverInvalid || undefined}
              aria-errormessage={serverInvalid ? "setup-error" : undefined}
            />
            <span className="field-help" id="setup-server-url-help">{t("setup.serverUrlHelp")}</span>
          </div>
          {canReuseRemoteEnrollment ? (
            <div className="enrollment-note">
              <span className="section-kicker">{t("setup.enrollment")}</span>
              <strong>{t("setup.reuseEnrollment")}</strong>
              <span>{t("setup.reuseEnrollmentHelp")}</span>
            </div>
          ) : (
            <div className="field-group">
              <label htmlFor="setup-pairing-code">{t("setup.pairingCode")}</label>
              <input
                id="setup-pairing-code"
                type="password"
                value={pairingCode}
                onChange={(event) => setPairingCode(event.target.value)}
                placeholder="wc_pair_…"
                autoComplete="off"
                spellCheck={false}
                disabled={mutationBusy}
                aria-describedby="setup-pairing-code-help"
                aria-invalid={pairingInvalid || undefined}
                aria-errormessage={pairingInvalid ? "setup-error" : undefined}
              />
              <span className="field-help" id="setup-pairing-code-help">{t("setup.pairingCodeHelp")}</span>
            </div>
          )}
        </div>
      )}

      {mode === "share" && (
        <fieldset className="provider-row provider-fieldset" role="radiogroup" aria-labelledby="quick-share-provider-legend">
          <legend id="quick-share-provider-legend">{t("setup.providerLegend")}</legend>
          {(["cloudflare", "openai", "none"] as QuickShareProvider[]).map((value) => (
            <div
              className={`provider-option ${provider === value ? "selected" : ""}`}
              key={value}
            >
              <input
                id={`quick-share-provider-${value}`}
                type="radio"
                name="quick-share-provider"
                value={value}
                checked={provider === value}
                onChange={() => setProvider(value)}
                disabled={mutationBusy}
                aria-describedby={`quick-share-provider-${value}-description`}
                data-webcodex-control={`quick-share-provider-${value}`}
              />
              <label htmlFor={`quick-share-provider-${value}`}>
                <strong>{providerLabel(value, t)}</strong>
                <span id={`quick-share-provider-${value}-description`}>{providerDescription(value, t)}</span>
              </label>
            </div>
          ))}
        </fieldset>
      )}

      {mode === "local" && state.openai_tunnel_configured && !state.regular_tunnel && (
        <label className="setup-choice-card" htmlFor="setup-connect-chatgpt">
          <input
            id="setup-connect-chatgpt"
            type="checkbox"
            checked={connectAfterSetup}
            onChange={(event) => setConnectAfterSetup(event.target.checked)}
            disabled={mutationBusy}
          />
          <span>
            <strong>{t("setup.connectChatGptAfterSetup")}</strong>
            <small>{t("setup.connectChatGptAfterSetupHelp")}</small>
          </span>
        </label>
      )}

      {mode === "remote" && (
        <details className="advanced-enrollment">
          <summary>{t("setup.advancedEnrollment")}</summary>
          <p>{t("setup.advancedEnrollmentHelp")}</p>
        </details>
      )}

      {error && (
        <div className="error-card" role="alert" id="setup-error">
          <strong>{presentation?.title}</strong>
          <span>{presentation?.action}</span>
          <details>
            <summary>{t("common.details")}</summary>
            <code>{error.code}</code>
            <p>{error.message}</p>
          </details>
          {error.code === "project_not_loaded" && (
            <div className="setup-recovery-actions">
              <button
                type="button"
                className="secondary-button"
                onClick={() => void run()}
                disabled={mutationBusy}
                data-webcodex-action="activate-project"
              >
                {t("setup.reloadProject")}
              </button>
              <span>{t("setup.reloadProjectHelp")}</span>
            </div>
          )}
        </div>
      )}

      <div className="setup-actions">
        {mode === "rdc" && rdcVerification === "ready" ? (
          <>
            <button
              type="button"
              className="primary-button"
              onClick={() => onComplete?.()}
              data-webcodex-action="complete-rdc-setup"
            >
              {rdcLabels.complete}
            </button>
            <button
              type="submit"
              className="secondary-button"
              disabled={mutationBusy || !project}
              data-webcodex-action="configure-rdc-auto"
            >
              {rdcLabels.retest}
            </button>
          </>
        ) : (
          <button
            type="submit"
            className="primary-button"
            disabled={
              mutationBusy ||
              !project ||
              (mode === "remote" &&
                (!serverUrl.trim() || (!canReuseRemoteEnrollment && !pairingCode.trim())))
            }
            data-webcodex-action={
              mode === "rdc"
                ? "configure-rdc-auto"
                : mode === "local"
                  ? "configure-local"
                  : mode === "remote"
                    ? "configure-remote"
                    : "start-quick-share"
            }
          >
            {mutationBusy ? t("common.checking") : actionLabel(mode, canReuseRemoteEnrollment, t, rdcLabels)}
          </button>
        )}
        <span className="action-help">
          {mutationBusy
            ? mode === "rdc" ? rdcLabels.checking : t("setup.verifying")
            : mode === "rdc" ? rdcLabels.noTerminal : t("setup.noTerminal")}
        </span>
      </div>
    </form>
  );
}

type Translate = ReturnType<typeof useLocale>["t"];

function SetupCheck({ label, ready, labels }: { label: string; ready: boolean; labels: RdcLabels }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd className="status-value">
        <i className={`status-dot ${ready ? "ready" : "unknown"}`} aria-hidden="true" />
        {ready ? labels.ready : labels.pending}
      </dd>
    </div>
  );
}

function modeLabel(mode: SetupMode, t: Translate, rdcLabels: RdcLabels) {
  if (mode === "rdc") return rdcLabels.label;
  return mode === "local" ? t("setup.localLabel") : mode === "remote" ? t("setup.remoteLabel") : t("setup.shareLabel");
}

function setupTitle(mode: SetupMode, t: Translate, rdcLabels: RdcLabels) {
  if (mode === "rdc") return rdcLabels.title;
  return mode === "local"
    ? t("setup.localTitle")
    : mode === "remote"
      ? t("setup.remoteTitle")
      : t("setup.shareTitle");
}

function setupDescription(mode: SetupMode, t: Translate, rdcLabels: RdcLabels) {
  if (mode === "rdc") return rdcLabels.description;
  if (mode === "local") return t("setup.localDescription");
  if (mode === "remote") return t("setup.remoteDescription");
  return t("setup.shareDescription");
}

function actionLabel(mode: SetupMode, canReuseRemoteEnrollment: boolean, t: Translate, rdcLabels: RdcLabels) {
  if (mode === "rdc") return rdcLabels.autoSetup;
  if (mode === "local") return t("setup.setUp");
  if (mode === "remote") return canReuseRemoteEnrollment ? t("setup.reconnect") : t("setup.connect");
  return t("setup.startShare");
}

function sameServerOrigin(left: string, right: string) {
  return left.trim().replace(/\/+$/, "").toLowerCase() === right.trim().replace(/\/+$/, "").toLowerCase();
}

function providerLabel(provider: QuickShareProvider, t: Translate) {
  if (provider === "cloudflare") return "Cloudflare";
  if (provider === "openai") return t("activity.source.regular_tunnel");
  return t("common.noChatGpt");
}

function providerDescription(provider: QuickShareProvider, t: Translate) {
  if (provider === "cloudflare") return t("provider.cloudflareDescription");
  if (provider === "openai") return t("provider.openaiDescription");
  return t("provider.localDescription");
}
