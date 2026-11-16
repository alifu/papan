fn main() {
    // Windows: embed one manifest (Common Controls v6) into every target, including the
    // lib unit-test exe. Without it the test exe fails to start with STATUS_ENTRYPOINT_NOT_FOUND.
    // Tauri's own default manifest is turned off so the two don't collide.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest_dir =
            std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
        println!("cargo:rerun-if-changed=windows-app.manifest");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{manifest_dir}\\windows-app.manifest");
        let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
        tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
            .expect("failed to run tauri-build");
        return;
    }
    tauri_build::build()
}
