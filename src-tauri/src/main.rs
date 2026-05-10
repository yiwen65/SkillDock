#[cfg(feature = "desktop")]
fn main() {
    skills_collection_app_lib::run();
}

#[cfg(not(feature = "desktop"))]
fn main() {
    eprintln!("Enable the `desktop` feature to run the Tauri shell.");
}
