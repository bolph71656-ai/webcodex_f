use crate::activity::ActivityEntry;
use crate::desktop_shell;
use crate::error::DesktopError;
use crate::models::{
    DesktopStateSnapshot, ProjectSelection, ServerTopology, StoredDesktopConfig, TunnelProxyMode,
};
use crate::state::AppState;
use crate::tray;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use url::{Host, Url};

#[tauri::command]
pub async fn update_tunnel_config(
    app: AppHandle,
    state: State<'_, AppState>,
    request: crate::tunnel_config::TunnelConfigRequest,
) -> Result<DesktopStateSnapshot, DesktopError> {
    project_state_result(&app, state.update_tunnel_config(request).await)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRequest {
    pub project_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSetupRequest {
    pub project_path: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSetupRequest {
    pub server_url: String,
    pub pairing_code: String,
    pub project_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickShareRequest {
    pub project_path: String,
    pub provider: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelDesktopOperationRequest {
    pub operation_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelProxyRequest {
    pub mode: TunnelProxyMode,
    pub custom_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchAtLoginRequest {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalMcpHandoff {
    pub mcp_url: String,
    pub authentication: String,
    pub loopback_only: bool,
    pub credential_available: bool,
}

fn project_state_result(
    app: &AppHandle,
    result: Result<DesktopStateSnapshot, DesktopError>,
) -> Result<DesktopStateSnapshot, DesktopError> {
    if let Ok(snapshot) = &result {
        tray::refresh_from_snapshot(app, snapshot);
    }
    result
}

#[tauri::command]
pub async fn get_desktop_state(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopStateSnapshot, DesktopError> {
    let snapshot = state.get_state();
    tray::refresh_from_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub fn open_powershell_install_guide() -> Result<(), DesktopError> {
    crate::platform::open_powershell_install_guide()
}

#[tauri::command]
pub async fn get_launch_at_login(app: AppHandle) -> Result<bool, DesktopError> {
    match desktop_shell::launch_at_login_enabled(&app) {
        Ok(enabled) => {
            tray::set_launch_at_login_observation(&app, Some(enabled));
            Ok(enabled)
        }
        Err(error) => {
            tray::set_launch_at_login_observation(&app, None);
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn set_launch_at_login(
    request: LaunchAtLoginRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, DesktopError> {
    match desktop_shell::set_launch_at_login(&app, request.enabled) {
        Ok(enabled) => {
            tray::set_launch_at_login_observation(&app, Some(enabled));
            Ok(enabled)
        }
        Err(error) => {
            tray::set_launch_at_login_observation(&app, None);
            let snapshot = state.get_state();
            tray::refresh_from_snapshot(&app, &snapshot);
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn refresh_runtime_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopStateSnapshot, DesktopError> {
    project_state_result(&app, state.refresh_runtime_status().await)
}

#[tauri::command]
pub async fn observe_chatgpt_activity(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopStateSnapshot, DesktopError> {
    project_state_result(&app, state.observe_chatgpt_activity().await)
}

#[tauri::command]
pub async fn resume_saved_runtime(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopStateSnapshot, DesktopError> {
    project_state_result(&app, state.resume_saved_runtime().await)
}

#[tauri::command]
pub async fn update_tunnel_proxy(
    request: TunnelProxyRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopStateSnapshot, DesktopError> {
    let result = state
        .update_tunnel_proxy(request.mode, request.custom_url.as_deref())
        .await;
    project_state_result(&app, result)
}

#[tauri::command]
pub async fn inspect_project(
    request: ProjectRequest,
    state: State<'_, AppState>,
) -> Result<ProjectSelection, DesktopError> {
    state.inspect_project(&request.project_path).await
}

#[tauri::command]
pub async fn configure_local_setup(
    request: LocalSetupRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopStateSnapshot, DesktopError> {
    let result = state
        .configure_local_setup(request.project_path.as_deref())
        .await;
    project_state_result(&app, result)
}

#[tauri::command]
pub async fn activate_local_project(
    request: ProjectRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopStateSnapshot, DesktopError> {
    project_state_result(
        &app,
        state.activate_local_project(&request.project_path).await,
    )
}

#[tauri::command]
pub async fn configure_remote_setup(
    request: RemoteSetupRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopStateSnapshot, DesktopError> {
    let result = state
        .configure_remote_setup(
            &request.server_url,
            &request.pairing_code,
            &request.project_path,
        )
        .await;
    project_state_result(&app, result)
}

#[tauri::command]
pub async fn start_quick_share(
    request: QuickShareRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopStateSnapshot, DesktopError> {
    let result = state
        .start_quick_share(&request.project_path, &request.provider)
        .await;
    project_state_result(&app, result)
}

#[tauri::command]
pub async fn stop_quick_share(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopStateSnapshot, DesktopError> {
    project_state_result(&app, state.stop_quick_share().await)
}

#[tauri::command]
pub async fn start_regular_tunnel(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopStateSnapshot, DesktopError> {
    project_state_result(&app, state.start_regular_tunnel().await)
}

#[tauri::command]
pub async fn stop_regular_tunnel(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopStateSnapshot, DesktopError> {
    project_state_result(&app, state.stop_regular_tunnel().await)
}

#[tauri::command]
pub async fn get_local_mcp_handoff(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<LocalMcpHandoff, DesktopError> {
    ensure_local_mcp_ready(&state.get_state())?;
    let config = load_desktop_config(&app).await?;
    let runtime = config.runtime.ok_or_else(local_mcp_unavailable)?;
    let mcp_url = local_mcp_url(&runtime.server_url)?;
    let credential_available = runtime
        .user_token_file
        .as_ref()
        .is_some_and(|path| path.is_file());
    if !credential_available {
        return Err(local_mcp_unavailable());
    }
    Ok(LocalMcpHandoff {
        mcp_url,
        authentication: "bearer".to_string(),
        loopback_only: true,
        credential_available,
    })
}

#[tauri::command]
pub async fn get_local_mcp_credential(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, DesktopError> {
    ensure_local_mcp_ready(&state.get_state())?;
    let config = load_desktop_config(&app).await?;
    let runtime = config.runtime.ok_or_else(local_mcp_unavailable)?;
    let _ = local_mcp_url(&runtime.server_url)?;
    let token_file = runtime.user_token_file.ok_or_else(local_mcp_unavailable)?;
    let bytes = tokio::fs::read(&token_file)
        .await
        .map_err(|_| local_mcp_unavailable())?;
    if bytes.is_empty() || bytes.len() > 64 * 1024 {
        return Err(local_mcp_unavailable());
    }
    let token = String::from_utf8(bytes).map_err(|_| local_mcp_unavailable())?;
    let token = token.trim();
    if token.is_empty() {
        return Err(local_mcp_unavailable());
    }
    Ok(token.to_string())
}

#[tauri::command]
pub async fn stop_local_runtime(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopStateSnapshot, DesktopError> {
    project_state_result(&app, state.stop_local_runtime().await)
}

#[tauri::command]
pub async fn cancel_desktop_operation(
    request: CancelDesktopOperationRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopStateSnapshot, DesktopError> {
    project_state_result(&app, state.cancel_operation(&request.operation_id))
}

#[tauri::command]
pub async fn get_bounded_activity(
    state: State<'_, AppState>,
) -> Result<Vec<ActivityEntry>, DesktopError> {
    Ok(state.activity())
}

fn ensure_local_mcp_ready(snapshot: &DesktopStateSnapshot) -> Result<(), DesktopError> {
    let local_server = snapshot
        .topology
        .as_ref()
        .is_some_and(|topology| matches!(&topology.server, ServerTopology::Local));
    if !local_server || !snapshot.readiness.runtime_ready {
        return Err(DesktopError::new(
            "local_mcp_unavailable",
            "The local MCP endpoint is not ready",
            "Start the local runtime and wait for Service, Runner, and Project to become ready.",
        ));
    }
    Ok(())
}

async fn load_desktop_config(app: &AppHandle) -> Result<StoredDesktopConfig, DesktopError> {
    let data_dir = app.path().app_local_data_dir().map_err(|_| {
        DesktopError::new(
            "local_mcp_unavailable",
            "Desktop local state is unavailable",
            "Check local application-data permissions and retry.",
        )
    })?;
    let bytes = tokio::fs::read(data_dir.join("desktop-state.json"))
        .await
        .map_err(|_| local_mcp_unavailable())?;
    if bytes.is_empty() || bytes.len() > 256 * 1024 {
        return Err(local_mcp_unavailable());
    }
    serde_json::from_slice(&bytes).map_err(|_| local_mcp_unavailable())
}

fn local_mcp_url(server_url: &str) -> Result<String, DesktopError> {
    let mut url = Url::parse(server_url).map_err(|_| local_mcp_unavailable())?;
    let loopback = match url.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    };
    if url.scheme() != "http" || !loopback {
        return Err(DesktopError::new(
            "local_mcp_not_loopback",
            "Desktop refused to expose non-loopback MCP handoff data",
            "Use the Desktop-owned local runtime for tunnel-free local MCP access.",
        ));
    }
    url.set_path("/mcp");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

fn local_mcp_unavailable() -> DesktopError {
    DesktopError::new(
        "local_mcp_unavailable",
        "Local MCP connection information is unavailable",
        "Start or reconfigure the local runtime, then retry.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_mcp_url_accepts_only_loopback_http() {
        assert_eq!(
            local_mcp_url("http://127.0.0.1:58208").unwrap(),
            "http://127.0.0.1:58208/mcp"
        );
        assert_eq!(
            local_mcp_url("http://localhost:8080/").unwrap(),
            "http://localhost:8080/mcp"
        );
        assert_eq!(
            local_mcp_url("https://example.test").unwrap_err().code,
            "local_mcp_not_loopback"
        );
        assert_eq!(
            local_mcp_url("http://192.168.1.10:8080").unwrap_err().code,
            "local_mcp_not_loopback"
        );
    }
}
