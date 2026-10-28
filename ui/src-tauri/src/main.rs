// Thin launcher — all application logic lives in lib.rs so the same entry point
// works across desktop (and, later, mobile) targets. The windows_subsystem
// attribute keeps a console window from opening alongside release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    selfishnet_ui_lib::run()
}
