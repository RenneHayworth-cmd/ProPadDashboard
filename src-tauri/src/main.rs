#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod native;
mod settings;

fn main() {
    app::run();
}
