use super::{ConfigTab, FrpcLauncherApp, MainTab, Toast, ToastStyle};
use crate::app_info::AppInfo;
use crate::frpc::{spawn_reader_thread, LogEvent};
use crate::ui::{setup_fonts, setup_visuals};
use crate::util::{app_base_dir, path_to_string, read_text_file, resolve_under_app};
use std::fs;
use std::io::BufReader;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

impl FrpcLauncherApp {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>, app_info: AppInfo) -> Self {
        let (log_tx, log_rx) = mpsc::channel();

        setup_fonts(&cc.egui_ctx);
        setup_visuals(&cc.egui_ctx);

        let app_dir = app_base_dir();
        let frpc_exe_path = "./frpc/frpc.exe".to_string();
        let config_path = "./frpc/frpc.toml".to_string();
        let config_text =
            read_text_file(&resolve_under_app(&app_dir, &config_path)).unwrap_or_default();

        let mut simple_form = crate::config::SimpleConfigForm::default();
        simple_form.apply_from_toml(&config_text);

        Self {
            app_dir,
            app_info,
            frpc_exe_path,
            config_path,
            main_tab: MainTab::Config,
            main_tab_scroll_gen: 0,
            config_tab: ConfigTab::Simple,
            simple_form,
            config_text,
            config_dirty: false,
            log_path: "bad_frpc_launcher.log".to_string(),
            logs: std::collections::VecDeque::new(),
            max_log_lines: 5000,
            toast_seq: 0,
            toasts: std::collections::VecDeque::new(),
            child: std::sync::Arc::new(std::sync::Mutex::new(None)),
            log_tx,
            log_rx,
            last_exit_code: None,
            last_error: None,
        }
    }

    pub(super) fn toast_default_style(&self) -> ToastStyle {
        ToastStyle {
            fill: egui::Color32::from_rgb(255, 252, 254),
            stroke: egui::Color32::from_rgb(230, 214, 231),
            text_color: egui::Color32::from_rgb(86, 78, 92),
            title_color: egui::Color32::from_rgb(94, 71, 95),
            title_size: 13.5,
            body_size: 13.0,
            monospace: false,
        }
    }

    pub(super) fn toast_style_ok(&self) -> ToastStyle {
        ToastStyle {
            fill: egui::Color32::from_rgb(233, 248, 241),
            stroke: egui::Color32::from_rgb(176, 226, 198),
            text_color: egui::Color32::from_rgb(46, 110, 78),
            title_color: egui::Color32::from_rgb(46, 110, 78),
            ..self.toast_default_style()
        }
    }

    pub(super) fn toast_style_info(&self) -> ToastStyle {
        ToastStyle {
            fill: egui::Color32::from_rgb(232, 242, 255),
            stroke: egui::Color32::from_rgb(190, 214, 245),
            text_color: egui::Color32::from_rgb(46, 92, 146),
            title_color: egui::Color32::from_rgb(46, 92, 146),
            ..self.toast_default_style()
        }
    }

    pub(super) fn toast_style_warn(&self) -> ToastStyle {
        ToastStyle {
            fill: egui::Color32::from_rgb(255, 246, 230),
            stroke: egui::Color32::from_rgb(238, 214, 165),
            text_color: egui::Color32::from_rgb(138, 92, 20),
            title_color: egui::Color32::from_rgb(138, 92, 20),
            ..self.toast_default_style()
        }
    }

    pub(super) fn toast_style_error(&self) -> ToastStyle {
        ToastStyle {
            fill: egui::Color32::from_rgb(255, 235, 238),
            stroke: egui::Color32::from_rgb(242, 190, 198),
            text_color: egui::Color32::from_rgb(156, 62, 78),
            title_color: egui::Color32::from_rgb(156, 62, 78),
            ..self.toast_default_style()
        }
    }

    pub(super) fn push_toast(
        &mut self,
        title: Option<String>,
        message: impl Into<String>,
        duration: Duration,
        style: ToastStyle,
    ) {
        self.toast_seq = self.toast_seq.wrapping_add(1);
        self.toasts.push_back(Toast {
            id: self.toast_seq,
            created_at: Instant::now(),
            duration,
            title,
            message: message.into(),
            style,
        });
        while self.toasts.len() > 4 {
            self.toasts.pop_front();
        }
    }

    pub(super) fn toast_info(&mut self, message: impl Into<String>) {
        self.push_toast(None, message, Duration::from_secs(2), self.toast_style_info());
    }

    pub(super) fn toast_ok(&mut self, message: impl Into<String>) {
        self.push_toast(None, message, Duration::from_secs(2), self.toast_style_ok());
    }

    pub(super) fn toast_warn(&mut self, message: impl Into<String>) {
        self.push_toast(None, message, Duration::from_secs(3), self.toast_style_warn());
    }

    pub(super) fn toast_error(&mut self, message: impl Into<String>) {
        self.push_toast(None, message, Duration::from_secs(4), self.toast_style_error());
    }

    pub(super) fn push_toast_custom(
        &mut self,
        title: Option<String>,
        message: impl Into<String>,
        duration: Duration,
        style: ToastStyle,
    ) {
        self.push_toast(title, message, duration, style);
    }

    pub(super) fn resolved_frpc_exe_path(&self) -> std::path::PathBuf {
        resolve_under_app(&self.app_dir, &self.frpc_exe_path)
    }

    pub(super) fn resolved_config_path(&self) -> std::path::PathBuf {
        resolve_under_app(&self.app_dir, &self.config_path)
    }

    pub(super) fn maybe_autofill_config_path(&mut self) {
        let exe = self.resolved_frpc_exe_path();
        if !exe.exists() {
            return;
        }
        let Some(exe_dir) = exe.parent() else {
            return;
        };

        let candidate = exe_dir.join("frpc.toml");
        if !candidate.exists() {
            return;
        }

        let current = self.config_path.trim();
        if current.is_empty() || current == "./frpc/frpc.toml" {
            self.config_path = path_to_string(&self.app_dir, &candidate);
        }
    }

    pub(super) fn sync_simple_from_advanced(&mut self) {
        self.simple_form.apply_from_toml(&self.config_text);
    }

    pub(super) fn sync_advanced_from_simple(&mut self) {
        self.config_text = self.simple_form.to_toml();
        self.config_dirty = true;
    }

    pub(super) fn push_log(&mut self, line: String) {
        if self.logs.len() >= self.max_log_lines {
            let overflow = self.logs.len().saturating_sub(self.max_log_lines) + 1;
            for _ in 0..overflow {
                self.logs.pop_front();
            }
        }
        self.logs.push_back(line);
    }

    pub(super) fn running_state(&mut self) -> bool {
        self.poll_child();
        self.child.lock().unwrap().is_some()
    }

    pub(super) fn poll_child(&mut self) {
        let mut maybe_exit_code: Option<Option<i32>> = None;
        let mut maybe_log_line: Option<String> = None;

        {
            let mut slot = self.child.lock().unwrap();
            if let Some(child) = slot.as_mut() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let code = status.code();
                        maybe_exit_code = Some(code);
                        maybe_log_line = Some(format!(
                            "[[Exited]] 退出码={}",
                            code.map(|v| v.to_string())
                                .unwrap_or_else(|| "None".to_string())
                        ));
                        *slot = None;
                    }
                    Ok(None) => {}
                    Err(err) => {
                        maybe_log_line = Some(format!("[[Error]] 轮询进程状态失败: {err}"));
                        *slot = None;
                    }
                }
            }
        }

        if let Some(code) = maybe_exit_code {
            self.last_exit_code = code;
        }
        if let Some(line) = maybe_log_line {
            self.push_log(line);
        }
    }

    pub(super) fn drain_log_events(&mut self) {
        while let Ok(evt) = self.log_rx.try_recv() {
            match evt {
                LogEvent::Stdout(line) => self.push_log(format!("[OUT] {line}")),
                LogEvent::Stderr(line) => self.push_log(format!("[ERR] {line}")),
                LogEvent::Error(line) => self.push_log(format!("[[Error]] {line}")),
            }
        }
    }

    pub(super) fn load_config(&mut self) -> bool {
        match read_text_file(&self.resolved_config_path()) {
            Ok(s) => {
                self.config_text = s;
                self.config_dirty = false;
                self.last_error = None;
                self.sync_simple_from_advanced();
                self.push_log("[[Info]] 配置已加载".to_string());
                true
            }
            Err(err) => {
                self.last_error = Some(err.to_string());
                self.push_log(format!("[[Error]] 加载配置失败: {err}"));
                false
            }
        }
    }

    pub(super) fn save_config(&mut self) -> bool {
        let path = self.resolved_config_path();
        if let Some(parent) = path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                self.last_error = Some(err.to_string());
                self.push_log(format!("[[Error]] 创建目录失败: {err}"));
                return false;
            }
        }
        match fs::write(&path, self.config_text.as_bytes()) {
            Ok(_) => {
                self.config_dirty = false;
                self.last_error = None;
                self.push_log("[[Info]] 配置已保存".to_string());
                true
            }
            Err(err) => {
                self.last_error = Some(err.to_string());
                self.push_log(format!("[[Error]] 保存配置失败: {err}"));
                false
            }
        }
    }

    pub(super) fn start_frpc(&mut self) {
        if self.running_state() {
            self.push_log("[[Info]] frpc 已在运行".to_string());
            self.toast_warn("frpc 已在运行");
            return;
        }

        if self.config_dirty && !self.save_config() {
            return;
        }

        let exe_path = self.resolved_frpc_exe_path();
        if !exe_path.exists() {
            self.last_error = Some("找不到 frpc.exe".to_string());
            self.push_log(format!("[[Error]] 找不到 frpc.exe: {}", exe_path.display()));
            self.toast_error("找不到 frpc.exe");
            return;
        }

        let config_path = self.resolved_config_path();
        if !config_path.exists() {
            self.last_error = Some("找不到 frpc.toml".to_string());
            self.push_log(format!("[[Error]] 找不到 frpc.toml: {}", config_path.display()));
            self.toast_error("找不到 frpc.toml");
            return;
        }

        let work_dir = exe_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.app_dir.clone());

        let mut cmd = Command::new(&exe_path);
        cmd.current_dir(&work_dir);
        cmd.arg("-c").arg(&config_path);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        match cmd.spawn() {
            Ok(mut child) => {
                self.last_exit_code = None;
                self.last_error = None;
                self.push_log(format!("[[Info]] 已启动: {}", exe_path.display()));
                self.toast_ok("frpc 已启动");

                if let Some(stdout) = child.stdout.take() {
                    spawn_reader_thread(BufReader::new(stdout), self.log_tx.clone(), false);
                }
                if let Some(stderr) = child.stderr.take() {
                    spawn_reader_thread(BufReader::new(stderr), self.log_tx.clone(), true);
                }

                *self.child.lock().unwrap() = Some(child);
            }
            Err(err) => {
                self.last_error = Some(err.to_string());
                self.push_log(format!("[[Error]] 启动失败: {err}"));
                self.toast_error("启动失败");
            }
        }
    }

    pub(super) fn stop_frpc(&mut self) {
        enum StopOutcome {
            Stopped,
            NotRunning,
            Failed,
        }

        let (outcome, log_line): (StopOutcome, String) = {
            let mut slot = self.child.lock().unwrap();
            if let Some(child) = slot.as_mut() {
                match child.kill() {
                    Ok(_) => (StopOutcome::Stopped, "[[Info]] 已发送停止信号".to_string()),
                    Err(err) => (StopOutcome::Failed, format!("[[Error]] 停止失败: {err}")),
                }
            } else {
                (StopOutcome::NotRunning, "[[Info]] frpc 未在运行".to_string())
            }
        };

        match outcome {
            StopOutcome::Stopped => self.toast_info("已发送停止"),
            StopOutcome::NotRunning => self.toast_warn("未在运行"),
            StopOutcome::Failed => self.toast_error("停止失败"),
        }

        self.push_log(log_line);
    }

    pub(super) fn restart_frpc(&mut self) {
        self.toast_info("重启中…");
        self.stop_frpc();
        std::thread::sleep(std::time::Duration::from_millis(200));
        self.poll_child();
        self.start_frpc();
    }

    pub(super) fn save_logs(&mut self) {
        let content = self.logs.iter().cloned().collect::<Vec<_>>().join("\n");
        let path = resolve_under_app(&self.app_dir, &self.log_path);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match fs::write(&path, content.as_bytes()) {
            Ok(_) => self.push_log(format!("[[Info]] 日志已保存到 {}", path.display())),
            Err(err) => self.push_log(format!("[[Error]] 保存日志失败: {err}")),
        }
    }

    pub(super) fn config_parse_status(&self) -> Result<(), String> {
        match toml::from_str::<toml::Value>(&self.config_text) {
            Ok(_) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    }
}
