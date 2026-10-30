fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_dashboard_state",
            "select_controller",
            "set_autostart",
            "set_reduced_motion",
            "set_panel_mode",
            "hide_panel",
        ]),
    ))
    .expect("Tauri resource build failed");
}
