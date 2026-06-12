mod state;
mod view;

use crate::app_info::AppInfo;
use crate::config::SimpleConfigForm;
use crate::frpc::LogEvent;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Child;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConfigTab {
    Simple,
    Advanced,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MainTab {
    Config,
    Logs,
    About,
}

#[derive(Clone)]
struct ToastStyle {
    fill: egui::Color32,
    stroke: egui::Color32,
    text_color: egui::Color32,
    title_color: egui::Color32,
    title_size: f32,
    body_size: f32,
    monospace: bool,
}

#[derive(Clone)]
struct Toast {
    id: u64,
    created_at: Instant,
    duration: Duration,
    title: Option<String>,
    message: String,
    style: ToastStyle,
}

pub(crate) struct FrpcLauncherApp {
    app_dir: PathBuf,
    app_info: AppInfo,
    frpc_exe_path: String,
    config_path: String,
    main_tab: MainTab,
    main_tab_scroll_gen: u64,
    config_tab: ConfigTab,
    simple_form: SimpleConfigForm,
    config_text: String,
    config_dirty: bool,
    log_path: String,
    logs: VecDeque<String>,
    max_log_lines: usize,
    toast_seq: u64,
    toasts: VecDeque<Toast>,
    child: Arc<Mutex<Option<Child>>>,
    log_tx: mpsc::Sender<LogEvent>,
    log_rx: mpsc::Receiver<LogEvent>,
    last_exit_code: Option<i32>,
    last_error: Option<String>,
}

pub(crate) fn app_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 760.0])
            .with_min_inner_size([980.0, 620.0]),
        ..Default::default()
    }
}

pub(crate) fn run_app() -> eframe::Result<()> {
    let app_dir = crate::util::app_base_dir();
    let app_info = AppInfo::load(&app_dir);
    let title = app_info.window_title();

    eframe::run_native(
        &title,
        app_options(),
        Box::new(move |cc| Box::new(FrpcLauncherApp::new(cc, app_info.clone()))),
    )
}
