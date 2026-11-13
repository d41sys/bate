fn main() {
    // The page may call these commands and nothing else of the shell's.
    let manifest = tauri_build::AppManifest::new().commands(&["send", "recv", "new_window"]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("the shell's build configuration");
}
