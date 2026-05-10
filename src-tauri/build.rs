fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_DESKTOP");

    if std::env::var_os("CARGO_FEATURE_DESKTOP").is_some() {
        ensure_default_icon();
        tauri_build::build();
    }
}

fn ensure_default_icon() {
    let icon_path = std::path::Path::new("icons").join("icon.png");
    if icon_path.exists() && icon_is_rgba_png(&icon_path) {
        return;
    }

    if let Some(parent) = icon_path.parent() {
        std::fs::create_dir_all(parent).expect("failed to create Tauri icons directory");
    }

    // 1x1 RGBA PNG used only to satisfy Tauri's scaffold icon requirement.
    const ICON_PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 218, 99, 252, 207, 192, 80,
        15, 0, 5, 131, 2, 127, 148, 95, 19, 19, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    std::fs::write(icon_path, ICON_PNG).expect("failed to write default Tauri icon");
}

fn icon_is_rgba_png(icon_path: &std::path::Path) -> bool {
    let Ok(bytes) = std::fs::read(icon_path) else {
        return false;
    };

    bytes.len() > 25
        && bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10])
        && bytes[24] == 8
        && bytes[25] == 6
}
