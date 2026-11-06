mod commands;
mod migrate;
mod monitor;
mod state;

use tauri::Manager;

pub fn run() {
    // Antes de que Tauri/WebView2 creen las carpetas del nuevo identificador.
    migrate::legacy_data();
    let app = tauri::Builder::default()
        .manage(state::AppState::new())
        .setup(|app| {
            monitor::spawn(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::detect_tools,
            commands::list_machines,
            commands::machine_action,
            commands::apply_topology,
            commands::guest_probe,
            commands::guest_apply,
            commands::guest_additions_prepare,
            commands::network_probe,
            commands::docker_start,
            commands::refresh_status,
            commands::save_workspace,
            commands::load_workspace,
        ])
        .build(tauri::generate_context!())
        .expect("error al iniciar VMLab");

    app.run(|handle, event| {
        // Al cerrar, si VMLab arrancó Docker Desktop, lo detiene para liberar su VM.
        // Nunca toca un Docker que ya estuviera en marcha antes de abrir la app.
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            let owned = handle.state::<state::AppState>().take_docker_ownership();
            if owned {
                api.prevent_exit();
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let tools = handle.state::<state::AppState>().tools().await;
                    let _ = vmlab_core::engine::docker_stop(&tools).await;
                    handle.exit(0);
                });
            }
        }
    });
}
