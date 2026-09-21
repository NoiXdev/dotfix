// Prevents an extra console window on Windows. dotfix is macOS-only, but the
// attribute is harmless and keeps the file identical to every other Tauri app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    dotfix_app_lib::run()
}
