use super::{ConfigTab, FrpcLauncherApp, MainTab};
use crate::ui::{
    centered_text_edit, draw_status_badge, filled_button, form_row, inner_card_full_width,
    log_line_color, proxy_type_dropdown, section_card, soft_button, status_badge, subtle_chip,
    tab_button, tinted_button, tinted_button_enabled, top_inline_label, StatusKind,
};
use std::time::{Duration, Instant};

impl FrpcLauncherApp {
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
                    let response = egui::Frame::none()
                        .fill(toast.style.fill)
                        .stroke(egui::Stroke::new(1.0, toast.style.stroke))
                        .rounding(egui::Rounding::same(10.0))
                        .inner_margin(egui::Margin::same(10.0))
                        .show(ui, |ui| {
                            ui.set_min_width(280.0);

                            if let Some(title) = toast.title.as_deref() {
                                ui.label(
                                    egui::RichText::new(title)
                                        .size(toast.style.title_size)
                                        .color(toast.style.title_color)
                                        .strong(),
                                );
                                ui.add_space(4.0);
                            }

                            let mut rich = egui::RichText::new(&toast.message)
                                .size(toast.style.body_size)
                                .color(toast.style.text_color);
                            if toast.style.monospace {
                                rich = rich.monospace();
                            }
                            ui.add(egui::Label::new(rich).wrap(true));
                        })
                        .response;

                    if response.clicked() {
                        dismiss.push(toast.id);
                    }
                }
            });

        if !dismiss.is_empty() {
            self.toasts.retain(|t| !dismiss.contains(&t.id));
        }
    }

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
                            centered_text_edit(
                                ui,
                                text_width,
                                28.0,
                                &mut self.simple_form.server_addr,
                                None,
                            );
                        });
                        form_row(ui, "端口", 88.0, |ui| {
                            centered_text_edit(
                                ui,
                                compact_text_width,
                                28.0,
                                &mut self.simple_form.server_port,
                                None,
                            );
                        });
                        form_row(ui, "Token", 88.0, |ui| {
                            centered_text_edit(
                                ui,
                                token_width,
                                28.0,
                                &mut self.simple_form.auth_token,
                                None,
                            );
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
                                centered_text_edit(
                                    ui,
                                    half_width,
                                    28.0,
                                    &mut self.simple_form.proxy_name,
                                    None,
                                );
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
                                centered_text_edit(
                                    ui,
                                    half_width,
                                    28.0,
                                    &mut self.simple_form.local_port,
                                    None,
                                );
                            });
                            ui.add_space(10.0);
                            form_row(ui, "远端端口", 56.0, |ui| {
                                centered_text_edit(
                                    ui,
                                    half_width,
                                    28.0,
                                    &mut self.simple_form.remote_port,
                                    None,
                                );
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
                            .stroke(egui::Stroke::new(
                                1.0,
                                egui::Color32::from_rgb(242, 190, 198),
                            ))
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

    fn render_about_panel(&mut self, ui: &mut egui::Ui) {
        section_card(ui, "关于", |ui| {
            ui.label(
                egui::RichText::new(&self.app_info.software.name)
                    .size(24.0)
                    .strong()
                    .color(egui::Color32::from_rgb(94, 71, 95)),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("版本 {}", self.app_info.software.version))
                    .size(14.0)
                    .color(egui::Color32::from_rgb(98, 88, 104)),
            );

            if !self.app_info.software.description.trim().is_empty() {
                ui.add_space(10.0);
                inner_card_full_width(ui, |ui| {
                    ui.label(
                        egui::RichText::new(&self.app_info.software.description)
                            .size(13.5)
                            .color(egui::Color32::from_rgb(86, 78, 92)),
                    );
                });
            }

            ui.add_space(10.0);
            inner_card_full_width(ui, |ui| {
                ui.label(
                    egui::RichText::new("作者信息")
                        .size(16.0)
                        .strong()
                        .color(egui::Color32::from_rgb(103, 78, 104)),
                );
                ui.add_space(8.0);
                ui.label(format!("作者: {}", self.app_info.author.name));
                if !self.app_info.author.bio.trim().is_empty() {
                    ui.label(format!("说明: {}", self.app_info.author.bio));
                }
                if !self.app_info.author.contact.trim().is_empty() {
                    ui.label(format!("联系方式: {}", self.app_info.author.contact));
                }
            });

            if !self.app_info.software.homepage.trim().is_empty() {
                ui.add_space(10.0);
                inner_card_full_width(ui, |ui| {
                    ui.label(
                        egui::RichText::new("项目链接")
                            .size(16.0)
                            .strong()
                            .color(egui::Color32::from_rgb(103, 78, 104)),
                    );
                    ui.add_space(8.0);
                    ui.hyperlink_to(
                        &self.app_info.software.homepage,
                        &self.app_info.software.homepage,
                    );
                });
            }
        });
    }
}

impl eframe::App for FrpcLauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

                    ui.with_layout(
                        egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                        |ui| {
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
                        },
                    );

                    ui.with_layout(
                        egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                        |ui| {
                            let action_button_size =
                                if narrow { [70.0, 30.0] } else { [76.0, 30.0] };
                            draw_status_badge(ui, running, action_button_size);
                            subtle_chip(
                                ui,
                                format!(
                                    "{} {}",
                                    self.app_info.software.name, self.app_info.software.version
                                ),
                            );
                            if !self.app_info.author.name.trim().is_empty() {
                                subtle_chip(ui, format!("作者: {}", self.app_info.author.name));
                            }

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
                        },
                    );
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
                    let about = tab_button(ui, "关于", self.main_tab == MainTab::About);
                    if about.clicked() && self.main_tab != MainTab::About {
                        self.main_tab = MainTab::About;
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
            .show(ctx, |ui| match self.main_tab {
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
                MainTab::About => {
                    egui::ScrollArea::vertical()
                        .id_source(("about_scroll", self.main_tab_scroll_gen))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            self.render_about_panel(ui);
                        });
                }
            });

        self.show_toasts(ctx);
        ctx.request_repaint();
    }
}
