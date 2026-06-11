#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// frpc GUI 启动器（Windows）
//
// 目标：
// - 在 GUI 中编辑/保存 frpc.toml
// - 启动/停止/重启 frpc.exe
// - 实时显示 stdout/stderr 日志，并支持保存到文件
//
// 设计要点：
// - frpc 作为子进程启动，stdout/stderr 通过管道读取
// - 读取线程通过 mpsc 把日志事件发送回 UI 线程
// - UI 每帧轮询接收队列 + 轮询子进程退出状态

use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
enum LogEvent {
    // 子进程 stdout 的一行输出（已去除行尾换行符）
    Stdout(String),
    // 子进程 stderr 的一行输出（已去除行尾换行符）
    Stderr(String),
    // UI/启动器自身的错误信息（不代表 frpc 的 stderr）
    Error(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConfigTab {
    Simple,
    Advanced,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MainTab {
    Config,
    Logs,
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

struct SimpleConfigForm {
    server_addr: String,
    server_port: String,
    auth_token: String,

    proxy_name: String,
    proxy_type: String,
    local_ip: String,
    local_port: String,
    remote_port: String,
}

impl Default for SimpleConfigForm {
    fn default() -> Self {
        Self {
            server_addr: String::new(),
            server_port: "7000".to_string(),
            auth_token: String::new(),
            proxy_name: "minecraft".to_string(),
            proxy_type: "tcp".to_string(),
            local_ip: "127.0.0.1".to_string(),
            local_port: "25565".to_string(),
            remote_port: "25565".to_string(),
        }
    }
}

impl SimpleConfigForm {
    fn apply_from_toml(&mut self, text: &str) {
        let Ok(v) = toml::from_str::<toml::Value>(text) else {
            return;
        };

        if let Some(s) = v.get("serverAddr").and_then(|x| x.as_str()) {
            self.server_addr = s.to_string();
        }
        if let Some(n) = v.get("serverPort").and_then(|x| x.as_integer()) {
            self.server_port = n.to_string();
        }

        if let Some(token) = v
            .get("auth")
            .and_then(|x| x.get("token"))
            .and_then(|x| x.as_str())
        {
            self.auth_token = token.to_string();
        }

        if let Some(proxy) = v.get("proxies").and_then(|x| x.as_array()).and_then(|arr| arr.first()) {
            if let Some(name) = proxy.get("name").and_then(|x| x.as_str()) {
                self.proxy_name = name.to_string();
            }
            if let Some(ty) = proxy.get("type").and_then(|x| x.as_str()) {
                self.proxy_type = normalize_proxy_type(ty);
            }
            if let Some(ip) = proxy.get("localIP").and_then(|x| x.as_str()) {
                self.local_ip = ip.to_string();
            }
            if let Some(p) = proxy.get("localPort").and_then(|x| x.as_integer()) {
                self.local_port = p.to_string();
            }
            if let Some(p) = proxy.get("remotePort").and_then(|x| x.as_integer()) {
                self.remote_port = p.to_string();
            }
        }
    }

    fn to_toml(&self) -> String {
        let server_port = self.server_port.trim().parse::<u16>().unwrap_or(7000);
        let local_port = self.local_port.trim().parse::<u16>().unwrap_or(0);
        let remote_port = self.remote_port.trim().parse::<u16>().unwrap_or(0);

        let mut out = String::new();
        out.push_str(&format!("serverAddr = \"{}\"\n", escape_toml_string(&self.server_addr)));
        out.push_str(&format!("serverPort = {}\n", server_port));

        if !self.auth_token.trim().is_empty() {
            out.push('\n');
            out.push_str("auth.method = \"token\"\n");
            out.push_str(&format!(
                "auth.token = \"{}\"\n",
                escape_toml_string(self.auth_token.trim())
            ));
        }

        out.push('\n');
        out.push_str("[[proxies]]\n");
        out.push_str(&format!("name = \"{}\"\n", escape_toml_string(&self.proxy_name)));
        out.push_str(&format!("type = \"{}\"\n", escape_toml_string(&self.proxy_type)));
        out.push_str(&format!("localIP = \"{}\"\n", escape_toml_string(&self.local_ip)));
        out.push_str(&format!("localPort = {}\n", local_port));
        out.push_str(&format!("remotePort = {}\n", remote_port));
        out
    }
}

// eframe/egui 应用状态：包含路径配置、编辑器内容、子进程句柄和日志缓存。
struct FrpcLauncherApp {
    // 应用所在目录（用于解析相对路径：让双击运行/快捷方式启动时也能正确找到 frpc 目录）
    app_dir: PathBuf,

    frpc_exe_path: String,
    config_path: String,

    main_tab: MainTab,
    main_tab_scroll_gen: u64,
    config_tab: ConfigTab,
    simple_form: SimpleConfigForm,

    // 配置文件的当前文本（多行编辑器绑定）
    config_text: String,
    // 配置文本是否被编辑但未保存
    config_dirty: bool,

    // 日志保存路径
    log_path: String,
    // UI 日志缓存（环形/截断队列，避免无限增长）
    logs: VecDeque<String>,
    // 最大缓存行数（超过则丢弃最旧日志）
    max_log_lines: usize,

    toast_seq: u64,
    toasts: VecDeque<Toast>,

    // frpc 子进程句柄（放入 Mutex 以便在 UI 回调中安全访问）
    // 这里使用 Arc 的原因：未来如果要把控制按钮放入多线程回调也无需改结构。
    child: Arc<Mutex<Option<Child>>>,
    // 读取线程 -> UI 线程的事件通道
    log_tx: mpsc::Sender<LogEvent>,
    log_rx: mpsc::Receiver<LogEvent>,

    // 最近一次子进程退出码（用于 UI 展示）
    last_exit_code: Option<i32>,
    // 最近一次启动器级错误（如读写文件失败、spawn 失败等）
    last_error: Option<String>,
}

impl FrpcLauncherApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // 无界通道：读取线程持续写入，UI 线程每帧尽可能取出并刷新显示。
        let (log_tx, log_rx) = mpsc::channel();

        setup_fonts(&cc.egui_ctx);
        setup_visuals(&cc.egui_ctx);

        let app_dir = app_base_dir();

        let frpc_exe_path = "./frpc/frpc.exe".to_string();
        let config_path = "./frpc/frpc.toml".to_string();
        let config_text = read_text_file(&resolve_under_app(&app_dir, &config_path)).unwrap_or_default();

        let mut simple_form = SimpleConfigForm::default();
        simple_form.apply_from_toml(&config_text);

        Self {
            app_dir,
            frpc_exe_path,
            config_path,
            main_tab: MainTab::Config,
            main_tab_scroll_gen: 0,
            config_tab: ConfigTab::Simple,
            simple_form,
            config_text,
            config_dirty: false,

            log_path: "bad_frpc_launcher.log".to_string(),
            logs: VecDeque::new(),
            max_log_lines: 5000,

            toast_seq: 0,
            toasts: VecDeque::new(),

            child: Arc::new(Mutex::new(None)),
            log_tx,
            log_rx,

            last_exit_code: None,
            last_error: None,
        }
    }

    fn toast_default_style(&self) -> ToastStyle {
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

    fn toast_style_ok(&self) -> ToastStyle {
        ToastStyle {
            fill: egui::Color32::from_rgb(233, 248, 241),
            stroke: egui::Color32::from_rgb(176, 226, 198),
            text_color: egui::Color32::from_rgb(46, 110, 78),
            title_color: egui::Color32::from_rgb(46, 110, 78),
            ..self.toast_default_style()
        }
    }

    fn toast_style_info(&self) -> ToastStyle {
        ToastStyle {
            fill: egui::Color32::from_rgb(232, 242, 255),
            stroke: egui::Color32::from_rgb(190, 214, 245),
            text_color: egui::Color32::from_rgb(46, 92, 146),
            title_color: egui::Color32::from_rgb(46, 92, 146),
            ..self.toast_default_style()
        }
    }

    fn toast_style_warn(&self) -> ToastStyle {
        ToastStyle {
            fill: egui::Color32::from_rgb(255, 246, 230),
            stroke: egui::Color32::from_rgb(238, 214, 165),
            text_color: egui::Color32::from_rgb(138, 92, 20),
            title_color: egui::Color32::from_rgb(138, 92, 20),
            ..self.toast_default_style()
        }
    }

    fn toast_style_error(&self) -> ToastStyle {
        ToastStyle {
            fill: egui::Color32::from_rgb(255, 235, 238),
            stroke: egui::Color32::from_rgb(242, 190, 198),
            text_color: egui::Color32::from_rgb(156, 62, 78),
            title_color: egui::Color32::from_rgb(156, 62, 78),
            ..self.toast_default_style()
        }
    }

    fn push_toast(
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

    fn toast_info(&mut self, message: impl Into<String>) {
        self.push_toast(None, message, Duration::from_secs(2), self.toast_style_info());
    }

    fn toast_ok(&mut self, message: impl Into<String>) {
        self.push_toast(None, message, Duration::from_secs(2), self.toast_style_ok());
    }

    fn toast_warn(&mut self, message: impl Into<String>) {
        self.push_toast(None, message, Duration::from_secs(3), self.toast_style_warn());
    }

    fn toast_error(&mut self, message: impl Into<String>) {
        self.push_toast(None, message, Duration::from_secs(4), self.toast_style_error());
    }

    fn push_toast_custom(
        &mut self,
        title: Option<String>,
        message: impl Into<String>,
        duration: Duration,
        style: ToastStyle,
    ) {
        self.push_toast(title, message, duration, style);
    }

    fn show_toasts(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        self.toasts
            .retain(|t| now.duration_since(t.created_at) < t.duration);
        if self.toasts.is_empty() {
            return;
        }

        let mut dismiss: Vec<u64> = Vec::new();
        egui::Area::new(egui::Id::new("toast_area"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 10.0);
                ui.set_min_width(280.0);

                for toast in self.toasts.iter().rev() {
                    let Toast {
                        id,
                        title,
                        message,
                        style,
                        ..
                    } = toast;

                    let response = egui::Frame::none()
                        .fill(style.fill)
                        .stroke(egui::Stroke::new(1.0, style.stroke))
                        .rounding(egui::Rounding::same(10.0))
                        .inner_margin(egui::Margin::same(10.0))
                        .show(ui, |ui| {
                            ui.set_min_width(280.0);

                            if let Some(t) = title.as_deref() {
                                ui.label(
                                    egui::RichText::new(t)
                                        .size(style.title_size)
                                        .color(style.title_color)
                                        .strong(),
                                );
                                ui.add_space(4.0);
                            }

                            let mut rich = egui::RichText::new(message)
                                .size(style.body_size)
                                .color(style.text_color);
                            if style.monospace {
                                rich = rich.monospace();
                            }
                            ui.add(egui::Label::new(rich).wrap(true));
                        })
                        .response;

                    if response.clicked() {
                        dismiss.push(*id);
                    }
                }
            });

        if !dismiss.is_empty() {
            self.toasts.retain(|t| !dismiss.contains(&t.id));
        }
    }

    fn resolved_frpc_exe_path(&self) -> PathBuf {
        resolve_under_app(&self.app_dir, &self.frpc_exe_path)
    }

    fn resolved_config_path(&self) -> PathBuf {
        resolve_under_app(&self.app_dir, &self.config_path)
    }

    fn maybe_autofill_config_path(&mut self) {
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

    fn sync_simple_from_advanced(&mut self) {
        self.simple_form.apply_from_toml(&self.config_text);
    }

    fn sync_advanced_from_simple(&mut self) {
        self.config_text = self.simple_form.to_toml();
        self.config_dirty = true;
    }

    // 追加一行日志到缓存，并在超过阈值时丢弃最旧日志。
    fn push_log(&mut self, line: String) {
        if self.logs.len() >= self.max_log_lines {
            let overflow = self.logs.len().saturating_sub(self.max_log_lines) + 1;
            for _ in 0..overflow {
                self.logs.pop_front();
            }
        }
        self.logs.push_back(line);
    }

    // 获取“当前是否运行中”的状态。
    // 这里会先轮询子进程是否退出：一旦退出就回收句柄并记录退出码。
    fn running_state(&mut self) -> bool {
        self.poll_child();
        self.child.lock().unwrap().is_some()
    }

    // 非阻塞地轮询子进程是否已经退出。
    // - 若已退出：记录退出码、写入日志、释放 Child 句柄
    // - 若仍运行：不做任何事
    fn poll_child(&mut self) {
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
                            code.map(|v| v.to_string()).unwrap_or_else(|| "None".to_string())
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

    // 将读取线程发送过来的日志事件尽量取出并落到 logs 队列中。
    // 使用 try_recv：避免阻塞 UI 渲染线程。
    fn drain_log_events(&mut self) {
        while let Ok(evt) = self.log_rx.try_recv() {
            match evt {
                LogEvent::Stdout(line) => self.push_log(format!("[OUT] {line}")),
                LogEvent::Stderr(line) => self.push_log(format!("[ERR] {line}")),
                LogEvent::Error(line) => self.push_log(format!("[[Error]] {line}")),
            }
        }
    }

    // 从磁盘重新加载配置文件到编辑器。
    fn load_config(&mut self) -> bool {
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

    // 保存编辑器中的配置文本到磁盘。
    // - 成功：清空 dirty 标记
    // - 失败：记录 last_error，并在日志中提示
    fn save_config(&mut self) -> bool {
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

    // 启动 frpc 子进程。
    // 约定的启动命令：frpc.exe -c frpc.toml
    // - 若配置有未保存改动：先保存，避免“UI 显示的配置”和“实际运行的配置”不一致
    // - stdout/stderr 分别起线程读取，一行一事件发送到 UI
    fn start_frpc(&mut self) {
        if self.running_state() {
            self.push_log("[[Info]] frpc 已在运行".to_string());
            self.toast_warn("frpc 已在运行");
            return;
        }

        if self.config_dirty {
            if !self.save_config() {
                return;
            }
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

                // 将 stdout/stderr 管道交给后台线程读取。
                // 必须 take()：否则 Child 会保留句柄，导致无法独占读取。
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

    // 尝试终止 frpc 子进程。
    // 说明：kill() 在 Windows 上等价于强制结束进程；这里不做更温和的 CTRL+C/CTRL+BREAK。
    fn stop_frpc(&mut self) {
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

    // 重启 frpc：
    // - 先 kill
    // - 短暂等待让系统回收资源
    // - 轮询一次确保句柄释放
    // - 再 start
    fn restart_frpc(&mut self) {
        self.toast_info("重启中…");
        self.stop_frpc();
        std::thread::sleep(std::time::Duration::from_millis(200));
        self.poll_child();
        self.start_frpc();
    }

    // 将当前 UI 日志缓存写入文件。
    // 仅保存当前缓存内容（受 max_log_lines 限制），不会读取历史文件再拼接。
    fn save_logs(&mut self) {
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

    // TOML 语法快速校验：只做语法解析，不做 frpc 字段语义校验。
    fn config_parse_status(&self) -> Result<(), String> {
        match toml::from_str::<toml::Value>(&self.config_text) {
            Ok(_) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    }

    // 配置页：简单表单/高级文本两种模式，加载/保存按钮跟随子标题放置。
    fn render_config_panel(&mut self, ui: &mut egui::Ui, compact: bool, window_height: f32) {
        let panel_width = ui.available_width();
        let text_width = (panel_width - 120.0).clamp(140.0, 420.0);
        let compact_text_width = (panel_width - 170.0).clamp(120.0, 180.0);
        let token_width = (panel_width - 120.0).clamp(180.0, 520.0);
        let half_width = ((panel_width - 170.0) / 2.0).clamp(110.0, 220.0);
        let editor_height = if compact {
            (window_height * 0.27).clamp(190.0, 280.0)
        } else {
            (window_height - 320.0).clamp(240.0, 420.0)
        };

        section_card(ui, "配置", |ui| {
            ui.horizontal_wrapped(|ui| {
                let simple = tab_button(ui, "简单配置", self.config_tab == ConfigTab::Simple);
                if simple.clicked() {
                    self.config_tab = ConfigTab::Simple;
                }
                let adv = tab_button(ui, "高级配置", self.config_tab == ConfigTab::Advanced);
                if adv.clicked() {
                    self.config_tab = ConfigTab::Advanced;
                }
            });

            ui.add_space(10.0);

            match self.config_tab {
                ConfigTab::Simple => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new("连接配置")
                                .size(17.0)
                                .strong()
                                .color(egui::Color32::from_rgb(103, 78, 104)),
                        );
                        ui.add_space(8.0);
                        if tinted_button(
                            ui,
                            "加载配置",
                            [90.0, 30.0],
                            egui::Color32::from_rgb(232, 242, 255),
                            egui::Color32::from_rgb(190, 214, 245),
                            egui::Color32::from_rgb(46, 92, 146),
                        )
                        .clicked()
                        {
                            if self.load_config() {
                                self.toast_ok("配置已加载");
                            } else {
                                self.toast_error("加载配置失败");
                            }
                        }
                        if tinted_button(
                            ui,
                            "保存配置",
                            [90.0, 30.0],
                            egui::Color32::from_rgb(255, 232, 244),
                            egui::Color32::from_rgb(242, 190, 220),
                            egui::Color32::from_rgb(122, 57, 93),
                        )
                        .clicked()
                        {
                            self.sync_advanced_from_simple();
                            if self.save_config() {
                                self.toast_ok("配置已保存");
                            } else {
                                self.toast_error("保存配置失败");
                            }
                        }
                    });
                    ui.add_space(6.0);
                    inner_card_full_width(ui, |ui| {
                            form_row(ui, "服务器", 88.0, |ui| {
                                centered_text_edit(ui, text_width, 28.0, &mut self.simple_form.server_addr, None);
                            });
                            form_row(ui, "端口", 88.0, |ui| {
                                centered_text_edit(ui, compact_text_width, 28.0, &mut self.simple_form.server_port, None);
                            });
                            form_row(ui, "Token", 88.0, |ui| {
                                centered_text_edit(ui, token_width, 28.0, &mut self.simple_form.auth_token, None);
                            });
                        });

                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("映射配置")
                            .size(17.0)
                            .strong()
                            .color(egui::Color32::from_rgb(103, 78, 104)),
                    );
                    ui.add_space(6.0);
                    inner_card_full_width(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                form_row(ui, "名称", 56.0, |ui| {
                                    centered_text_edit(ui, half_width, 28.0, &mut self.simple_form.proxy_name, None);
                                });
                                ui.add_space(10.0);
                                form_row(ui, "类型", 48.0, |ui| {
                                    proxy_type_dropdown(
                                        ui,
                                        half_width.min(140.0),
                                        28.0,
                                        &mut self.simple_form.proxy_type,
                                    );
                                });
                            });
                            form_row(ui, "本地IP", 56.0, |ui| {
                                centered_text_edit(
                                    ui,
                                    text_width.min(300.0),
                                    28.0,
                                    &mut self.simple_form.local_ip,
                                    None,
                                );
                            });
                            ui.horizontal_wrapped(|ui| {
                                form_row(ui, "本地端口", 56.0, |ui| {
                                    centered_text_edit(ui, half_width, 28.0, &mut self.simple_form.local_port, None);
                                });
                                ui.add_space(10.0);
                                form_row(ui, "远端端口", 56.0, |ui| {
                                    centered_text_edit(ui, half_width, 28.0, &mut self.simple_form.remote_port, None);
                                });
                            });
                        });
                }
                ConfigTab::Advanced => {
                    let parse = self.config_parse_status();
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new("高级配置")
                                .size(17.0)
                                .strong()
                                .color(egui::Color32::from_rgb(103, 78, 104)),
                        );
                        ui.add_space(8.0);

                        if tinted_button(
                            ui,
                            "加载配置",
                            [86.0, 30.0],
                            egui::Color32::from_rgb(232, 242, 255),
                            egui::Color32::from_rgb(190, 214, 245),
                            egui::Color32::from_rgb(46, 92, 146),
                        )
                        .clicked()
                        {
                            if self.load_config() {
                                self.toast_ok("配置已加载");
                            } else {
                                self.toast_error("加载配置失败");
                            }
                        }
                        let save_btn = tinted_button_enabled(
                            ui,
                            "保存配置",
                            [86.0, 30.0],
                            parse.is_ok(),
                            egui::Color32::from_rgb(255, 232, 244),
                            egui::Color32::from_rgb(242, 190, 220),
                            egui::Color32::from_rgb(122, 57, 93),
                        );
                        if save_btn.clicked() {
                            if self.save_config() {
                                self.toast_ok("配置已保存");
                            } else {
                                self.toast_error("保存配置失败");
                            }
                        }

                        ui.add_space(10.0);
                        let badge_size = [76.0, 30.0];
                        match &parse {
                            Ok(()) => status_badge(ui, StatusKind::Ok, "语法正确", badge_size),
                            Err(_) => status_badge(ui, StatusKind::Error, "语法错误", badge_size),
                        }

                        if self.config_dirty {
                            status_badge(ui, StatusKind::Dirty, "未保存", badge_size);
                        } else {
                            status_badge(ui, StatusKind::Saved, "已保存", badge_size);
                        }
                    });

                    if let Err(err) = parse {
                        ui.add_space(8.0);
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(255, 235, 238))
                            .rounding(egui::Rounding::same(12.0))
                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(242, 190, 198)))
                            .inner_margin(egui::Margin::same(10.0))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(format!("TOML 语法错误: {err}"))
                                        .monospace()
                                        .size(13.0)
                                        .color(egui::Color32::from_rgb(156, 62, 78)),
                                );
                            });
                    }
                    ui.add_space(8.0);
                    let edit = ui.add_sized(
                        [ui.available_width(), editor_height],
                        egui::TextEdit::multiline(&mut self.config_text)
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace),
                    );
                    if edit.changed() {
                        self.config_dirty = true;
                    }
                }
            }
        });
    }

    // 日志页：显示缓存日志并支持保存/清空。
    fn render_log_panel(&mut self, ui: &mut egui::Ui, compact: bool, window_height: f32) {
        let log_input_width = if compact { 220.0 } else { 260.0 };
        let log_height = if compact {
            (window_height * 0.26).clamp(180.0, 260.0)
        } else {
            (window_height - 360.0).clamp(220.0, 420.0)
        };

        section_card(ui, "日志", |ui| {
            ui.horizontal_wrapped(|ui| {
                top_inline_label(ui, "保存到", 48.0);
                centered_text_edit(ui, log_input_width, 28.0, &mut self.log_path, None);
                if soft_button(ui, "保存日志", [84.0, 30.0]).clicked() {
                    self.save_logs();
                    self.push_toast_custom(
                        Some("日志".to_string()),
                        "已保存",
                        Duration::from_secs(2),
                        self.toast_style_info(),
                    );
                }
                if soft_button(ui, "清空", [64.0, 30.0]).clicked() {
                    self.logs.clear();
                }
            });

            ui.add_space(10.0);
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(252, 248, 252))
                .rounding(egui::Rounding::same(12.0))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(232, 220, 232)))
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.set_min_height(log_height);
                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                for line in self.logs.iter() {
                                    let color = log_line_color(line);
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(line)
                                                .monospace()
                                                .size(13.0)
                                                .color(color),
                                        )
                                        .wrap(false),
                                    );
                                }
                            });
                        });
                });
        });
    }
}

impl eframe::App for FrpcLauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 每帧都尽量刷新日志与进程状态，确保 UI 显示“实时”。
        self.drain_log_events();
        self.poll_child();
        let running = self.running_state();
        let window_width = ctx.available_rect().width();
        let window_height = ctx.available_rect().height();
        let compact = window_width < 1120.0;
        let narrow = window_width < 1180.0;

        egui::TopBottomPanel::top("top")
            .exact_height(if narrow { 98.0 } else { 76.0 })
            .frame(
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(248, 242, 250))
                    .inner_margin(egui::Margin::symmetric(12.0, 4.0)),
            )
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);

                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true), |ui| {
                        let input_width = if narrow {
                            260.0
                        } else if compact {
                            300.0
                        } else {
                            370.0
                        };
                        top_inline_label(ui, "frpc 路径", 58.0);
                        let resp = centered_text_edit(
                            ui,
                            input_width,
                            28.0,
                            &mut self.frpc_exe_path,
                            Some("./frpc/frpc.exe"),
                        );
                        if resp.changed() {
                            self.maybe_autofill_config_path();
                        }

                        ui.add_space(8.0);
                        top_inline_label(ui, "配置文件", 58.0);
                        centered_text_edit(
                            ui,
                            input_width,
                            28.0,
                            &mut self.config_path,
                            Some("./frpc/frpc.toml"),
                        );
                    });

                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true), |ui| {
                        let action_button_size = if narrow { [70.0, 30.0] } else { [76.0, 30.0] };
                        draw_status_badge(ui, running, action_button_size);

                        if let Some(code) = self.last_exit_code {
                            subtle_chip(ui, format!("上次退出码: {code}"));
                        }
                        if let Some(err) = self.last_error.as_deref() {
                            subtle_chip(ui, format!("错误: {err}"));
                        }

                        ui.add_space(10.0);

                        if filled_button(
                            ui,
                            "启动",
                            egui::Color32::from_rgb(91, 201, 137),
                            egui::Color32::WHITE,
                            action_button_size,
                        )
                        .clicked()
                        {
                            self.start_frpc();
                        }
                        if filled_button(
                            ui,
                            "停止",
                            egui::Color32::from_rgb(232, 92, 105),
                            egui::Color32::WHITE,
                            action_button_size,
                        )
                        .clicked()
                        {
                            self.stop_frpc();
                        }
                        if filled_button(
                            ui,
                            "重启",
                            egui::Color32::from_rgb(255, 185, 92),
                            egui::Color32::WHITE,
                            action_button_size,
                        )
                        .clicked()
                        {
                            self.restart_frpc();
                        }
                    });
                });
            });

        egui::TopBottomPanel::top("main_tabs")
            .exact_height(40.0)
            .frame(
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(255, 252, 254))
                    .inner_margin(egui::Margin::symmetric(12.0, 4.0)),
            )
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let config = tab_button(ui, "配置", self.main_tab == MainTab::Config);
                    if config.clicked() && self.main_tab != MainTab::Config {
                        self.main_tab = MainTab::Config;
                        self.main_tab_scroll_gen = self.main_tab_scroll_gen.wrapping_add(1);
                    }
                    let logs = tab_button(ui, "日志", self.main_tab == MainTab::Logs);
                    if logs.clicked() && self.main_tab != MainTab::Logs {
                        self.main_tab = MainTab::Logs;
                        self.main_tab_scroll_gen = self.main_tab_scroll_gen.wrapping_add(1);
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(255, 252, 254))
                    .inner_margin(egui::Margin::symmetric(14.0, 14.0)),
            )
            .show(ctx, |ui| {
                match self.main_tab {
                    MainTab::Config => {
                        egui::ScrollArea::vertical()
                            .id_source(("config_scroll", self.main_tab_scroll_gen))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                self.render_config_panel(ui, compact, window_height);
                            });
                    }
                    MainTab::Logs => {
                        egui::ScrollArea::vertical()
                            .id_source(("logs_scroll", self.main_tab_scroll_gen))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                self.render_log_panel(ui, compact, window_height);
                            });
                    }
                }
            });

        self.show_toasts(ctx);

        // 持续重绘用于实现日志实时刷新；需要降占用时可改为按事件触发或 request_repaint_after。
        ctx.request_repaint();
    }
}

fn app_base_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|v| v.to_path_buf()))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

// 相对路径解析：
// - 优先使用“存在的候选路径”
// - 兼容双击运行与 cargo run（current_dir 与 exe_dir 可能不同）
fn resolve_under_app(app_dir: &Path, p: &str) -> PathBuf {
    let pbuf = PathBuf::from(p);
    if pbuf.is_absolute() {
        return pbuf;
    }

    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }

    candidates.push(app_dir.to_path_buf());
    if let Some(parent) = app_dir.parent() {
        candidates.push(parent.to_path_buf());
        if let Some(grand_parent) = parent.parent() {
            candidates.push(grand_parent.to_path_buf());
        }
    }

    for base in &candidates {
        let candidate = base.join(&pbuf);
        if candidate.exists() {
            return candidate;
        }
    }

    candidates
        .into_iter()
        .next()
        .unwrap_or_else(|| app_dir.to_path_buf())
        .join(pbuf)
}

fn path_to_string(app_dir: &Path, abs: &Path) -> String {
    if let Ok(rel) = abs.strip_prefix(app_dir) {
        let rel_str = rel.to_string_lossy().to_string();
        if rel_str.is_empty() {
            ".".to_string()
        } else {
            format!("./{rel_str}")
        }
    } else {
        abs.to_string_lossy().to_string()
    }
}

fn escape_toml_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\"', "\\\"")
}

// 读取 UTF-8 文本文件到 String。
fn read_text_file(path: &Path) -> std::io::Result<String> {
    fs::read_to_string(path)
}

// 后台读取线程：
// - 循环 read_line 直到 EOF
// - 对每行去除 \r\n，再通过通道发送到 UI 线程
// - is_err 用于区分 stdout/stderr
fn spawn_reader_thread<R: BufRead + Send + 'static>(mut reader: R, tx: mpsc::Sender<LogEvent>, is_err: bool) {
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let s = strip_ansi_and_control(line.trim_end_matches(['\r', '\n']));
                    let _ = tx.send(if is_err { LogEvent::Stderr(s) } else { LogEvent::Stdout(s) });
                }
                Err(err) => {
                    let _ = tx.send(LogEvent::Error(err.to_string()));
                    break;
                }
            }
        }
    });
}

fn strip_ansi_and_control(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if let Some('[') = chars.peek().copied() {
                chars.next();
                while let Some(next) = chars.next() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
                continue;
            }
            continue;
        }

        if ch.is_control() && ch != '\t' {
            continue;
        }

        out.push(ch);
    }

    out
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    if let Some(font_bytes) = load_windows_cjk_font_bytes() {
        fonts.font_data.insert("cjk".to_string(), egui::FontData::from_owned(font_bytes));

        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.insert(0, "cjk".to_string());
        }
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            family.insert(0, "cjk".to_string());
        }
    }

    ctx.set_fonts(fonts);
}

fn setup_visuals(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.interact_size = egui::vec2(34.0, 26.0);
    style.spacing.scroll.floating = true;
    style.spacing.scroll.bar_width = 8.0;
    style.visuals.window_rounding = egui::Rounding::same(12.0);
    style.visuals.menu_rounding = egui::Rounding::same(10.0);
    style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);
    style.visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
    style.visuals.widgets.hovered.rounding = egui::Rounding::same(8.0);
    style.visuals.widgets.active.rounding = egui::Rounding::same(8.0);
    style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(250, 244, 250);
    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(246, 231, 244);
    style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(240, 218, 238);
    style.visuals.extreme_bg_color = egui::Color32::from_rgb(255, 252, 254);
    style.visuals.panel_fill = egui::Color32::from_rgb(255, 252, 254);
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(13.5, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(13.5, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(13.0, egui::FontFamily::Monospace),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(24.0, egui::FontFamily::Proportional),
    );
    ctx.set_style(style);
}

fn filled_button(
    ui: &mut egui::Ui,
    text: &str,
    fill: egui::Color32,
    text_color: egui::Color32,
    size: [f32; 2],
) -> egui::Response {
    ui.add_sized(
        size,
        egui::Button::new(egui::RichText::new(text).size(13.5).color(text_color).strong())
            .fill(fill)
            .stroke(egui::Stroke::NONE)
            .rounding(egui::Rounding::same(8.0)),
    )
}

fn soft_button(ui: &mut egui::Ui, text: &str, size: [f32; 2]) -> egui::Response {
    ui.add_sized(
        size,
        egui::Button::new(
            egui::RichText::new(text)
                .size(13.5)
                .color(egui::Color32::from_rgb(84, 73, 89))
                .strong(),
        )
            .fill(egui::Color32::from_rgb(250, 245, 250))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(232, 222, 232)))
            .rounding(egui::Rounding::same(8.0)),
    )
}

fn tinted_button(
    ui: &mut egui::Ui,
    text: &str,
    size: [f32; 2],
    fill: egui::Color32,
    stroke: egui::Color32,
    text_color: egui::Color32,
) -> egui::Response {
    ui.add_sized(
        size,
        egui::Button::new(egui::RichText::new(text).size(13.5).color(text_color).strong())
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke))
            .rounding(egui::Rounding::same(8.0)),
    )
}

fn tinted_button_enabled(
    ui: &mut egui::Ui,
    text: &str,
    size: [f32; 2],
    enabled: bool,
    fill: egui::Color32,
    stroke: egui::Color32,
    text_color: egui::Color32,
) -> egui::Response {
    let button = egui::Button::new(
        egui::RichText::new(text)
            .size(13.5)
            .color(if enabled {
                text_color
            } else {
                egui::Color32::from_rgb(150, 138, 156)
            })
            .strong(),
    )
    .fill(if enabled {
        fill
    } else {
        egui::Color32::from_rgb(246, 241, 246)
    })
    .stroke(egui::Stroke::new(
        1.0,
        if enabled {
            stroke
        } else {
            egui::Color32::from_rgb(236, 227, 236)
        },
    ))
    .rounding(egui::Rounding::same(8.0));

    ui.add_enabled_ui(enabled, |ui| ui.add_sized(size, button))
        .inner
}

fn tab_button(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let (fill, text_color) = if selected {
        (
            egui::Color32::from_rgb(255, 201, 226),
            egui::Color32::from_rgb(112, 62, 93),
        )
    } else {
        (
            egui::Color32::from_rgb(247, 239, 246),
            egui::Color32::from_rgb(108, 96, 112),
        )
    };

    ui.add_sized(
        [94.0, 28.0],
        egui::Button::new(egui::RichText::new(text).size(13.0).color(text_color).strong())
            .fill(fill)
            .stroke(egui::Stroke::NONE)
            .rounding(egui::Rounding::same(8.0)),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StatusKind {
    Ok,
    Saved,
    Dirty,
    Error,
}

fn status_badge(ui: &mut egui::Ui, kind: StatusKind, text: &str, size: [f32; 2]) {
    let (fill, stroke, color) = match kind {
        StatusKind::Ok => (
            egui::Color32::from_rgb(233, 248, 241),
            egui::Color32::from_rgb(176, 226, 198),
            egui::Color32::from_rgb(46, 110, 78),
        ),
        StatusKind::Saved => (
            egui::Color32::from_rgb(233, 248, 241),
            egui::Color32::from_rgb(176, 226, 198),
            egui::Color32::from_rgb(46, 110, 78),
        ),
        StatusKind::Dirty => (
            egui::Color32::from_rgb(255, 246, 230),
            egui::Color32::from_rgb(238, 214, 165),
            egui::Color32::from_rgb(138, 92, 20),
        ),
        StatusKind::Error => (
            egui::Color32::from_rgb(255, 235, 238),
            egui::Color32::from_rgb(242, 190, 198),
            egui::Color32::from_rgb(156, 62, 78),
        ),
    };

    egui::Frame::none()
        .fill(fill)
        .rounding(egui::Rounding::same(8.0))
        .stroke(egui::Stroke::new(1.0, stroke))
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(size[0], size[1]),
                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| {
                    ui.label(egui::RichText::new(text).size(13.0).color(color).strong());
                },
            );
        });
}

fn draw_status_badge(ui: &mut egui::Ui, running: bool, size: [f32; 2]) {
    let (text, fill) = if running {
        ("运行中", egui::Color32::from_rgb(91, 201, 137))
    } else {
        ("已停止", egui::Color32::from_rgb(232, 92, 105))
    };

    egui::Frame::none()
        .fill(fill)
        .rounding(egui::Rounding::same(8.0))
        .stroke(egui::Stroke::NONE)
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(size[0], size[1]),
                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| {
                    ui.label(
                        egui::RichText::new(text)
                            .size(13.5)
                            .color(egui::Color32::WHITE)
                            .strong(),
                    );
                },
            );
        });
}

fn subtle_chip(ui: &mut egui::Ui, text: String) {
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(247, 239, 246))
        .rounding(egui::Rounding::same(999.0))
        .inner_margin(egui::Margin::symmetric(9.0, 4.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(13.0)
                    .color(egui::Color32::from_rgb(98, 88, 104)),
            );
        });
}

fn section_card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    let width = ui.available_width();
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(255, 247, 252))
        .rounding(egui::Rounding::same(14.0))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(235, 221, 235)))
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.set_min_width(width);
            ui.label(
                egui::RichText::new(title)
                    .size(21.0)
                    .strong()
                    .color(egui::Color32::from_rgb(94, 71, 95)),
            );
            ui.add_space(6.0);
            add_contents(ui);
        });
}

fn inner_card_full_width(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let width = ui.available_width();
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(252, 246, 250))
        .rounding(egui::Rounding::same(10.0))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(238, 226, 237)))
        .inner_margin(egui::Margin::same(10.0))
        .show(ui, |ui| {
            ui.set_min_width(width);
            add_contents(ui);
        });
}

fn form_row(
    ui: &mut egui::Ui,
    label: &str,
    label_width: f32,
    add_field: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(label_width, 28.0),
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| {
                ui.label(
                    egui::RichText::new(label)
                        .size(13.0)
                        .strong()
                        .color(egui::Color32::from_rgb(100, 88, 105)),
                );
            },
        );
        add_field(ui);
    });
}

fn top_inline_label(ui: &mut egui::Ui, text: &str, width: f32) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 28.0),
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(13.0)
                    .strong()
                    .color(egui::Color32::from_rgb(78, 69, 84)),
            );
        },
    );
}

fn centered_text_edit(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    value: &mut String,
    hint: Option<&str>,
) -> egui::Response {
    let line_height = ui.text_style_height(&egui::TextStyle::Body);
    let vertical_padding = ((height - line_height) / 2.0).max(2.0);

    let mut edit = egui::TextEdit::singleline(value)
        .desired_width(width)
        .margin(egui::vec2(8.0, vertical_padding));
    if let Some(h) = hint {
        edit = edit.hint_text(h);
    }
    ui.add_sized([width, height], edit)
}

// 简单配置中的映射类型仅允许使用 frpc 常见的 tcp/udp。
fn proxy_type_dropdown(ui: &mut egui::Ui, width: f32, height: f32, value: &mut String) -> egui::Response {
    *value = normalize_proxy_type(value);
    let selected = value.to_uppercase();

    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            egui::ComboBox::from_id_source("simple_proxy_type")
                .selected_text(selected)
                .width(width)
                .show_ui(ui, |ui| {
                    ui.selectable_value(value, "tcp".to_string(), "TCP");
                    ui.selectable_value(value, "udp".to_string(), "UDP");
                })
                .response
        },
    )
    .inner
}

fn normalize_proxy_type(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "udp" => "udp".to_string(),
        _ => "tcp".to_string(),
    }
}

fn log_line_color(line: &str) -> egui::Color32 {
    if line.starts_with("[[Error]]") || line.starts_with("[ERR]") {
        egui::Color32::from_rgb(205, 78, 96)
    } else if line.starts_with("[[Exited]]") {
        egui::Color32::from_rgb(160, 107, 70)
    } else if line.starts_with("[[Info]]") {
        egui::Color32::from_rgb(92, 130, 192)
    } else if line.starts_with("[OUT]") {
        if line.contains("login to server success")
            || line.contains("start proxy success")
            || line.contains("proxy added")
        {
            egui::Color32::from_rgb(76, 153, 108)
        } else if line.contains("try to connect to server") {
            egui::Color32::from_rgb(198, 140, 62)
        } else {
            egui::Color32::from_rgb(86, 78, 92)
        }
    } else {
        egui::Color32::from_rgb(86, 78, 92)
    }
}

fn load_windows_cjk_font_bytes() -> Option<Vec<u8>> {
    let candidates = [
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\msyhl.ttf",
        r"C:\Windows\Fonts\simsun.ttf",
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simsun.ttc",
    ];

    for path in candidates {
        if let Ok(bytes) = fs::read(path) {
            return Some(bytes);
        }
    }

    None
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 760.0])
            .with_min_inner_size([980.0, 620.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Bad frpc Launcher",
        options,
        Box::new(|cc| Box::new(FrpcLauncherApp::new(cc))),
    )
}
