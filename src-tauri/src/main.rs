#[cfg(feature = "desktop")]
fn main() {
    skilldock_lib::run();
}

#[cfg(not(feature = "desktop"))]
fn main() {
    eprintln!("Enable the `desktop` feature to run the Tauri shell.");
}
