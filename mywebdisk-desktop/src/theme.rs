//! Godot NeonStyle 配色与尺寸（800×1040 登录窗）
use std::fs;

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Pos2, RichText, Stroke,
    Vec2, Visuals,
};

pub const WINDOW_W: f32 = 380.0;
pub const WINDOW_H: f32 = 310.0;

/// 登录后会话页窗口尺寸（相对登录页 2 倍）
pub const SESSIONS_WINDOW_W: f32 = WINDOW_W * 2.0;
pub const SESSIONS_WINDOW_H: f32 = WINDOW_H * 2.0;

pub const CYAN: Color32 = Color32::from_rgb(0, 240, 255);
pub const CYAN_SOFT: Color32 = Color32::from_rgb(102, 245, 255);
pub const CYAN_DIM: Color32 = Color32::from_rgba_premultiplied(0, 240, 255, 115);
pub const ORANGE: Color32 = Color32::from_rgb(255, 107, 0);
pub const BG: Color32 = Color32::from_rgb(10, 14, 23);
pub const INPUT_BG: Color32 = Color32::from_rgb(8, 14, 22);
pub const STATUS_HINT: Color32 = Color32::from_rgb(89, 166, 191);
pub const STATUS_SUCCESS: Color32 = Color32::from_rgb(102, 255, 140);
pub const STATUS_ERROR: Color32 = Color32::from_rgba_premultiplied(255, 107, 0, 242);


fn try_load_font(paths: &[&str]) -> Option<Vec<u8>> {
    for path in paths {
        if let Ok(bytes) = fs::read(path) {
            return Some(bytes);
        }
    }
    None
}

fn apply_cjk_font(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/System/Library/Fonts/Supplemental/Songti.ttc",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ]
    } else if cfg!(target_os = "windows") {
        &[
            "C:/Windows/Fonts/msyh.ttc",
            "C:/Windows/Fonts/msyh.ttf",
            "C:/Windows/Fonts/simhei.ttf",
            "C:/Windows/Fonts/simsun.ttc",
        ]
    } else {
        &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        ]
    };

    if let Some(bytes) = try_load_font(candidates) {
        fonts
            .font_data
            .insert("cjk_fallback".to_owned(), FontData::from_owned(bytes).into());

        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "cjk_fallback".to_owned());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push("cjk_fallback".to_owned());

        ctx.set_fonts(fonts);
    }
}

pub fn apply(ctx: &egui::Context) {
    apply_cjk_font(ctx);
    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = BG;
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = Color32::from_rgb(12, 20, 30);
    visuals.widgets.noninteractive.bg_fill = INPUT_BG;
    visuals.widgets.inactive.bg_fill = INPUT_BG;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(14, 24, 36);
    visuals.widgets.active.bg_fill = Color32::from_rgb(16, 28, 42);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, CYAN_DIM);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, CYAN_SOFT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.5, CYAN);
    visuals.selection.bg_fill = Color32::from_rgba_premultiplied(0, 180, 200, 80);
    visuals.selection.stroke = Stroke::new(1.0, CYAN);
    visuals.override_text_color = Some(Color32::from_rgb(217, 242, 255));
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(0.0, 10.0);
    style.spacing.button_padding = egui::vec2(18.0, 12.0);
    ctx.set_style(style);
}

pub fn hud_label(text: &str) -> RichText {
    RichText::new(text).size(10.0).color(CYAN_DIM)
}

pub fn input_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(INPUT_BG)
        .corner_radius(CornerRadius::same(4))
        .stroke(Stroke::new(1.5, Color32::from_rgba_premultiplied(0, 180, 200, 140)))
        .inner_margin(egui::Margin::symmetric(6, 5))
}

pub fn primary_button<'a>(ui: &mut egui::Ui, text: &'a str, enabled: bool) -> egui::Response {
    primary_button_scaled(ui, text, enabled, 1.0)
}

pub fn primary_button_scaled<'a>(
    ui: &mut egui::Ui,
    text: &'a str,
    enabled: bool,
    scale: f32,
) -> egui::Response {
    let fill = if enabled {
        Color32::from_rgba_premultiplied(0, 80, 95, 180)
    } else {
        Color32::from_rgba_premultiplied(0, 40, 50, 100)
    };
    let stroke = if enabled { CYAN } else { CYAN_DIM };
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    ui.add(
        egui::Button::new(RichText::new(text).size(13.0 * scale).color(CYAN_SOFT))
            .fill(fill)
            .stroke(Stroke::new(2.0, stroke))
            .min_size(egui::vec2(120.0 * scale, 28.0 * scale))
            .corner_radius(CornerRadius::same((4.0 * scale) as u8))
            .sense(sense),
    )
}

pub fn lang_button(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let (fill, text_color, stroke) = if active {
        (
            Color32::from_rgba_premultiplied(0, 140, 165, 165),
            ORANGE,
            Stroke::new(1.5, ORANGE),
        )
    } else {
        (
            Color32::from_rgba_premultiplied(15, 25, 38, 200),
            CYAN_DIM,
            Stroke::new(1.0, Color32::from_rgba_premultiplied(0, 180, 200, 90)),
        )
    };
    ui.add(
        egui::Button::new(RichText::new(label).size(10.0).color(text_color))
            .fill(fill)
            .stroke(stroke)
            .min_size(egui::vec2(28.0, 18.0))
            .corner_radius(CornerRadius::same(4)),
    )
}

pub fn input_font() -> FontId {
    FontId::proportional(12.0)
}

// macOS Finder 风格（深色）
pub const MAC_PANEL: Color32 = Color32::from_rgb(28, 28, 30);
pub const MAC_TOOLBAR: Color32 = Color32::from_rgb(40, 40, 42);
/// 文件列表行悬停：圆角浅绿色高光
pub const MAC_ROW_HOVER_GREEN: Color32 = Color32::from_rgba_premultiplied(88, 200, 130, 42);
pub const MAC_TEXT: Color32 = Color32::from_rgb(235, 235, 235);
pub const MAC_TEXT_DIM: Color32 = Color32::from_rgb(152, 152, 156);
pub const MAC_BORDER: Color32 = Color32::from_rgb(58, 58, 60);
pub const MAC_BTN_BG: Color32 = Color32::from_rgb(62, 62, 66);
pub const MAC_BTN_HOVER: Color32 = Color32::from_rgb(88, 88, 94);
pub const MAC_BTN_ACTIVE: Color32 = Color32::from_rgb(48, 132, 255);
pub const MAC_BTN_BORDER: Color32 = Color32::from_rgb(98, 98, 104);
pub const NEON_POPUP_BG: Color32 = Color32::from_rgb(10, 26, 34);
pub const NEON_POPUP_BORDER: Color32 = Color32::from_rgb(44, 222, 210);
pub const NEON_POPUP_TEXT: Color32 = Color32::from_rgb(184, 255, 246);
pub const NEON_ACCENT: Color32 = Color32::from_rgb(64, 232, 219);
pub const NEON_MENU_BG: Color32 = Color32::from_rgb(0, 146, 163);
pub const NEON_MENU_TEXT_BLUE: Color32 = Color32::from_rgb(134, 214, 255);

/// 资源管理器工具栏图标按钮（尺寸与图标一致，悬停/按下变色；`enabled=false` 时灰色不可点）
pub fn mac_toolbar_button(
    ui: &mut egui::Ui,
    icon: &str,
    tooltip: &str,
    enabled: bool,
) -> egui::Response {
    const BTN: f32 = 22.0;
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(BTN, BTN), sense);

    let fill = if !enabled {
        Color32::from_rgb(48, 48, 50)
    } else if response.is_pointer_button_down_on() {
        MAC_BTN_ACTIVE
    } else if response.hovered() {
        MAC_BTN_HOVER
    } else {
        MAC_BTN_BG
    };

    let stroke_color = if !enabled {
        MAC_BORDER
    } else if response.hovered() || response.is_pointer_button_down_on() {
        Color32::from_rgb(120, 180, 255)
    } else {
        MAC_BTN_BORDER
    };

    let text_color = if enabled { MAC_TEXT } else { MAC_TEXT_DIM };

    let p = ui.painter();
    p.rect_filled(rect, 5.0, fill);
    p.rect_stroke(
        rect,
        5.0,
        Stroke::new(1.0, stroke_color),
        egui::StrokeKind::Inside,
    );
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        FontId::proportional(14.0),
        text_color,
    );

    response.on_hover_text(tooltip)
}

/// 资源管理器文件行「操作」按钮：浅蓝胶囊样式
pub fn finder_action_button<'a>(label: &'a str, row_hovered: bool) -> egui::Button<'a> {
    let (fill, stroke, text_color) = if row_hovered {
        (
            Color32::from_rgba_premultiplied(90, 155, 255, 150),
            Stroke::new(1.0, Color32::from_rgb(130, 185, 255)),
            Color32::WHITE,
        )
    } else {
        (
            Color32::from_rgba_premultiplied(70, 130, 230, 95),
            Stroke::new(1.0, Color32::from_rgb(110, 165, 245)),
            Color32::from_rgb(230, 242, 255),
        )
    };
    egui::Button::new(RichText::new(label).size(10.5).color(text_color))
    .fill(fill)
    .stroke(stroke)
    .min_size(egui::vec2(44.0, 18.0))
    .corner_radius(CornerRadius::same(5))
}

pub fn neon_popup_frame() -> egui::Frame {
    egui::Frame::popup(&egui::Style::default())
        .fill(NEON_POPUP_BG)
        .stroke(Stroke::new(1.2, NEON_POPUP_BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(8, 8))
}

pub fn neon_modal_frame() -> egui::Frame {
    egui::Frame::window(&egui::Style::default())
        .fill(Color32::from_rgb(8, 22, 30))
        .stroke(Stroke::new(1.5, NEON_ACCENT))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(12, 10))
}

pub fn neon_menu_button(label: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label.into()).size(11.5).color(NEON_POPUP_TEXT))
        .fill(Color32::from_rgba_premultiplied(0, 146, 163, 92))
        .stroke(Stroke::new(1.0, NEON_ACCENT))
        .corner_radius(CornerRadius::same(6))
        .min_size(egui::vec2(120.0, 24.0))
}

pub fn neon_primary_button(label: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(label.into())
            .size(12.0)
            .color(Color32::from_rgb(232, 255, 255)),
    )
        .fill(Color32::from_rgba_premultiplied(0, 150, 168, 170))
        .stroke(Stroke::new(1.3, NEON_ACCENT))
        .corner_radius(CornerRadius::same(6))
        .min_size(egui::vec2(78.0, 26.0))
}

pub fn neon_secondary_button(label: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label.into()).size(12.0).color(NEON_POPUP_TEXT))
        .fill(Color32::from_rgba_premultiplied(0, 108, 122, 150))
        .stroke(Stroke::new(1.0, NEON_ACCENT))
        .corner_radius(CornerRadius::same(6))
        .min_size(egui::vec2(78.0, 26.0))
}

pub fn neon_input_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgb(5, 30, 38))
        .stroke(Stroke::new(1.2, NEON_ACCENT))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(8, 6))
}

/// 统一右键菜单配色：新建/文件夹/文件使用同一蓝绿色调，避免重叠色层。
pub fn apply_neon_menu_visuals(ui: &mut egui::Ui) {
    let visuals = &mut ui.style_mut().visuals;
    visuals.override_text_color = Some(NEON_MENU_TEXT_BLUE);
    visuals.widgets.noninteractive.bg_fill = NEON_MENU_BG;
    visuals.widgets.inactive.bg_fill = NEON_MENU_BG;
    visuals.widgets.hovered.bg_fill = NEON_MENU_BG;
    visuals.widgets.active.bg_fill = NEON_MENU_BG;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, NEON_ACCENT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, NEON_ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, NEON_ACCENT);
}

/// 资源管理器「关闭会话」按钮：矢量红色 X，悬停时 macOS 风格红底白叉
pub fn mac_toolbar_close_button(ui: &mut egui::Ui, tooltip: &str, enabled: bool) -> egui::Response {
    const BTN: f32 = 22.0;
    const RADIUS: f32 = 9.0;

    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(BTN), sense);
    let center = rect.center();
    let p = ui.painter();

    let (bg, x_color, stroke_w) = if !enabled {
        (
            Color32::from_rgb(48, 48, 50),
            Color32::from_rgb(110, 110, 114),
            1.4,
        )
    } else if response.is_pointer_button_down_on() {
        (
            Color32::from_rgb(200, 58, 52),
            Color32::WHITE,
            2.0,
        )
    } else if response.hovered() {
        (
            Color32::from_rgb(255, 88, 82),
            Color32::WHITE,
            2.0,
        )
    } else {
        (
            Color32::from_rgb(58, 58, 62),
            Color32::from_rgb(255, 108, 102),
            1.8,
        )
    };

    p.circle_filled(center, RADIUS, bg);
    if enabled {
        let ring = if response.hovered() || response.is_pointer_button_down_on() {
            Color32::from_rgb(255, 140, 135)
        } else {
            Color32::from_rgb(120, 70, 68)
        };
        p.circle_stroke(center, RADIUS, Stroke::new(0.8, ring));
    }

    let arm = RADIUS * 0.52;
    let a = Pos2::new(center.x - arm, center.y - arm);
    let b = Pos2::new(center.x + arm, center.y + arm);
    let c = Pos2::new(center.x - arm, center.y + arm);
    let d = Pos2::new(center.x + arm, center.y - arm);
    let stroke = Stroke::new(stroke_w, x_color);
    p.line_segment([a, b], stroke);
    p.line_segment([c, d], stroke);

    response.on_hover_text(tooltip)
}
