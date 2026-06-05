//! GUI 程序：Windows 下不弹出控制台窗口，仅显示界面。
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod file_lock;
mod file_icons;
mod ipc;
mod locale;
mod session_workspace;
mod stream_protocol;
mod theme;
mod ui;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([theme::WINDOW_W, theme::WINDOW_H])
            .with_min_inner_size([theme::WINDOW_W, theme::WINDOW_H])
            .with_max_inner_size([theme::SESSIONS_WINDOW_W, theme::SESSIONS_WINDOW_H])
            .with_resizable(false)
            .with_decorations(false)
            .with_title("MyWebDisk"),
        ..Default::default()
    };
    eframe::run_native(
        "MyWebDisk",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::MyWebDiskApp::new()))
        }),
    )
}
