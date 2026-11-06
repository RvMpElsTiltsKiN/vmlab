//! Puente IPC: la única superficie que la interfaz web puede invocar.
//! Todos los comandos son `async`, así que se ejecutan en el runtime de Tauri y
//! nunca bloquean el hilo de la UI. El frontend NO puede lanzar procesos arbitrarios:
//! solo enviar datos (tipos cerrados) que `vmlab-core` valida antes de ejecutar.
use crate::state::AppState;
use vmlab_core::model::{
    Action, ApplyReport, DockerStartInfo, GuestAdditionsPrep, GuestConfigRequest, GuestProfile, GuestReport, Inventory, NodeKind,
    NodeProbe, ProbeNode, Snapshot, Topology,
};
use vmlab_core::tools::Tools;
use vmlab_core::engine;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

const MAX_WORKSPACE_BYTES: usize = 5 * 1024 * 1024;

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[tauri::command]
pub async fn detect_tools(state: State<'_, AppState>) -> Result<Tools, String> {
    let fresh = Tools::detect();
    *state.tools.write().await = fresh.clone();
    Ok(fresh)
}

#[tauri::command]
pub async fn list_machines(state: State<'_, AppState>) -> Result<Inventory, String> {
    Ok(engine::inventory(&state.tools().await).await)
}

#[tauri::command]
pub async fn refresh_status(state: State<'_, AppState>) -> Result<Snapshot, String> {
    Ok(engine::snapshot(&state.tools().await).await)
}

#[tauri::command]
pub async fn machine_action(
    state: State<'_, AppState>,
    kind: NodeKind,
    target: String,
    image: Option<String>,
    action: Action,
) -> Result<(), String> {
    let tools = state.tools().await;
    engine::act(&tools, kind, &target, image.as_deref(), action).await.map_err(s)
}

#[tauri::command]
pub async fn apply_topology(state: State<'_, AppState>, topology: Topology) -> Result<ApplyReport, String> {
    Ok(engine::apply(&state.tools().await, &topology).await)
}

/// Sondea la VM (Linux) a través de Guest Additions y devuelve gestores, rutas e interfaces.
#[tauri::command]
pub async fn guest_probe(
    state: State<'_, AppState>,
    target: String,
    username: String,
    password: String,
) -> Result<GuestProfile, String> {
    engine::guest_probe(&state.tools().await, &target, &username, &password)
        .await
        .map_err(s)
}

/// Escribe la configuración de red dentro de la VM Linux sin abrir su consola.
#[tauri::command]
pub async fn guest_apply(state: State<'_, AppState>, request: GuestConfigRequest) -> Result<GuestReport, String> {
    engine::guest_apply(&state.tools().await, request).await.map_err(s)
}

/// Monta la ISO de Guest Additions y abre la VM para instalar o actualizar las adiciones.
#[tauri::command]
pub async fn guest_additions_prepare(
    state: State<'_, AppState>,
    target: String,
) -> Result<GuestAdditionsPrep, String> {
    engine::guest_additions_prepare(&state.tools().await, &target).await.map_err(s)
}

/// Comprueba la comunicación real de los nodos del lienzo (IP en la red de VMLab).
#[tauri::command]
pub async fn network_probe(
    state: State<'_, AppState>,
    nodes: Vec<ProbeNode>,
) -> Result<Vec<NodeProbe>, String> {
    Ok(engine::network_probe(&state.tools().await, nodes).await)
}

/// Arranca Docker Desktop en segundo plano (sin abrir su panel) y espera al motor.
#[tauri::command]
pub async fn docker_start(state: State<'_, AppState>) -> Result<DockerStartInfo, String> {
    let mut tools = state.tools().await;
    if tools.docker.is_none() {
        tools = Tools::detect();
    }
    let info = engine::docker_start(&tools, 180).await.map_err(s)?;
    if info.launched {
        // Docker Desktop no estaba en marcha: VMLab pasa a ser responsable de pararlo.
        state.own_docker();
    }
    if info.engine_ready {
        // Por si el CLI no estaba en el PATH y Docker Desktop acaba de instalarlo/registrarlo.
        *state.tools.write().await = Tools::detect();
    }
    Ok(info)
}

fn workspace_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(s)?;
    std::fs::create_dir_all(&dir).map_err(s)?;
    Ok(dir.join("workspace.json"))
}

/// Guardado atómico: se escribe a un temporal y se renombra, para no corromper el lienzo si se cae la app.
#[tauri::command]
pub async fn save_workspace(app: AppHandle, json: String) -> Result<(), String> {
    if json.len() > MAX_WORKSPACE_BYTES {
        return Err("el espacio de trabajo es demasiado grande".into());
    }
    serde_json::from_str::<serde_json::Value>(&json).map_err(s)?;
    let path = workspace_path(&app)?;
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, json).await.map_err(s)?;
    tokio::fs::rename(&tmp, &path).await.map_err(s)
}

#[tauri::command]
pub async fn load_workspace(app: AppHandle) -> Result<Option<String>, String> {
    let path = workspace_path(&app)?;
    match tokio::fs::read_to_string(&path).await {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(s(e)),
    }
}
