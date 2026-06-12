use crate::config::normalize_proxy_type;
use std::fs;

pub(crate) fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    if let Some(font_bytes) = load_windows_cjk_font_bytes() {
        fonts
            .font_data
            .insert("cjk".to_string(), egui::FontData::from_owned(font_bytes));

        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.insert(0, "cjk".to_string());
        }
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            family.insert(0, "cjk".to_string());
        }
    }

    ctx.set_fonts(fonts);
}

pub(crate) fn setup_visuals(ctx: &egui::Context) {
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

pub(crate) fn filled_button(
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

pub(crate) fn soft_button(ui: &mut egui::Ui, text: &str, size: [f32; 2]) -> egui::Response {
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

pub(crate) fn tinted_button(
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

pub(crate) fn tinted_button_enabled(
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

pub(crate) fn tab_button(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
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
pub(crate) enum StatusKind {
    Ok,
    Saved,
    Dirty,
    Error,
}

pub(crate) fn status_badge(ui: &mut egui::Ui, kind: StatusKind, text: &str, size: [f32; 2]) {
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

pub(crate) fn draw_status_badge(ui: &mut egui::Ui, running: bool, size: [f32; 2]) {
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

pub(crate) fn subtle_chip(ui: &mut egui::Ui, text: String) {
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

pub(crate) fn section_card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
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

pub(crate) fn inner_card_full_width(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
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

pub(crate) fn form_row(
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

pub(crate) fn top_inline_label(ui: &mut egui::Ui, text: &str, width: f32) {
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

pub(crate) fn centered_text_edit(
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

pub(crate) fn proxy_type_dropdown(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    value: &mut String,
) -> egui::Response {
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

pub(crate) fn log_line_color(line: &str) -> egui::Color32 {
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
