//! Monitorización en tiempo real: un bucle en segundo plano consulta el estado
//! (2 procesos cortos cada 2 s) y emite el evento `vmlab:status` solo cuando algo cambia.
use crate::state::AppState;
use vmlab_core::engine;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub const STATUS_EVENT: &str = "vmlab:status";
const INTERVAL: Duration = Duration::from_secs(2);

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last = None;
        loop {
            let tools = app.state::<AppState>().tools().await;
            let snap = engine::snapshot(&tools).await;
            if last.as_ref() != Some(&snap) {
                let _ = app.emit(STATUS_EVENT, &snap);
                last = Some(snap);
            }
            tokio::time::sleep(INTERVAL).await;
        }
    });
}
