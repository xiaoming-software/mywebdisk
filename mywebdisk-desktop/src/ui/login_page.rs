use eframe::egui;
use egui::RichText;

use crate::locale::{Lang, Locale};
use crate::theme::{hud_label, input_font, input_frame, lang_button, primary_button, CYAN_SOFT, STATUS_ERROR, STATUS_HINT, STATUS_SUCCESS};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Hint,
    Info,
    Success,
    Error,
}

pub enum LoginAction {
    None,
    Login,
    OpenWebrpc,
    SetLang(Lang),
}

pub struct LoginView<'a> {
    pub locale: &'a Locale,
    pub token: &'a mut String,
    pub passwd: &'a mut String,
    pub status_text: &'a str,
    pub status_kind: StatusKind,
    pub logging_in: bool,
    pub can_login: bool,
}

pub fn draw(ui: &mut egui::Ui, view: LoginView<'_>) -> LoginAction {
    let mut action = LoginAction::None;
    ui.spacing_mut().item_spacing.y = 3.0;
    let full = ui.max_rect();
    let bottom_h = 56.0;
    let content_rect = egui::Rect::from_min_max(
        full.min,
        egui::pos2(full.max.x, (full.max.y - bottom_h).max(full.min.y)),
    );
    let bottom_rect = egui::Rect::from_min_max(
        egui::pos2(full.min.x, content_rect.max.y),
        full.max,
    );

    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
            if lang_button(ui, &view.locale.tr("LANG_ZH"), view.locale.lang == Lang::Zh).clicked() {
                action = LoginAction::SetLang(Lang::Zh);
            }
            if lang_button(ui, &view.locale.tr("LANG_EN"), view.locale.lang == Lang::En).clicked() {
                action = LoginAction::SetLang(Lang::En);
            }
        });

        ui.label(
            RichText::new(view.locale.tr("LOGIN_DESC"))
                .size(14.0)
                .color(CYAN_SOFT),
        );

        ui.label(hud_label(&view.locale.tr("LABEL_TOKEN")));
        input_frame().show(ui, |ui| {
            ui.set_min_height(24.0);
            ui.add(
                egui::TextEdit::singleline(view.token)
                    .font(input_font())
                    .hint_text(view.locale.tr("PLACEHOLDER_TOKEN"))
                    .margin(egui::Margin::symmetric(1, 2))
                    .interactive(!view.logging_in)
                    .desired_width(f32::INFINITY),
            );
        });

        ui.label(hud_label(&view.locale.tr("LABEL_PASSWD")));
        input_frame().show(ui, |ui| {
            ui.set_min_height(24.0);
            ui.add(
                egui::TextEdit::singleline(view.passwd)
                    .font(input_font())
                    .password(true)
                    .hint_text(view.locale.tr("PLACEHOLDER_PASSWD"))
                    .margin(egui::Margin::symmetric(1, 2))
                    .interactive(!view.logging_in)
                    .desired_width(f32::INFINITY),
            );
        });

        let color = match view.status_kind {
            StatusKind::Hint => STATUS_HINT,
            StatusKind::Info => CYAN_SOFT,
            StatusKind::Success => STATUS_SUCCESS,
            StatusKind::Error => STATUS_ERROR,
        };
        ui.add_sized(
            [ui.available_width(), 16.0],
            egui::Label::new(RichText::new(view.status_text).size(14.0).color(color)).truncate(),
        );
    });

    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bottom_rect), |ui| {
        ui.vertical_centered(|ui| {
            let btn_label = if view.logging_in {
                view.locale.tr("BTN_LOGINING")
            } else {
                view.locale.tr("BTN_LOGIN")
            };
            if primary_button(ui, &btn_label, view.can_login).clicked() && view.can_login {
                action = LoginAction::Login;
            }
            if ui
                .link(RichText::new(view.locale.tr("LINK_WEBRPC")).size(9.0).underline().color(CYAN_SOFT))
                .clicked()
            {
                action = LoginAction::OpenWebrpc;
            }
        });
    });

    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && view.can_login {
        action = LoginAction::Login;
    }

    action
}
