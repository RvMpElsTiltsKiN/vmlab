mod commands;
mod migrate;
mod monitor;
mod state;

pub fn run() {
    // Antes de que Tauri/WebView2 creen las carpetas del nuevo identificador.
    migrate::legacy_data();
    tauri::Builder::default()
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
        .run(tauri::generate_context!())
        .expect("error al iniciar VMLab");
}
