// Release builds must not spawn a console window behind the overlay.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    riposte_desktop::run()
}
