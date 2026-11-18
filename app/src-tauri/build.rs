fn main() {
    println!("cargo:rerun-if-changed=kit.manifest.xml");
    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("kit.manifest.xml"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows)).expect("tauri build script");
}
