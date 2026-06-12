#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod app_info;
mod config;
mod frpc;
mod ui;
mod util;

fn main() -> eframe::Result<()> {
    app::run_app()
}
