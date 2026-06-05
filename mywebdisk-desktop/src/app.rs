use std::collections::{HashMap, HashSet};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;
use egui::ViewportCommand;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ipc::{generate_permission, WebrpcIpc, SEND_DATA_UPLOAD_TIMEOUT_MS};
use crate::locale::Locale;
use crate::theme::{self, SESSIONS_WINDOW_H, SESSIONS_WINDOW_W, WINDOW_H, WINDOW_W};
use crate::ui::login_page::{self, LoginAction, LoginView, StatusKind};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Login,
    Sessions,
}

enum LoginResult {
    Ok,
    Err(String),
}

enum CallbackEvent {
    Connected,
    Data { session_id: u32, payload: Vec<u8> },
    File {
        session_id: u32,
        file_name: Vec<u8>,
        data: Vec<u8>,
    },
    Error(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DownloadPurpose {
    Image,
    Text,
    Office,
    SaveToLocal,
}

struct PendingDownload {
    session_id: u32,
    display_name: String,
    path: String,
    purpose: DownloadPurpose,
    temp_path: Option<PathBuf>,
    save_dest: Option<PathBuf>,
}

/// webrpc SendFile 多桶接收状态（每桶约 10MB，收齐后等 fileSent 再收尾）
struct IncomingFileDownload {
    req_id: String,
    pending: PendingDownload,
    write_path: Option<PathBuf>,
    memory: Vec<u8>,
    bytes_received: u64,
    /// 服务端已发 fileSent（可能与最后一桶在链路上乱序，不能单独作为完成条件）
    file_sent: bool,
    expected_size: Option<u64>,
}

struct PendingDelete {
    session_id: u32,
    api_path: String,
    display_name: String,
}

enum FinderRowAction {
    Download {
        dest_path: PathBuf,
    },
    Delete,
}

#[derive(Clone)]
struct OfficeEditState {
    session_id: u32,
    api_path: String,
    display_name: String,
    temp_path: PathBuf,
    baseline_len: u64,
    baseline_modified: SystemTime,
}

#[derive(Default)]
struct UploadWait {
    prepare_done: bool,
    prepare_ok: bool,
    complete_done: bool,
    complete_ok: bool,
    error: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FinderUploadPhase {
    Preparing,
    Sending,
    Finalizing,
    Success,
    Failed,
}

struct FinderUploadState {
    session_id: u32,
    current_path: String,
    local_path: PathBuf,
    file_name: String,
    total_size: u64,
    req_id: String,
    phase: FinderUploadPhase,
    progress: f32,
    send_started: Option<Instant>,
    error: String,
    show_success_dialog: bool,
}

struct PendingWrite {
    session_id: u32,
    path: String,
    purpose: WritePurpose,
}

#[derive(Clone)]
enum WritePurpose {
    SaveText,
    CreateFile {
        current_path: String,
        display_name: String,
    },
}

struct PendingMkdir {
    session_id: u32,
    path: String,
    display_name: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NewEntryKind {
    Folder,
    File,
}

struct NewEntryModalState {
    session_id: u32,
    current_path: String,
    kind: NewEntryKind,
    name: String,
    error: String,
    open: bool,
}

struct FinderBlankMenuState {
    session_id: u32,
    current_path: String,
    pos: egui::Pos2,
    show_new_submenu: bool,
}

struct PendingStreamDownload {
    session_id: u32,
    temp_path: PathBuf,
    total_size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MediaStreamKind {
    Video,
    Audio,
}

impl MediaStreamKind {
    fn min_start_bytes(self) -> u64 {
        match self {
            MediaStreamKind::Video => crate::stream_protocol::MIN_VIDEO_START_BYTES,
            MediaStreamKind::Audio => crate::stream_protocol::MIN_AUDIO_START_BYTES,
        }
    }

    fn tr_error(&self, locale: &Locale) -> String {
        locale.tr(match self {
            MediaStreamKind::Video => "VIDEO_PLAY_ERROR",
            MediaStreamKind::Audio => "AUDIO_PLAY_ERROR",
        })
    }

    fn tr_too_large(&self, locale: &Locale) -> String {
        locale.tr(match self {
            MediaStreamKind::Video => "VIDEO_PLAY_TOO_LARGE",
            MediaStreamKind::Audio => "AUDIO_PLAY_TOO_LARGE",
        })
    }

    fn tr_no_callback(&self, locale: &Locale) -> String {
        locale.tr(match self {
            MediaStreamKind::Video => "VIDEO_PLAY_NO_CALLBACK",
            MediaStreamKind::Audio => "AUDIO_PLAY_NO_CALLBACK",
        })
    }

    fn req_id_prefix(self) -> &'static str {
        match self {
            MediaStreamKind::Video => "vs",
            MediaStreamKind::Audio => "as",
        }
    }
}

/// 后台音视频拉流（无 UI，分块写入临时文件后调起系统播放器）。
struct MediaStreamState {
    kind: MediaStreamKind,
    session_id: u32,
    api_path: String,
    temp_path: PathBuf,
    total_size: u64,
    written: u64,
    player_started: bool,
    done: bool,
    failed: bool,
}

enum TextPreviewState {
    Loading {
        name: String,
    },
    Ready {
        name: String,
        path: String,
        session_id: u32,
        content: String,
        saved_snapshot: String,
        saving: bool,
        status_line: String,
        status_error: bool,
    },
    Error {
        name: String,
        message: String,
    },
}

enum ImagePreviewState {
    Loading {
        name: String,
    },
    Ready {
        name: String,
        color_image: Option<egui::ColorImage>,
        texture: Option<egui::TextureHandle>,
    },
    Error {
        name: String,
        message: String,
    },
}

/// 文件预览最大体积（内存），避免超大文件占满内存
const MAX_FILE_PREVIEW_BYTES: usize = 50 * 1024 * 1024;

#[derive(Default)]
struct SessionEntry {
    id: u32,
    target_token: String,
    current_path: String,
    resources: Vec<ResourceEntry>,
    last_error: String,
}

#[derive(Clone)]
struct ResourceEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: i64,
}

enum OpenSessionResult {
    Ok(SessionEntry),
    Err(String),
}

#[derive(Deserialize)]
struct RpcResponse {
    id: String,
    ok: bool,
    error: Option<String>,
    data: Option<Value>,
}

#[derive(Deserialize)]
struct RpcListData {
    path: String,
    items: Vec<RpcFileEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcFileEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: i64,
}

pub struct MyWebDiskApp {
    locale: Locale,
    screen: Screen,
    token: String,
    passwd: String,
    status_key: String,
    status_args: String,
    status_kind: StatusKind,
    logging_in: bool,
    login_tx: Option<Sender<LoginResult>>,
    login_rx: Option<Receiver<LoginResult>>,
    connecting_session: bool,
    open_session_tx: Option<Sender<OpenSessionResult>>,
    open_session_rx: Option<Receiver<OpenSessionResult>>,
    ipc: Arc<Mutex<Option<WebrpcIpc>>>,
    sessions: Vec<SessionEntry>,
    selected_session: Option<usize>,
    show_connect_modal: bool,
    connect_target_token: String,
    connect_passwd: String,
    connect_status: String,
    connect_status_is_error: bool,
    callback_tx: Sender<CallbackEvent>,
    callback_rx: Receiver<CallbackEvent>,
    callback_started: bool,
    callback_connected: bool,
    pending_list_requests: HashMap<String, u32>,
    pending_download_requests: HashMap<String, PendingDownload>,
    awaiting_file_downloads: HashMap<u32, IncomingFileDownload>,
    pending_delete_requests: HashMap<String, PendingDelete>,
    pending_write_requests: HashMap<String, PendingWrite>,
    pending_mkdir_requests: HashMap<String, PendingMkdir>,
    pending_stream_downloads: HashMap<String, PendingStreamDownload>,
    image_preview: Option<ImagePreviewState>,
    text_preview: Option<TextPreviewState>,
    media_streams: HashMap<String, MediaStreamState>,
    /// 当前应调起系统播放器的拉流请求 id
    media_play_req_id: Option<String>,
    cancelled_stream_req_ids: HashSet<String>,
    office_edits: HashMap<String, OfficeEditState>,
    upload_waits: HashMap<String, UploadWait>,
    finder_upload: Option<FinderUploadState>,
    upload_send_rx: Option<Receiver<Result<(), String>>>,
    new_entry_modal: Option<NewEntryModalState>,
    finder_blank_menu: Option<FinderBlankMenuState>,
}

impl MyWebDiskApp {
    fn format_file_size(size: i64) -> String {
        const KB: i64 = 1024;
        const MB: i64 = KB * 1024;
        const GB: i64 = MB * 1024;
        if size >= GB {
            format!("{:.1} GB", size as f64 / GB as f64)
        } else if size >= MB {
            format!("{:.1} MB", size as f64 / MB as f64)
        } else if size >= KB {
            format!("{:.1} KB", size as f64 / KB as f64)
        } else {
            format!("{size} B")
        }
    }

    fn draw_finder_row(
        ui: &mut egui::Ui,
        entry: &ResourceEntry,
        row_w: f32,
        menu_id: egui::Id,
        actions_label: &str,
        download_label: &str,
        delete_label: &str,
    ) -> (egui::Response, Option<FinderRowAction>) {
        const ROW_H: f32 = 24.0;
        const ROW_RADIUS: f32 = 4.0;
        const LEFT_PAD: f32 = 8.0;
        const RIGHT_PAD: f32 = 12.0;
        const ICON_SIZE: f32 = 16.0;
        const ICON_NAME_GAP: f32 = 8.0;
        const SIZE_COL_W: f32 = 68.0;
        const ACTIONS_COL_W: f32 = 48.0;
        const COL_GAP: f32 = 10.0;

        let row_id = menu_id.with("row");
        let (row_rect, _row_slot) =
            ui.allocate_exact_size(egui::vec2(row_w, ROW_H), egui::Sense::hover());

        let row_right = row_rect.max.x - RIGHT_PAD;
        // 各列按行右缘（扣除内边距）固定坐标，保证每行图标/大小/操作竖向对齐
        let actions_x = row_right - ACTIONS_COL_W;
        let size_x = actions_x - COL_GAP - SIZE_COL_W;
        let icon_x = row_rect.min.x + LEFT_PAD;
        let name_x = icon_x + ICON_SIZE + ICON_NAME_GAP;

        let action_rect = egui::Rect::from_min_size(
            egui::pos2(actions_x, row_rect.min.y),
            egui::vec2(ACTIONS_COL_W, ROW_H),
        );
        let size_rect = egui::Rect::from_min_size(
            egui::pos2(size_x, row_rect.min.y),
            egui::vec2(SIZE_COL_W, ROW_H),
        );
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(icon_x, row_rect.center().y - ICON_SIZE * 0.5),
            egui::vec2(ICON_SIZE, ICON_SIZE),
        );
        let name_rect = egui::Rect::from_min_max(
            egui::pos2(name_x, row_rect.min.y),
            egui::pos2(size_x - COL_GAP, row_rect.max.y),
        );
        let content_rect = egui::Rect::from_min_max(
            row_rect.min,
            egui::pos2(action_rect.min.x, row_rect.max.y),
        );

        let content_response = ui.interact(
            content_rect,
            row_id.with("content"),
            egui::Sense::click().union(egui::Sense::hover()),
        );
        let row_hovered =
            content_response.hovered() || ui.rect_contains_pointer(action_rect);

        let mut row_action = None;

        // 所有行内控件放在同一子 Ui 中，避免在滚动列表父级上多次 allocate 导致逐行右移
        let mut row_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(row_id)
                .max_rect(row_rect),
        );

        if row_hovered {
            row_ui.painter().rect_filled(
                row_ui.min_rect(),
                ROW_RADIUS,
                crate::theme::MAC_ROW_HOVER_GREEN,
            );
        }

        crate::file_icons::draw(row_ui.painter(), icon_rect, &entry.name, entry.is_dir);

        row_ui.allocate_new_ui(
            egui::UiBuilder::new()
                .id_salt(row_id.with("name"))
                .max_rect(name_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&entry.name)
                            .size(13.0)
                            .color(crate::theme::MAC_TEXT),
                    )
                    .truncate(),
                );
            },
        );

        row_ui.allocate_new_ui(
            egui::UiBuilder::new()
                .id_salt(row_id.with("size"))
                .max_rect(size_rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                if !entry.is_dir {
                    ui.label(
                        egui::RichText::new(Self::format_file_size(entry.size))
                            .size(11.0)
                            .color(crate::theme::MAC_TEXT_DIM),
                    );
                }
            },
        );

        row_ui.allocate_new_ui(
            egui::UiBuilder::new()
                .id_salt(menu_id)
                .max_rect(action_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.set_width(ACTIONS_COL_W);
                ui.set_max_width(ACTIONS_COL_W);
                let btn = ui.add(crate::theme::finder_action_button(
                    actions_label,
                    row_hovered,
                ));
                if btn.clicked() {
                    ui.memory_mut(|mem| mem.toggle_popup(menu_id));
                }
                if let Some(_inner) = egui::popup::popup_below_widget(
                    ui,
                    menu_id,
                    &btn,
                    egui::popup::PopupCloseBehavior::CloseOnClickOutside,
                    |ui| {
                        ui.set_min_width(88.0);
                        if !entry.is_dir {
                            if ui.button(download_label).clicked() {
                                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                    row_action = Some(FinderRowAction::Download {
                                        dest_path: dir.join(&entry.name),
                                    });
                                    ui.memory_mut(|mem| mem.close_popup());
                                }
                            }
                        }
                        if ui
                            .button(
                                egui::RichText::new(delete_label)
                                    .color(crate::theme::STATUS_ERROR),
                            )
                            .clicked()
                        {
                            row_action = Some(FinderRowAction::Delete);
                            ui.memory_mut(|mem| mem.close_popup());
                        }
                    },
                ) {}
            },
        );

        if entry.is_dir
            || crate::file_icons::is_image(&entry.name, entry.is_dir)
            || crate::file_icons::is_text(&entry.name, entry.is_dir)
            || crate::file_icons::is_video(&entry.name, entry.is_dir)
            || crate::file_icons::is_audio(&entry.name, entry.is_dir)
            || crate::file_icons::is_office(&entry.name, entry.is_dir)
        {
            content_response
                .clone()
                .on_hover_cursor(egui::CursorIcon::PointingHand);
        }

        (content_response, row_action)
    }

    fn draw_resource_finder(&mut self, ui: &mut egui::Ui) {
        let refresh_label = self.locale.tr("BTN_REFRESH_RESOURCE");
        let upload_label = self.locale.tr("BTN_UPLOAD");
        let back_label = self.locale.tr("BTN_BACK_PARENT");
        let back_disabled_label = self.locale.tr("BTN_BACK_PARENT_DISABLED");
        let close_label = self.locale.tr("BTN_CLOSE_SESSION");
        let empty_label = self.locale.tr("RESOURCE_EMPTY");
        let not_selected_label = self.locale.tr("SESSION_NOT_SELECTED");
        let actions_label = self.locale.tr("BTN_FILE_ACTIONS");
        let download_label = self.locale.tr("MENU_DOWNLOAD");
        let delete_label = self.locale.tr("MENU_DELETE");

        let panel = self.selected_session.and_then(|sel| {
            self.sessions.get(sel).map(|s| {
                (
                    s.id,
                    s.current_path.clone(),
                    s.resources.clone(),
                    s.last_error.clone(),
                )
            })
        });

        let mut do_refresh: Option<u32> = None;
        let mut open_folder: Option<(u32, String)> = None;
        let mut go_parent: Option<(u32, String)> = None;
        let mut close_session: Option<u32> = None;
        let mut open_image: Option<(u32, String, String)> = None;
        let mut open_text: Option<(u32, String, String)> = None;
        let mut open_video: Option<(u32, String, String, i64)> = None;
        let mut open_audio: Option<(u32, String, String, i64)> = None;
        let mut open_office: Option<(u32, String, String, i64)> = None;
        let mut save_local: Option<(u32, String, String, PathBuf, i64)> = None;
        let mut delete_item: Option<(u32, String, String)> = None;
        let mut start_upload: Option<(u32, String)> = None;

        let full = ui.max_rect();
        let toolbar_h = 28.0;
        let toolbar_rect =
            egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, full.min.y + toolbar_h));
        let body_rect = egui::Rect::from_min_max(
            egui::pos2(full.min.x, toolbar_rect.max.y),
            full.max,
        );

        egui::Frame::new()
            .fill(crate::theme::MAC_PANEL)
            .stroke(egui::Stroke::new(1.0, crate::theme::MAC_BORDER))
            .corner_radius(egui::CornerRadius::same(8))
            .show(ui, |ui| {
                ui.set_clip_rect(full);

                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(toolbar_rect), |ui| {
                    egui::Frame::new()
                        .fill(crate::theme::MAC_TOOLBAR)
                        .inner_margin(egui::Margin::symmetric(6, 3))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                if let Some((session_id, current_path, _, _)) = &panel {
                                    let at_root = current_path.trim_end_matches('/').is_empty();
                                    let back_tip = if at_root {
                                        &back_disabled_label
                                    } else {
                                        &back_label
                                    };
                                    if crate::theme::mac_toolbar_button(ui, "←", back_tip, !at_root)
                                        .clicked()
                                    {
                                        if let Some(parent) = Self::parent_api_path(current_path) {
                                            go_parent = Some((*session_id, parent));
                                        }
                                    }
                                    if crate::theme::mac_toolbar_button(
                                        ui,
                                        "↻",
                                        &refresh_label,
                                        true,
                                    )
                                    .clicked()
                                    {
                                        do_refresh = Some(*session_id);
                                    }
                                    let upload_busy = self
                                        .finder_upload
                                        .as_ref()
                                        .is_some_and(|u| u.session_id == *session_id);
                                    if ui
                                        .add_enabled(
                                            !upload_busy,
                                            crate::theme::finder_action_button(&upload_label, false),
                                        )
                                        .clicked()
                                    {
                                        start_upload =
                                            Some((*session_id, current_path.clone()));
                                    }
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if let Some((session_id, _, _, _)) = &panel {
                                            if crate::theme::mac_toolbar_close_button(
                                                ui,
                                                &close_label,
                                                true,
                                            )
                                            .clicked()
                                            {
                                                close_session = Some(*session_id);
                                            }
                                        }
                                    },
                                );
                            });
                        });
                    let p = ui.painter();
                    p.line_segment(
                        [
                            egui::pos2(toolbar_rect.min.x + 4.0, toolbar_rect.max.y),
                            egui::pos2(toolbar_rect.max.x - 4.0, toolbar_rect.max.y),
                        ],
                        egui::Stroke::new(1.0, crate::theme::MAC_BORDER),
                    );
                });

                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body_rect), |ui| {
                    ui.set_min_size(body_rect.size());
                    match &panel {
                        Some((session_id, current_path, resources, last_error)) => {
                            if !last_error.is_empty() {
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new(last_error)
                                        .size(11.0)
                                        .color(crate::theme::STATUS_ERROR),
                                );
                            }
                            let mut new_kind: Option<NewEntryKind> = None;
                            egui::Frame::NONE
                                .inner_margin(egui::Margin::symmetric(8, 2))
                                .show(ui, |ui| {
                                    let menu_bg_rect = ui.available_rect_before_wrap();
                                    let menu_bg_resp = ui.interact(
                                        menu_bg_rect,
                                        egui::Id::new(("finder_blank_ctx", *session_id)),
                                        egui::Sense::click(),
                                    );

                                    egui::ScrollArea::vertical()
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            if resources.is_empty() {
                                                ui.add_space(24.0);
                                                ui.vertical_centered(|ui| {
                                                    ui.label(
                                                        egui::RichText::new(&empty_label)
                                                            .size(12.0)
                                                            .color(crate::theme::MAC_TEXT_DIM),
                                                    );
                                                });
                                                return;
                                            }

                                            let row_w = ui.available_width().max(80.0);
                                            ui.spacing_mut().item_spacing.y = 1.0;
                                            for item in resources {
                                                let api_path =
                                                    Self::entry_api_path(current_path, item);
                                                let menu_id = egui::Id::new((
                                                    "file_row_menu",
                                                    *session_id,
                                                    api_path.clone(),
                                                ));
                                                let (response, action) = Self::draw_finder_row(
                                                    ui,
                                                    item,
                                                    row_w,
                                                    menu_id,
                                                    &actions_label,
                                                    &download_label,
                                                    &delete_label,
                                                );
                                                if let Some(action) = action {
                                                    match action {
                                                        FinderRowAction::Download { dest_path } => {
                                                            save_local = Some((
                                                                *session_id,
                                                                api_path.clone(),
                                                                item.name.clone(),
                                                                dest_path,
                                                                item.size,
                                                            ));
                                                        }
                                                        FinderRowAction::Delete => {
                                                            delete_item = Some((
                                                                *session_id,
                                                                api_path.clone(),
                                                                item.name.clone(),
                                                            ));
                                                        }
                                                    }
                                                }
                                                let row_click = response.clicked()
                                                    && !ui.ctx().memory(|m| m.any_popup_open());
                                                if item.is_dir && row_click {
                                                    open_folder =
                                                        Some((*session_id, api_path.clone()));
                                                } else if crate::file_icons::is_image(
                                                    &item.name,
                                                    item.is_dir,
                                                ) && row_click
                                                {
                                                    open_image = Some((
                                                        *session_id,
                                                        api_path,
                                                        item.name.clone(),
                                                    ));
                                                } else if crate::file_icons::is_text(
                                                    &item.name,
                                                    item.is_dir,
                                                ) && row_click
                                                {
                                                    open_text = Some((
                                                        *session_id,
                                                        api_path,
                                                        item.name.clone(),
                                                    ));
                                                } else if crate::file_icons::is_video(
                                                    &item.name,
                                                    item.is_dir,
                                                ) && row_click
                                                {
                                                    open_video = Some((
                                                        *session_id,
                                                        api_path,
                                                        item.name.clone(),
                                                        item.size,
                                                    ));
                                                } else if crate::file_icons::is_audio(
                                                    &item.name,
                                                    item.is_dir,
                                                ) && row_click
                                                {
                                                    open_audio = Some((
                                                        *session_id,
                                                        api_path,
                                                        item.name.clone(),
                                                        item.size,
                                                    ));
                                                } else if crate::file_icons::is_office(
                                                    &item.name,
                                                    item.is_dir,
                                                ) && row_click
                                                {
                                                    open_office = Some((
                                                        *session_id,
                                                        api_path,
                                                        item.name.clone(),
                                                        item.size,
                                                    ));
                                                }
                                            }
                                        });

                                    menu_bg_resp.context_menu(|ui| {
                                        crate::theme::apply_neon_menu_visuals(ui);
                                        ui.menu_button(
                                            egui::RichText::new(self.locale.tr("CTX_NEW"))
                                                .color(crate::theme::NEON_MENU_TEXT_BLUE)
                                                .size(12.0),
                                            |ui| {
                                                crate::theme::apply_neon_menu_visuals(ui);
                                                if ui
                                                    .button(
                                                        egui::RichText::new(
                                                            self.locale.tr("CTX_NEW_FOLDER"),
                                                        )
                                                        .color(crate::theme::NEON_MENU_TEXT_BLUE)
                                                        .size(11.5),
                                                    )
                                                    .clicked()
                                                {
                                                    new_kind = Some(NewEntryKind::Folder);
                                                    ui.close_menu();
                                                }
                                                if ui
                                                    .button(
                                                        egui::RichText::new(
                                                            self.locale.tr("CTX_NEW_FILE"),
                                                        )
                                                        .color(crate::theme::NEON_MENU_TEXT_BLUE)
                                                        .size(11.5),
                                                    )
                                                    .clicked()
                                                {
                                                    new_kind = Some(NewEntryKind::File);
                                                    ui.close_menu();
                                                }
                                            },
                                        );
                                    });
                                });

                            if let Some(kind) = new_kind {
                                self.new_entry_modal = Some(NewEntryModalState {
                                    session_id: *session_id,
                                    current_path: current_path.clone(),
                                    kind,
                                    name: String::new(),
                                    error: String::new(),
                                    open: true,
                                });
                            }
                        }
                        None => {
                            ui.add_space(24.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new(&not_selected_label)
                                        .size(12.0)
                                        .color(crate::theme::MAC_TEXT_DIM),
                                );
                            });
                        }
                    }
                });
            });

        if let Some((session_id, path, name)) = open_image {
            self.start_image_preview_download(session_id, &path, &name);
        }
        if let Some((session_id, path, name)) = open_text {
            self.start_text_preview_download(session_id, &path, &name);
        }
        if let Some((session_id, path, name, size)) = open_video {
            self.start_media_stream(MediaStreamKind::Video, session_id, &path, &name, size);
        }
        if let Some((session_id, path, name, size)) = open_audio {
            self.start_media_stream(MediaStreamKind::Audio, session_id, &path, &name, size);
        }
        if let Some((session_id, path, name, size)) = open_office {
            self.start_office_edit(session_id, &path, &name, size);
        }
        if let Some((session_id, path, name, dest_path, size)) = save_local {
            self.start_local_download(session_id, &path, &name, dest_path, size);
        }
        if let Some((session_id, path, name)) = delete_item {
            self.request_delete(session_id, &path, &name);
        }
        if let Some((session_id, current_path)) = start_upload {
            self.start_finder_upload(session_id, &current_path);
        }
        if let Some(session_id) = close_session {
            self.close_disk_session(session_id);
        }
        if let Some((session_id, path)) = go_parent {
            eprintln!(
                "[mywebdisk-desktop][app] go parent session_id={session_id} path='{path}'"
            );
            self.request_list(session_id, &path);
        }
        if let Some((session_id, path)) = open_folder {
            eprintln!(
                "[mywebdisk-desktop][app] open folder session_id={session_id} path='{path}'"
            );
            self.request_list(session_id, &path);
        }
        if let Some(session_id) = do_refresh {
            let path = self
                .session_by_id(session_id)
                .map(|s| s.current_path.clone())
                .unwrap_or_default();
            self.request_list(session_id, &path);
        }
    }

    fn parent_api_path(current: &str) -> Option<String> {
        let current = current.trim_end_matches('/');
        if current.is_empty() {
            return None;
        }
        Some(match current.rfind('/') {
            None => String::new(),
            Some(0) => String::new(),
            Some(idx) => current[..idx].to_string(),
        })
    }

    fn join_api_path(base: &str, name: &str) -> String {
        let base = base.trim_end_matches('/');
        if base.is_empty() {
            name.to_string()
        } else {
            format!("{base}/{name}")
        }
    }

    /// 列表项在当前目录下的 API 路径（与界面所见一致，避免 `item.path` 陈旧导致下错文件）。
    fn entry_api_path(current_path: &str, item: &ResourceEntry) -> String {
        let joined = Self::join_api_path(current_path, &item.name);
        if item.path.is_empty() {
            return joined;
        }
        let path = item.path.replace('\\', "/");
        let joined_norm = joined.replace('\\', "/");
        if path == joined_norm || path.ends_with(&format!("/{}", item.name)) {
            path
        } else {
            joined
        }
    }

    pub fn new() -> Self {
        let (callback_tx, callback_rx) = mpsc::channel();
        Self {
            locale: Locale::load_embedded(),
            screen: Screen::Login,
            token: String::new(),
            passwd: String::new(),
            status_key: "STATUS_HINT".into(),
            status_args: String::new(),
            status_kind: StatusKind::Hint,
            logging_in: false,
            login_tx: None,
            login_rx: None,
            connecting_session: false,
            open_session_tx: None,
            open_session_rx: None,
            ipc: Arc::new(Mutex::new(None)),
            sessions: Vec::new(),
            selected_session: None,
            show_connect_modal: false,
            connect_target_token: String::new(),
            connect_passwd: String::new(),
            connect_status: String::new(),
            connect_status_is_error: false,
            callback_tx,
            callback_rx,
            callback_started: false,
            callback_connected: false,
            pending_list_requests: HashMap::new(),
            pending_download_requests: HashMap::new(),
            awaiting_file_downloads: HashMap::new(),
            pending_delete_requests: HashMap::new(),
            pending_write_requests: HashMap::new(),
            pending_mkdir_requests: HashMap::new(),
            pending_stream_downloads: HashMap::new(),
            image_preview: None,
            text_preview: None,
            media_streams: HashMap::new(),
            media_play_req_id: None,
            cancelled_stream_req_ids: HashSet::new(),
            office_edits: HashMap::new(),
            upload_waits: HashMap::new(),
            finder_upload: None,
            upload_send_rx: None,
            new_entry_modal: None,
            finder_blank_menu: None,
        }
    }

    fn office_edit_key(session_id: u32, api_path: &str) -> String {
        format!("{session_id}:{api_path}")
    }

    fn office_file_modified(state: &OfficeEditState) -> bool {
        let Ok(meta) = std::fs::metadata(&state.temp_path) else {
            return false;
        };
        let len = meta.len();
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        len != state.baseline_len || modified != state.baseline_modified
    }

    fn file_preview_open(&self) -> bool {
        self.image_preview.is_some() || self.text_preview.is_some()
    }

    fn report_media_stream_error(&mut self, session_id: u32, message: String) {
        if let Some(session) = self.session_mut_by_id(session_id) {
            session.last_error = message;
        }
        let stale: Vec<String> = self
            .media_streams
            .iter()
            .filter(|(_, m)| m.session_id == session_id)
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            self.media_streams.remove(&id);
            self.cancelled_stream_req_ids.insert(id);
        }
        if self
            .media_play_req_id
            .as_ref()
            .is_some_and(|id| self.media_streams.get(id).is_none())
        {
            self.media_play_req_id = None;
        }
    }

    fn cancel_session_media_streams(&mut self, session_id: u32) {
        let ids: Vec<String> = self
            .media_streams
            .iter()
            .filter(|(_, m)| m.session_id == session_id)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &ids {
            self.cancelled_stream_req_ids.insert(id.clone());
            self.media_streams.remove(id);
        }
        self.pending_stream_downloads
            .retain(|_, p| p.session_id != session_id);
        if self.media_play_req_id.as_ref().is_some_and(|id| ids.contains(id)) {
            self.media_play_req_id = None;
        }
    }

    fn append_file_bucket(incoming: &mut IncomingFileDownload, data: &[u8]) -> Result<(), String> {
        if let Some(path) = &incoming.write_path {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(path)
                .map_err(|e| e.to_string())?;
            file.write_all(data).map_err(|e| e.to_string())?;
        } else {
            incoming.memory.extend_from_slice(data);
        }
        incoming.bytes_received += data.len() as u64;
        Ok(())
    }

    fn cleanup_ipc(&self) {
        if let Ok(mut guard) = self.ipc.lock() {
            if let Some(mut ipc) = guard.take() {
                ipc.close();
            }
        }
    }

    fn can_login(&self) -> bool {
        !self.token.trim().is_empty() && !self.passwd.is_empty() && !self.logging_in
    }

    fn status_text(&self) -> String {
        let mut text = self.locale.tr(&self.status_key);
        if !self.status_args.is_empty() {
            text = text.replace("%s", &self.status_args);
        }
        text
    }

    fn set_status_key(&mut self, key: &str, args: &str, kind: StatusKind) {
        self.status_key = key.to_string();
        self.status_args = args.to_string();
        self.status_kind = kind;
    }

    fn poll_login(&mut self) {
        let Some(rx) = self.login_rx.as_ref() else {
            return;
        };
        if let Ok(result) = rx.try_recv() {
            self.logging_in = false;
            self.login_tx = None;
            self.login_rx = None;
            match result {
                LoginResult::Ok => {
                    self.set_status_key("STATUS_SUCCESS", "", StatusKind::Success);
                    self.screen = Screen::Sessions;
                    self.start_callback_reader_if_needed();
                }
                LoginResult::Err(msg) => {
                    self.set_status_key("STATUS_FAILED", &msg, StatusKind::Error);
                }
            }
        }
    }

    fn start_callback_reader_if_needed(&mut self) {
        if self.callback_started {
            return;
        }
        let port_result: Result<i32, String> = match self.ipc.lock() {
            Ok(guard) => {
                if let Some(ipc) = guard.as_ref() {
                    ipc.receive_port()
                } else {
                    Err("webrpc client is not ready".into())
                }
            }
            Err(_) => Err("webrpc lock failed".into()),
        };
        let port = match port_result {
            Ok(port) => port,
            Err(e) => {
                self.set_status_key("STATUS_FAILED", &e, StatusKind::Error);
                return;
            }
        };
        eprintln!("[mywebdisk-desktop][app] start callback reader on port={port}");
        self.callback_started = true;
        Self::spawn_callback_reader(port, self.callback_tx.clone());
    }

    fn spawn_callback_reader(port: i32, tx: Sender<CallbackEvent>) {
        thread::spawn(move || {
            let addr = format!("127.0.0.1:{port}");
            loop {
                eprintln!("[mywebdisk-desktop][app] callback connecting to {addr}");
                let mut conn = match TcpStream::connect(&addr) {
                    Ok(stream) => stream,
                    Err(e) => {
                        let _ = tx.send(CallbackEvent::Error(format!("callback connect failed: {e}")));
                        thread::sleep(std::time::Duration::from_millis(500));
                        continue;
                    }
                };
                eprintln!("[mywebdisk-desktop][app] callback connected");
                let _ = tx.send(CallbackEvent::Connected);

                loop {
                    let mut hdr = [0u8; 5];
                    if let Err(e) = conn.read_exact(&mut hdr) {
                        if e.kind() != ErrorKind::UnexpectedEof {
                            let _ = tx.send(CallbackEvent::Error(format!("callback read header failed: {e}")));
                        } else {
                            let _ = tx.send(CallbackEvent::Error("callback connection closed".into()));
                        }
                        break;
                    }
                    let session_id = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
                    let data_type = hdr[4];
                    eprintln!(
                        "[mywebdisk-desktop][app] callback frame session_id={} type={}",
                        session_id, data_type
                    );

                    if data_type == 2 {
                        let mut len_buf = [0u8; 4];
                        if conn.read_exact(&mut len_buf).is_err() {
                            let _ = tx.send(CallbackEvent::Error("callback read data length failed".into()));
                            break;
                        }
                        let n = u32::from_be_bytes(len_buf) as usize;
                        let mut payload = vec![0u8; n];
                        if conn.read_exact(&mut payload).is_err() {
                            let _ = tx.send(CallbackEvent::Error("callback read data body failed".into()));
                            break;
                        }
                        eprintln!(
                            "[mywebdisk-desktop][app] callback data session_id={} len={}",
                            session_id, n
                        );
                        let _ = tx.send(CallbackEvent::Data { session_id, payload });
                    } else if data_type == 1 {
                        let mut name_len = [0u8; 4];
                        if conn.read_exact(&mut name_len).is_err() {
                            break;
                        }
                        let n1 = u32::from_be_bytes(name_len) as usize;
                        let mut name = vec![0u8; n1];
                        if conn.read_exact(&mut name).is_err() {
                            break;
                        }
                        let mut data_len = [0u8; 4];
                        if conn.read_exact(&mut data_len).is_err() {
                            break;
                        }
                        let n2 = u32::from_be_bytes(data_len) as usize;
                        let mut data = vec![0u8; n2];
                        if conn.read_exact(&mut data).is_err() {
                            break;
                        }
                        eprintln!(
                            "[mywebdisk-desktop][app] callback file session_id={} name_len={} data_len={}",
                            session_id, n1, n2
                        );
                        let _ = tx.send(CallbackEvent::File {
                            session_id,
                            file_name: name,
                            data,
                        });
                    }
                }

                thread::sleep(std::time::Duration::from_millis(300));
            }
        });
    }

    fn can_connect_session(&self) -> bool {
        !self.connect_target_token.trim().is_empty() && !self.connect_passwd.is_empty() && !self.connecting_session
    }

    fn start_login(&mut self) {
        if !self.can_login() {
            self.set_status_key("ERR_EMPTY", "", StatusKind::Error);
            return;
        }
        self.cleanup_ipc();
        self.callback_started = false;
        self.callback_connected = false;
        self.pending_list_requests.clear();
        self.pending_download_requests.clear();
        self.awaiting_file_downloads.clear();
        self.pending_delete_requests.clear();
        self.pending_write_requests.clear();
        self.pending_mkdir_requests.clear();
        self.pending_stream_downloads.clear();
        self.upload_waits.clear();
        self.finder_upload = None;
        self.upload_send_rx = None;
        self.new_entry_modal = None;
        self.finder_blank_menu = None;
        self.image_preview = None;
        self.text_preview = None;
        let (tx, rx) = mpsc::channel();
        self.login_tx = Some(tx.clone());
        self.login_rx = Some(rx);
        self.logging_in = true;
        self.set_status_key("STATUS_CONNECTING", "", StatusKind::Info);

        let token = self.token.trim().to_string();
        let passwd = self.passwd.clone();
        let ipc_holder = Arc::clone(&self.ipc);
        thread::spawn(move || {
            let result = match WebrpcIpc::start() {
                Ok(mut ipc) => match ipc.login(&token, &passwd, &generate_permission()) {
                    Ok(()) => {
                        if let Ok(mut guard) = ipc_holder.lock() {
                            *guard = Some(ipc);
                        }
                        LoginResult::Ok
                    }
                    Err(e) => LoginResult::Err(e),
                },
                Err(e) => LoginResult::Err(e),
            };
            let _ = tx.send(result);
        });
    }

    fn poll_open_session(&mut self) {
        let Some(rx) = self.open_session_rx.as_ref() else {
            return;
        };
        if let Ok(result) = rx.try_recv() {
            self.connecting_session = false;
            self.open_session_tx = None;
            self.open_session_rx = None;
            match result {
                OpenSessionResult::Ok(session) => {
                    let session_id = session.id;
                    if let Err(e) =
                        crate::session_workspace::ensure_session_temp(&session.target_token)
                    {
                        eprintln!(
                            "[mywebdisk-desktop][app] ensure_session_temp token='{}' err={e}",
                            session.target_token
                        );
                    }
                    self.sessions.push(session);
                    self.selected_session = Some(self.sessions.len().saturating_sub(1));
                    self.connect_status.clear();
                    self.connect_status_is_error = false;
                    self.show_connect_modal = false;
                    self.connect_target_token.clear();
                    self.connect_passwd.clear();
                    self.request_list(session_id, "");
                }
                OpenSessionResult::Err(msg) => {
                    self.connect_status = msg;
                    self.connect_status_is_error = true;
                    self.show_connect_modal = true;
                }
            }
        }
    }

    fn start_open_session(&mut self) {
        if !self.can_connect_session() {
            self.connect_status = "请填写目标 token 和口令".to_string();
            self.connect_status_is_error = true;
            return;
        }
        self.connect_status = "正在建立会话连接...".to_string();
        self.connect_status_is_error = false;
        self.connecting_session = true;
        let (tx, rx) = mpsc::channel();
        self.open_session_tx = Some(tx.clone());
        self.open_session_rx = Some(rx);

        let target_token = self.connect_target_token.trim().to_string();
        let permission = self.connect_passwd.clone();
        eprintln!(
            "[mywebdisk-desktop][app] open_session request target_token='{}' permission_len={}",
            target_token,
            permission.len()
        );
        let ipc_holder = Arc::clone(&self.ipc);
        thread::spawn(move || {
            let t0 = std::time::Instant::now();
            let result = match ipc_holder.lock() {
                Ok(mut guard) => {
                    if let Some(ipc) = guard.as_mut() {
                        match ipc.open_session(&target_token, &permission) {
                            Ok(session_id) => OpenSessionResult::Ok(SessionEntry {
                                id: session_id,
                                target_token,
                                current_path: String::new(),
                                resources: Vec::new(),
                                last_error: String::new(),
                            }),
                            Err(e) => OpenSessionResult::Err(e),
                        }
                    } else {
                        OpenSessionResult::Err("webrpc client is not ready".into())
                    }
                }
                Err(_) => OpenSessionResult::Err("webrpc client lock failed".into()),
            };
            eprintln!(
                "[mywebdisk-desktop][app] open_session thread total_ms={}",
                t0.elapsed().as_millis()
            );
            let _ = tx.send(result);
        });
    }

    fn request_list(&mut self, session_id: u32, path: &str) {
        if !self.callback_connected {
            let msg = "callback channel is not connected yet".to_string();
            eprintln!(
                "[mywebdisk-desktop][app] request_list blocked session_id={} err={}",
                session_id, msg
            );
            if let Some(session) = self.session_mut_by_id(session_id) {
                session.last_error = msg;
            }
            return;
        }
        let req_id = format!("list-{:016x}", rand::random::<u64>());
        let body = json!({
            "id": req_id,
            "op": "list",
            "path": path,
        })
        .to_string();
        eprintln!(
            "[mywebdisk-desktop][app] request_list session_id={} req_id={} path='{}'",
            session_id, req_id, path
        );
        self.pending_list_requests.insert(req_id.clone(), session_id);
        eprintln!(
            "[mywebdisk-desktop][app] request_list pending insert req_id={} pending_count={}",
            req_id,
            self.pending_list_requests.len()
        );

        // SendData 可能同步阻塞到 SEND_DATA_TIMEOUT_MS；放后台线程，避免拖慢 OpenSession 后的连接体验。
        let ipc_holder = Arc::clone(&self.ipc);
        thread::spawn(move || {
            let send_result = match ipc_holder.lock() {
                Ok(guard) => {
                    if let Some(ipc) = guard.as_ref() {
                        ipc.send_data(session_id, body.as_bytes())
                    } else {
                        Err("webrpc client is not ready".into())
                    }
                }
                Err(_) => Err("webrpc client lock failed".into()),
            };
            match send_result {
                Ok(()) => {
                    eprintln!("[mywebdisk-desktop][app] request_list send ok req_id={req_id}");
                }
                Err(e) => {
                    eprintln!(
                        "[mywebdisk-desktop][app] request_list send warning session_id={session_id} req_id={req_id} err={e}"
                    );
                }
            }
        });
    }

    fn start_image_preview_download(&mut self, session_id: u32, path: &str, name: &str) {
        if !self.callback_connected {
            let msg = self.locale.tr("IMAGE_PREVIEW_NO_CALLBACK");
            self.image_preview = Some(ImagePreviewState::Error {
                name: name.to_string(),
                message: msg,
            });
            return;
        }
        if let Some(entry) = self
            .session_by_id(session_id)
            .and_then(|s| s.resources.iter().find(|e| e.name == name))
        {
            if entry.size > MAX_FILE_PREVIEW_BYTES as i64 {
                self.image_preview = Some(ImagePreviewState::Error {
                    name: name.to_string(),
                    message: self.locale.tr("IMAGE_PREVIEW_TOO_LARGE"),
                });
                return;
            }
        }
        self.text_preview = None;
        self.image_preview = Some(ImagePreviewState::Loading {
            name: name.to_string(),
        });
        self.request_download(session_id, path, name, DownloadPurpose::Image, None, None);
    }

    fn start_text_preview_download(&mut self, session_id: u32, path: &str, name: &str) {
        if !self.callback_connected {
            let msg = self.locale.tr("IMAGE_PREVIEW_NO_CALLBACK");
            self.text_preview = Some(TextPreviewState::Error {
                name: name.to_string(),
                message: msg,
            });
            return;
        }
        if let Some(entry) = self
            .session_by_id(session_id)
            .and_then(|s| s.resources.iter().find(|e| e.name == name))
        {
            if entry.size > MAX_FILE_PREVIEW_BYTES as i64 {
                self.text_preview = Some(TextPreviewState::Error {
                    name: name.to_string(),
                    message: self.locale.tr("TEXT_PREVIEW_TOO_LARGE"),
                });
                return;
            }
        }
        self.image_preview = None;
        self.text_preview = Some(TextPreviewState::Loading {
            name: name.to_string(),
        });
        self.request_download(session_id, path, name, DownloadPurpose::Text, None, None);
    }

    fn start_office_edit(&mut self, session_id: u32, path: &str, name: &str, size: i64) {
        if !self.callback_connected {
            let msg = self.locale.tr("VIDEO_PLAY_NO_CALLBACK");
            if let Some(session) = self.session_mut_by_id(session_id) {
                session.last_error = msg;
            }
            return;
        }
        if size > crate::stream_protocol::MAX_MEDIA_STREAM_BYTES as i64 {
            let msg = self.locale.tr("OFFICE_TOO_LARGE");
            if let Some(session) = self.session_mut_by_id(session_id) {
                session.last_error = msg;
            }
            return;
        }
        let target_token = match self.session_by_id(session_id) {
            Some(s) => s.target_token.clone(),
            None => return,
        };
        if let Some(session) = self.session_mut_by_id(session_id) {
            session.last_error.clear();
        }
        if let Err(e) = crate::session_workspace::ensure_session_temp(&target_token) {
            let msg = format!("{}: {e}", self.locale.tr("OFFICE_OPEN_FAILED"));
            if let Some(session) = self.session_mut_by_id(session_id) {
                session.last_error = msg;
            }
            return;
        }
        eprintln!(
            "[mywebdisk-desktop][app] start_office_edit session_id={} api_path='{}'",
            session_id, path
        );
        self.request_download(session_id, path, name, DownloadPurpose::Office, None, None);
    }

    fn start_local_download(
        &mut self,
        session_id: u32,
        path: &str,
        name: &str,
        dest_path: PathBuf,
        _size: i64,
    ) {
        if !self.callback_connected {
            let msg = self.locale.tr("VIDEO_PLAY_NO_CALLBACK");
            if let Some(session) = self.session_mut_by_id(session_id) {
                session.last_error = msg;
            }
            return;
        }
        if let Some(session) = self.session_mut_by_id(session_id) {
            session.last_error.clear();
        }
        eprintln!(
            "[mywebdisk-desktop][app] start_local_download session_id={} api_path='{}' dest={}",
            session_id,
            path,
            dest_path.display()
        );
        self.request_download(
            session_id,
            path,
            name,
            DownloadPurpose::SaveToLocal,
            None,
            Some(dest_path),
        );
    }

    fn request_delete(&mut self, session_id: u32, path: &str, name: &str) {
        if !self.callback_connected {
            let msg = self.locale.tr("VIDEO_PLAY_NO_CALLBACK");
            if let Some(session) = self.session_mut_by_id(session_id) {
                session.last_error = msg;
            }
            return;
        }
        let req_id = format!("rm-{:016x}", rand::random::<u64>());
        let body = json!({
            "id": req_id,
            "op": "delete",
            "path": path,
        })
        .to_string();
        eprintln!(
            "[mywebdisk-desktop][app] request_delete session_id={} req_id={} path='{}'",
            session_id, req_id, path
        );
        self.pending_delete_requests.insert(
            req_id.clone(),
            PendingDelete {
                session_id,
                api_path: path.to_string(),
                display_name: name.to_string(),
            },
        );
        let ipc_holder = Arc::clone(&self.ipc);
        let req_id_log = req_id.clone();
        thread::spawn(move || {
            if let Ok(guard) = ipc_holder.lock() {
                if let Some(ipc) = guard.as_ref() {
                    if let Err(e) = ipc.send_data(session_id, body.as_bytes()) {
                        eprintln!(
                            "[mywebdisk-desktop][app] request_delete send failed req_id={req_id_log} err={e}"
                        );
                    }
                }
            }
        });
    }

    fn request_mkdir(&mut self, session_id: u32, current_path: &str, folder_name: &str) {
        if !self.callback_connected {
            let msg = self.locale.tr("VIDEO_PLAY_NO_CALLBACK");
            if let Some(session) = self.session_mut_by_id(session_id) {
                session.last_error = msg;
            }
            return;
        }
        let req_id = format!("mk-{:016x}", rand::random::<u64>());
        let api_path = Self::join_api_path(current_path, folder_name);
        let body = json!({
            "id": req_id,
            "op": "mkdir",
            "path": api_path,
        })
        .to_string();
        eprintln!(
            "[mywebdisk-desktop][app] request_mkdir session_id={} req_id={} path='{}'",
            session_id, req_id, current_path
        );
        self.pending_mkdir_requests.insert(
            req_id.clone(),
            PendingMkdir {
                session_id,
                path: current_path.to_string(),
                display_name: folder_name.to_string(),
            },
        );
        let ipc_holder = Arc::clone(&self.ipc);
        let req_id_log = req_id.clone();
        thread::spawn(move || {
            if let Ok(guard) = ipc_holder.lock() {
                if let Some(ipc) = guard.as_ref() {
                    if let Err(e) = ipc.send_data(session_id, body.as_bytes()) {
                        eprintln!(
                            "[mywebdisk-desktop][app] request_mkdir send failed req_id={req_id_log} err={e}"
                        );
                    }
                }
            }
        });
    }

    fn start_media_stream(
        &mut self,
        kind: MediaStreamKind,
        session_id: u32,
        path: &str,
        name: &str,
        size: i64,
    ) {
        if !self.callback_connected {
            self.report_media_stream_error(session_id, kind.tr_no_callback(&self.locale));
            return;
        }
        if size > crate::stream_protocol::MAX_MEDIA_STREAM_BYTES as i64 {
            self.report_media_stream_error(session_id, kind.tr_too_large(&self.locale));
            return;
        }
        let target_token = match self.session_by_id(session_id) {
            Some(s) => s.target_token.clone(),
            None => return,
        };
        if let Some(session) = self.session_mut_by_id(session_id) {
            session.last_error.clear();
        }
        self.cancel_session_media_streams(session_id);
        let temp_dir = match crate::session_workspace::ensure_session_temp(&target_token) {
            Ok(d) => d,
            Err(e) => {
                self.report_media_stream_error(
                    session_id,
                    format!("{}: {e}", kind.tr_error(&self.locale)),
                );
                return;
            }
        };
        crate::session_workspace::clear_session_media_temps(&target_token);
        let req_id = format!("{}-{:016x}", kind.req_id_prefix(), rand::random::<u64>());
        let temp_path =
            crate::session_workspace::temp_media_path(&target_token, &req_id, path, name);
        if let Err(e) = std::fs::File::create(&temp_path) {
            self.report_media_stream_error(
                session_id,
                format!("{}: {e}", kind.tr_error(&self.locale)),
            );
            return;
        }
        self.media_streams.insert(
            req_id.clone(),
            MediaStreamState {
                kind,
                session_id,
                api_path: path.to_string(),
                temp_path: temp_path.clone(),
                total_size: size.max(0) as u64,
                written: 0,
                player_started: false,
                done: false,
                failed: false,
            },
        );
        self.media_play_req_id = Some(req_id.clone());
        self.pending_stream_downloads.insert(
            req_id.clone(),
            PendingStreamDownload {
                session_id,
                temp_path,
                total_size: size.max(0) as u64,
            },
        );
        eprintln!(
            "[mywebdisk-desktop][app] start_media_stream {:?} session_id={} req_id={} api_path='{}' temp={}",
            kind,
            session_id,
            req_id,
            path,
            temp_dir.display()
        );
        self.request_download_stream(session_id, path, &req_id);
    }

    fn request_download_stream(&mut self, session_id: u32, path: &str, req_id: &str) {
        let body = json!({
            "id": req_id,
            "op": "download_stream",
            "path": path,
        })
        .to_string();
        let ipc_holder = Arc::clone(&self.ipc);
        let req_id_log = req_id.to_string();
        thread::spawn(move || {
            if let Ok(guard) = ipc_holder.lock() {
                if let Some(ipc) = guard.as_ref() {
                    if let Err(e) = ipc.send_data(session_id, body.as_bytes()) {
                        eprintln!(
                            "[mywebdisk-desktop][app] request_download_stream send failed req_id={req_id_log} err={e}"
                        );
                    }
                }
            }
        });
    }

    fn request_download(
        &mut self,
        session_id: u32,
        path: &str,
        display_name: &str,
        purpose: DownloadPurpose,
        temp_path: Option<PathBuf>,
        save_dest: Option<PathBuf>,
    ) {
        let req_id = match purpose {
            DownloadPurpose::Office => format!("of-{:016x}", rand::random::<u64>()),
            DownloadPurpose::SaveToLocal => format!("sv-{:016x}", rand::random::<u64>()),
            _ => format!("dl-{:016x}", rand::random::<u64>()),
        };
        let mut temp_path = temp_path;
        let save_dest = save_dest;
        if purpose == DownloadPurpose::Office {
            let token = self
                .session_by_id(session_id)
                .map(|s| s.target_token.as_str())
                .unwrap_or("session");
            let office_temp =
                crate::session_workspace::temp_media_path(token, &req_id, path, display_name);
            if let Err(e) = std::fs::File::create(&office_temp) {
                let msg = format!("{}: {e}", self.locale.tr("OFFICE_OPEN_FAILED"));
                if let Some(session) = self.session_mut_by_id(session_id) {
                    session.last_error = msg;
                }
                return;
            }
            temp_path = Some(office_temp);
        } else if purpose == DownloadPurpose::SaveToLocal {
            if let Some(dest) = save_dest.as_ref() {
                if let Some(parent) = dest.parent() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        let msg = self
                            .locale
                            .tr("FILE_DOWNLOAD_FAILED")
                            .replace("%s", &e.to_string());
                        if let Some(session) = self.session_mut_by_id(session_id) {
                            session.last_error = msg;
                        }
                        return;
                    }
                }
                if let Err(e) = std::fs::File::create(dest) {
                    let msg = self
                        .locale
                        .tr("FILE_DOWNLOAD_FAILED")
                        .replace("%s", &e.to_string());
                    if let Some(session) = self.session_mut_by_id(session_id) {
                        session.last_error = msg;
                    }
                    return;
                }
            }
        }
        let body = json!({
            "id": req_id,
            "op": "download",
            "path": path,
        })
        .to_string();
        eprintln!(
            "[mywebdisk-desktop][app] request_download session_id={} req_id={} path='{}'",
            session_id, req_id, path
        );
        self.pending_download_requests.insert(
            req_id.clone(),
            PendingDownload {
                session_id,
                display_name: display_name.to_string(),
                path: path.to_string(),
                purpose,
                temp_path,
                save_dest,
            },
        );
        let ipc_holder = Arc::clone(&self.ipc);
        let req_id_log = req_id.clone();
        thread::spawn(move || {
            let send_result = match ipc_holder.lock() {
                Ok(guard) => {
                    if let Some(ipc) = guard.as_ref() {
                        ipc.send_data(session_id, body.as_bytes())
                    } else {
                        Err("webrpc client is not ready".into())
                    }
                }
                Err(_) => Err("webrpc client lock failed".into()),
            };
            if let Err(e) = send_result {
                eprintln!(
                    "[mywebdisk-desktop][app] request_download send failed req_id={req_id_log} err={e}"
                );
            }
        });
    }

    fn handle_download_response(&mut self, session_id: u32, resp: &RpcResponse) {
        let Some(pending) = self.pending_download_requests.remove(&resp.id) else {
            return;
        };
        if pending.session_id != session_id {
            eprintln!(
                "[mywebdisk-desktop][app] download response session mismatch id={} expected={} got={}",
                resp.id, pending.session_id, session_id
            );
            return;
        }
        if !resp.ok {
            let message = resp.error.clone().unwrap_or_else(|| match pending.purpose {
                DownloadPurpose::Text => self.locale.tr("TEXT_PREVIEW_ERROR"),
                DownloadPurpose::Office => self.locale.tr("OFFICE_OPEN_FAILED"),
                DownloadPurpose::SaveToLocal => self.locale.tr("FILE_DOWNLOAD_FAILED").replace("%s", ""),
                DownloadPurpose::Image => self.locale.tr("IMAGE_PREVIEW_ERROR"),
            });
            self.set_download_error(pending, message);
            return;
        }
        eprintln!(
            "[mywebdisk-desktop][app] download ack ok id={} awaiting file stream session_id={}",
            resp.id, session_id
        );
        let write_path = match pending.purpose {
            DownloadPurpose::Office => pending.temp_path.clone(),
            DownloadPurpose::SaveToLocal => pending.save_dest.clone(),
            _ => None,
        };
        self.awaiting_file_downloads.insert(
            session_id,
            IncomingFileDownload {
                req_id: resp.id.clone(),
                pending,
                write_path,
                memory: Vec::new(),
                bytes_received: 0,
                file_sent: false,
                expected_size: None,
            },
        );
    }

    fn handle_file_sent_notification(&mut self, session_id: u32, resp: &RpcResponse) {
        let Some(incoming) = self.awaiting_file_downloads.get_mut(&session_id) else {
            return;
        };
        if incoming.req_id != resp.id {
            eprintln!(
                "[mywebdisk-desktop][app] fileSent id mismatch expected={} got={}",
                incoming.req_id, resp.id
            );
            return;
        }
        if !resp.ok {
            let incoming = self.awaiting_file_downloads.remove(&session_id).unwrap();
            let message = resp.error.clone().unwrap_or_else(|| match incoming.pending.purpose {
                DownloadPurpose::Text => self.locale.tr("TEXT_PREVIEW_ERROR"),
                DownloadPurpose::Office => self.locale.tr("OFFICE_OPEN_FAILED"),
                DownloadPurpose::SaveToLocal => {
                    self.locale.tr("FILE_DOWNLOAD_FAILED").replace("%s", "")
                }
                DownloadPurpose::Image => self.locale.tr("IMAGE_PREVIEW_ERROR"),
            });
            if let Some(path) = incoming.write_path {
                let _ = std::fs::remove_file(path);
            }
            self.set_download_error(incoming.pending, message);
            return;
        }
        incoming.file_sent = true;
        incoming.expected_size = resp
            .data
            .as_ref()
            .and_then(|d| d.get("size"))
            .and_then(|v| v.as_u64());
        eprintln!(
            "[mywebdisk-desktop][app] fileSent signaled session_id={} req_id={} expected={:?} received={}",
            session_id,
            resp.id,
            incoming.expected_size,
            incoming.bytes_received
        );
        self.try_finalize_file_download(session_id);
    }

    fn try_finalize_file_download(&mut self, session_id: u32) {
        let ready = self
            .awaiting_file_downloads
            .get(&session_id)
            .is_some_and(|i| {
                i.file_sent
                    && i
                        .expected_size
                        .is_some_and(|exp| i.bytes_received >= exp)
            });
        if !ready {
            return;
        }
        let Some(incoming) = self.awaiting_file_downloads.remove(&session_id) else {
            return;
        };
        eprintln!(
            "[mywebdisk-desktop][app] file download complete session_id={} req_id={} bytes={}",
            session_id, incoming.req_id, incoming.bytes_received
        );
        let pending = incoming.pending;
        match pending.purpose {
            DownloadPurpose::Image => {
                self.finish_image_preview_bytes(pending.display_name, incoming.memory);
            }
            DownloadPurpose::Text => {
                self.finish_text_preview_bytes(
                    pending.display_name,
                    pending.path,
                    pending.session_id,
                    incoming.memory,
                );
            }
            DownloadPurpose::Office => {
                if let Some(temp_path) = pending.temp_path {
                    self.open_office_from_temp(
                        pending.session_id,
                        &pending.path,
                        &pending.display_name,
                        &temp_path,
                    );
                }
            }
            DownloadPurpose::SaveToLocal => {
                if let Some(dest) = pending.save_dest {
                    let msg = self
                        .locale
                        .tr("FILE_DOWNLOAD_OK")
                        .replace("%s", &dest.display().to_string());
                    if let Some(session) = self.session_mut_by_id(pending.session_id) {
                        session.last_error = msg;
                    }
                }
            }
        }
    }

    fn handle_stream_download_response(&mut self, session_id: u32, resp: &RpcResponse) {
        let Some(pending) = self.pending_stream_downloads.remove(&resp.id) else {
            return;
        };
        if pending.session_id != session_id {
            return;
        }
        if self.cancelled_stream_req_ids.contains(&resp.id) {
            return;
        }
        if !resp.ok {
            let message = resp
                .error
                .clone()
                .unwrap_or_else(|| self.locale.tr("VIDEO_PLAY_ERROR"));
            self.report_media_stream_error(pending.session_id, message);
            return;
        }
        let total_size = resp
            .data
            .as_ref()
            .and_then(|d| d.get("totalSize"))
            .and_then(|v| v.as_u64())
            .unwrap_or(pending.total_size);
        if let Some(ms) = self.media_streams.get_mut(&resp.id) {
            ms.total_size = total_size;
        }
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&pending.temp_path) {
            if total_size > 0 {
                let _ = file.set_len(total_size);
            }
        }
        eprintln!(
            "[mywebdisk-desktop][app] download_stream ack ok id={} total_size={}",
            resp.id, total_size
        );
    }

    fn handle_stream_frame(&mut self, session_id: u32, payload: &[u8]) {
        let Some(Ok(frame)) = crate::stream_protocol::decode_stream_frame(payload) else {
            if let Some(Err(e)) = crate::stream_protocol::decode_stream_frame(payload) {
                eprintln!("[mywebdisk-desktop][app] stream frame decode err: {e}");
            }
            return;
        };
        if self.cancelled_stream_req_ids.contains(&frame.req_id) {
            return;
        }

        let Some(ms) = self.media_streams.get_mut(&frame.req_id) else {
            return;
        };
        if ms.session_id != session_id {
            return;
        }
        let kind = ms.kind;
        let req_id = frame.req_id.clone();

        match frame.flag {
            crate::stream_protocol::STREAM_FLAG_ERROR => {
                let msg = String::from_utf8_lossy(&frame.data).into_owned();
                let message = if msg.is_empty() {
                    kind.tr_error(&self.locale)
                } else {
                    msg
                };
                self.report_media_stream_error(session_id, message);
                return;
            }
            crate::stream_protocol::STREAM_FLAG_DONE => {
                ms.done = true;
                ms.written = ms.written.max(frame.offset);
            }
            crate::stream_protocol::STREAM_FLAG_CHUNK => {
                if frame.data.is_empty() {
                    return;
                }
                let path = ms.temp_path.clone();
                if let Err(e) = Self::write_media_chunk_at(&path, frame.offset, &frame.data) {
                    ms.failed = true;
                    self.report_media_stream_error(
                        session_id,
                        format!("{}: {e}", kind.tr_error(&self.locale)),
                    );
                    return;
                }
                ms.written = ms.written.max(frame.offset + frame.data.len() as u64);
            }
            _ => return,
        }
        if self.media_play_req_id.as_deref() == Some(req_id.as_str()) {
            self.maybe_launch_media_player(&req_id);
        }
        if self
            .media_streams
            .get(&req_id)
            .is_some_and(|m| m.done && m.player_started && !m.failed)
        {
            self.media_streams.remove(&req_id);
            if self.media_play_req_id.as_deref() == Some(req_id.as_str()) {
                self.media_play_req_id = None;
            }
        }
    }

    fn write_media_chunk_at(path: &PathBuf, offset: u64, data: &[u8]) -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new().write(true).open(path)?;
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(data)?;
        file.flush()?;
        Ok(())
    }

    fn open_office_from_temp(
        &mut self,
        session_id: u32,
        api_path: &str,
        display_name: &str,
        temp_path: &PathBuf,
    ) {
        let Ok(meta) = std::fs::metadata(temp_path) else {
            return;
        };
        let baseline_len = meta.len();
        let baseline_modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let key = Self::office_edit_key(session_id, api_path);
        self.office_edits.insert(
            key,
            OfficeEditState {
                session_id,
                api_path: api_path.to_string(),
                display_name: display_name.to_string(),
                temp_path: temp_path.clone(),
                baseline_len,
                baseline_modified,
            },
        );
        let open_path = std::fs::canonicalize(temp_path).unwrap_or_else(|_| temp_path.clone());
        if let Err(e) = open::that(&open_path) {
            let msg = format!("{}: {e}", self.locale.tr("OFFICE_OPEN_FAILED"));
            if let Some(session) = self.session_mut_by_id(session_id) {
                session.last_error = msg;
            }
            return;
        }
        eprintln!(
            "[mywebdisk-desktop][app] office opened api_path='{}' file={}",
            api_path,
            open_path.display()
        );
    }

    fn maybe_launch_media_player(&mut self, req_id: &str) {
        if self.cancelled_stream_req_ids.contains(req_id) {
            return;
        }
        let Some(ms) = self.media_streams.get_mut(req_id) else {
            return;
        };
        if ms.failed || ms.player_started {
            return;
        }
        let min_start = ms.kind.min_start_bytes();
        let ready = ms.done || (ms.written >= min_start && ms.written > 0);
        if !ready {
            return;
        }
        let temp_path = ms.temp_path.clone();
        let api_path = ms.api_path.clone();
        let sid = ms.session_id;
        let kind = ms.kind;
        let open_path = std::fs::canonicalize(&temp_path).unwrap_or(temp_path.clone());
        match open::that(&open_path) {
            Ok(()) => {
                if let Some(ms) = self.media_streams.get_mut(req_id) {
                    ms.player_started = true;
                }
                eprintln!(
                    "[mywebdisk-desktop][app] launched system player {:?} api_path='{}' file={}",
                    kind,
                    api_path,
                    open_path.display()
                );
            }
            Err(e) => {
                self.report_media_stream_error(
                    sid,
                    format!("{}: {e}", kind.tr_error(&self.locale)),
                );
            }
        }
    }

    fn handle_callback_file(&mut self, session_id: u32, file_name: &[u8], data: &[u8]) {
        eprintln!(
            "[mywebdisk-desktop][app] handle_callback_file session_id={} name={} len={}",
            session_id,
            String::from_utf8_lossy(file_name),
            data.len()
        );
        let Some(incoming) = self.awaiting_file_downloads.get_mut(&session_id) else {
            eprintln!(
                "[mywebdisk-desktop][app] unexpected file bucket session_id={}",
                session_id
            );
            return;
        };
        if let Err(e) = Self::append_file_bucket(incoming, data) {
            let incoming = self.awaiting_file_downloads.remove(&session_id).unwrap();
            let message = match incoming.pending.purpose {
                DownloadPurpose::Office => {
                    format!("{}: {e}", self.locale.tr("OFFICE_OPEN_FAILED"))
                }
                DownloadPurpose::SaveToLocal => self
                    .locale
                    .tr("FILE_DOWNLOAD_FAILED")
                    .replace("%s", &e),
                DownloadPurpose::Image => self.locale.tr("IMAGE_PREVIEW_ERROR"),
                DownloadPurpose::Text => self.locale.tr("TEXT_PREVIEW_ERROR"),
            };
            if let Some(path) = incoming.write_path {
                let _ = std::fs::remove_file(path);
            }
            self.set_download_error(incoming.pending, message);
            return;
        }
        let total = self
            .awaiting_file_downloads
            .get(&session_id)
            .map(|i| i.bytes_received)
            .unwrap_or(0);
        eprintln!(
            "[mywebdisk-desktop][app] file bucket appended session_id={} total_bytes={}",
            session_id, total
        );
        self.try_finalize_file_download(session_id);
    }

    fn set_download_error(&mut self, pending: PendingDownload, message: String) {
        match pending.purpose {
            DownloadPurpose::Image => {
                self.image_preview = Some(ImagePreviewState::Error {
                    name: pending.display_name,
                    message,
                });
            }
            DownloadPurpose::Text => {
                self.text_preview = Some(TextPreviewState::Error {
                    name: pending.display_name,
                    message,
                });
            }
            DownloadPurpose::Office => {
                if let Some(session) = self.session_mut_by_id(pending.session_id) {
                    session.last_error = message;
                }
            }
            DownloadPurpose::SaveToLocal => {
                if let Some(session) = self.session_mut_by_id(pending.session_id) {
                    session.last_error = message;
                }
            }
        }
    }

    fn handle_delete_response(&mut self, session_id: u32, resp: &RpcResponse) {
        let Some(pending) = self.pending_delete_requests.remove(&resp.id) else {
            return;
        };
        if pending.session_id != session_id {
            eprintln!(
                "[mywebdisk-desktop][app] delete response session mismatch id={} expected={} got={}",
                resp.id, pending.session_id, session_id
            );
            return;
        }
        if !resp.ok {
            let detail = resp.error.clone().unwrap_or_default();
            let message = if detail.is_empty() {
                self.locale
                    .tr("FILE_DELETE_FAILED")
                    .replace("%s", &pending.display_name)
            } else {
                self.locale.tr("FILE_DELETE_FAILED").replace("%s", &detail)
            };
            if let Some(session) = self.session_mut_by_id(session_id) {
                session.last_error = message;
            }
            return;
        }
        let msg = self
            .locale
            .tr("FILE_DELETE_OK")
            .replace("%s", &pending.display_name);
        let current_path = self
            .session_by_id(session_id)
            .map(|s| s.current_path.clone())
            .unwrap_or_default();
        if let Some(session) = self.session_mut_by_id(session_id) {
            session.last_error = msg;
        }
        eprintln!(
            "[mywebdisk-desktop][app] delete ok api_path='{}' name='{}'",
            pending.api_path, pending.display_name
        );
        self.request_list(session_id, &current_path);
    }

    fn handle_mkdir_response(&mut self, session_id: u32, resp: &RpcResponse) {
        let Some(pending) = self.pending_mkdir_requests.remove(&resp.id) else {
            return;
        };
        if pending.session_id != session_id {
            eprintln!(
                "[mywebdisk-desktop][app] mkdir response session mismatch id={} expected={} got={}",
                resp.id, pending.session_id, session_id
            );
            return;
        }
        if !resp.ok {
            let detail = resp.error.clone().unwrap_or_default();
            let msg = if detail.is_empty() {
                self.locale.tr("MKDIR_FAILED").replace("%s", &pending.display_name)
            } else {
                self.locale.tr("MKDIR_FAILED").replace("%s", &detail)
            };
            if let Some(session) = self.session_mut_by_id(session_id) {
                session.last_error = msg;
            }
            return;
        }
        let msg = self
            .locale
            .tr("MKDIR_OK")
            .replace("%s", &pending.display_name);
        if let Some(session) = self.session_mut_by_id(session_id) {
            session.last_error = msg;
        }
        self.request_list(session_id, &pending.path);
    }

    fn upload_office_file_blocking(
        &mut self,
        session_id: u32,
        edit: &OfficeEditState,
    ) -> Result<(), String> {
        let upload_path = match crate::file_lock::prepare_file_for_upload(&edit.temp_path) {
            Ok(p) => p,
            Err(e) => {
                crate::file_lock::force_release(&edit.temp_path).ok();
                thread::sleep(Duration::from_millis(400));
                crate::file_lock::prepare_file_for_upload(&edit.temp_path).map_err(|e2| {
                    format!("{}: {e}; retry: {e2}", self.locale.tr("OFFICE_OPEN_FAILED"))
                })?
            }
        };
        let staging = upload_path != edit.temp_path;
        let req_id = format!("up-{:016x}", rand::random::<u64>());
        self.upload_waits.insert(req_id.clone(), UploadWait::default());

        let body = json!({
            "id": req_id,
            "op": "upload",
            "path": edit.api_path,
        })
        .to_string();

        {
            let ipc = self
                .ipc
                .lock()
                .map_err(|_| "webrpc lock failed".to_string())?;
            let ipc = ipc
                .as_ref()
                .ok_or_else(|| "webrpc client is not ready".to_string())?;
            ipc.send_data_with_timeout(session_id, body.as_bytes(), SEND_DATA_UPLOAD_TIMEOUT_MS)
                .map_err(|e| format!("upload prepare: {e}"))?;
        }

        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if Instant::now() >= deadline {
                self.upload_waits.remove(&req_id);
                return Err("upload prepare timeout".into());
            }
            self.poll_callback_events();
            if let Some(wait) = self.upload_waits.get(&req_id) {
                if wait.prepare_done {
                    if !wait.prepare_ok {
                        let err = wait.error.clone();
                        self.upload_waits.remove(&req_id);
                        return Err(if err.is_empty() {
                            "upload prepare rejected".into()
                        } else {
                            err
                        });
                    }
                    break;
                }
            }
            thread::sleep(Duration::from_millis(20));
        }

        let mut send_err = None;
        for attempt in 0..4 {
            let result = {
                let ipc = self.ipc.lock().map_err(|_| "webrpc lock failed".to_string())?;
                let ipc = ipc
                    .as_ref()
                    .ok_or_else(|| "webrpc client is not ready".to_string())?;
                ipc.send_file(session_id, &upload_path)
            };
            match result {
                Ok(()) => {
                    send_err = None;
                    break;
                }
                Err(e) => {
                    send_err = Some(e);
                    eprintln!(
                        "[mywebdisk-desktop][app] SendFile attempt {} failed: {}",
                        attempt + 1,
                        send_err.as_ref().unwrap()
                    );
                    crate::file_lock::force_release(&edit.temp_path).ok();
                    thread::sleep(Duration::from_millis(500));
                }
            }
        }
        if let Some(e) = send_err {
            self.upload_waits.remove(&req_id);
            if staging {
                let _ = std::fs::remove_file(&upload_path);
            }
            return Err(e);
        }

        let file_size = std::fs::metadata(&upload_path)
            .map(|m| m.len())
            .unwrap_or(0);
        self.send_upload_done(session_id, &req_id, file_size)
            .map_err(|e| format!("upload finalize: {e}"))?;

        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            if Instant::now() >= deadline {
                self.upload_waits.remove(&req_id);
                if staging {
                    let _ = std::fs::remove_file(&upload_path);
                }
                return Err("upload complete timeout".into());
            }
            self.poll_callback_events();
            if let Some(wait) = self.upload_waits.get(&req_id) {
                if wait.complete_done {
                    let ok = wait.complete_ok;
                    let err = wait.error.clone();
                    self.upload_waits.remove(&req_id);
                    if staging {
                        let _ = std::fs::remove_file(&upload_path);
                    }
                    if ok {
                        return Ok(());
                    }
                    return Err(if err.is_empty() {
                        "upload failed on server".into()
                    } else {
                        err
                    });
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn send_upload_done(
        &self,
        session_id: u32,
        req_id: &str,
        size: u64,
    ) -> Result<(), String> {
        let body = json!({
            "id": req_id,
            "op": "upload_done",
            "size": size,
        })
        .to_string();
        let ipc = self
            .ipc
            .lock()
            .map_err(|_| "webrpc lock failed".to_string())?;
        let ipc = ipc
            .as_ref()
            .ok_or_else(|| "webrpc client is not ready".to_string())?;
        ipc.send_data_with_timeout(session_id, body.as_bytes(), SEND_DATA_UPLOAD_TIMEOUT_MS)
            .map_err(|e| format!("upload_done: {e}"))
    }

    fn start_finder_upload(&mut self, session_id: u32, current_path: &str) {
        if self.finder_upload.is_some() {
            return;
        }
        let Some(local_path) = rfd::FileDialog::new().pick_file() else {
            return;
        };
        let file_name = local_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("upload")
            .to_string();
        let total_size = std::fs::metadata(&local_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let api_path = Self::join_api_path(current_path, &file_name);
        let req_id = format!("up-{:016x}", rand::random::<u64>());
        self.upload_waits.insert(req_id.clone(), UploadWait::default());

        let body = json!({
            "id": req_id,
            "op": "upload",
            "path": api_path,
        })
        .to_string();

        if let Err(e) = (|| {
            let ipc = self
                .ipc
                .lock()
                .map_err(|_| "webrpc lock failed".to_string())?;
            let ipc = ipc
                .as_ref()
                .ok_or_else(|| "webrpc client is not ready".to_string())?;
            ipc.send_data_with_timeout(session_id, body.as_bytes(), SEND_DATA_UPLOAD_TIMEOUT_MS)
                .map_err(|e| format!("upload prepare: {e}"))
        })() {
            self.upload_waits.remove(&req_id);
            self.finder_upload = Some(FinderUploadState {
                session_id,
                current_path: current_path.to_string(),
                local_path,
                file_name,
                total_size,
                req_id,
                phase: FinderUploadPhase::Failed,
                progress: 0.0,
                send_started: None,
                error: e,
                show_success_dialog: false,
            });
            return;
        }

        eprintln!(
            "[mywebdisk-desktop][app] finder upload start session_id={session_id} api_path='{api_path}' size={total_size}"
        );
        self.finder_upload = Some(FinderUploadState {
            session_id,
            current_path: current_path.to_string(),
            local_path,
            file_name,
            total_size,
            req_id,
            phase: FinderUploadPhase::Preparing,
            progress: 0.05,
            send_started: None,
            error: String::new(),
            show_success_dialog: false,
        });
    }

    fn poll_finder_upload(&mut self, ctx: &egui::Context) {
        let mut refresh_list: Option<(u32, String)> = None;
        let mut spawn_send: Option<(u32, PathBuf)> = None;
        let mut send_upload_done: Option<(u32, String, u64)> = None;
        let mut fail_msg: Option<String> = None;

        if let Some(state) = self.finder_upload.as_mut() {
            match state.phase {
                FinderUploadPhase::Preparing => {
                    if let Some(wait) = self.upload_waits.get(&state.req_id) {
                        if wait.prepare_done {
                            if wait.prepare_ok {
                                state.phase = FinderUploadPhase::Sending;
                                state.progress = 0.1;
                                state.send_started = Some(Instant::now());
                                spawn_send = Some((state.session_id, state.local_path.clone()));
                            } else {
                                state.phase = FinderUploadPhase::Failed;
                                state.error = if wait.error.is_empty() {
                                    self.locale.tr("UPLOAD_FAILED").to_string()
                                } else {
                                    wait.error.clone()
                                };
                                self.upload_waits.remove(&state.req_id);
                            }
                        }
                    }
                }
                FinderUploadPhase::Sending => {
                    if let Some(started) = state.send_started {
                        let est_secs = (state.total_size as f64 / (4.0 * 1024.0 * 1024.0)).max(1.0);
                        let elapsed = started.elapsed().as_secs_f64();
                        let frac = (elapsed / est_secs).min(0.92) as f32;
                        state.progress = 0.1 + frac * 0.75;
                    }
                    ctx.request_repaint_after(Duration::from_millis(80));

                    if let Some(rx) = self.upload_send_rx.as_ref() {
                        if let Ok(result) = rx.try_recv() {
                            self.upload_send_rx = None;
                            match result {
                                Ok(()) => {
                                    state.phase = FinderUploadPhase::Finalizing;
                                    state.progress = 0.9;
                                    send_upload_done = Some((
                                        state.session_id,
                                        state.req_id.clone(),
                                        state.total_size,
                                    ));
                                }
                                Err(e) => {
                                    state.phase = FinderUploadPhase::Failed;
                                    state.error = e;
                                    self.upload_waits.remove(&state.req_id);
                                }
                            }
                        }
                    }
                }
                FinderUploadPhase::Finalizing => {
                    if let Some(wait) = self.upload_waits.get(&state.req_id) {
                        if wait.complete_done {
                            let ok = wait.complete_ok;
                            let err = wait.error.clone();
                            self.upload_waits.remove(&state.req_id);
                            if ok {
                                state.phase = FinderUploadPhase::Success;
                                state.progress = 1.0;
                                state.show_success_dialog = true;
                                refresh_list =
                                    Some((state.session_id, state.current_path.clone()));
                            } else {
                                state.phase = FinderUploadPhase::Failed;
                                state.error = if err.is_empty() {
                                    self.locale.tr("UPLOAD_FAILED").to_string()
                                } else {
                                    err
                                };
                            }
                        }
                    }
                }
                FinderUploadPhase::Success | FinderUploadPhase::Failed => {}
            }
        }

        if let Some((session_id, path)) = spawn_send {
            let ipc = Arc::clone(&self.ipc);
            let (tx, rx) = mpsc::channel();
            self.upload_send_rx = Some(rx);
            thread::spawn(move || {
                let result = (|| {
                    let guard = ipc
                        .lock()
                        .map_err(|_| "webrpc lock failed".to_string())?;
                    let ipc = guard
                        .as_ref()
                        .ok_or_else(|| "webrpc client is not ready".to_string())?;
                    ipc.send_file(session_id, &path)
                })();
                let _ = tx.send(result);
            });
        }

        if let Some((session_id, req_id, size)) = send_upload_done {
            if let Err(e) = self.send_upload_done(session_id, &req_id, size) {
                fail_msg = Some(e);
            }
        }

        if let Some(e) = fail_msg {
            if let Some(state) = self.finder_upload.as_mut() {
                state.phase = FinderUploadPhase::Failed;
                state.error = e;
            }
            if let Some(state) = self.finder_upload.as_ref() {
                self.upload_waits.remove(&state.req_id);
            }
        }

        if let Some((session_id, path)) = refresh_list {
            self.request_list(session_id, &path);
        }
    }

    fn draw_finder_upload_ui(&mut self, ctx: &egui::Context) {
        let Some(state) = self.finder_upload.as_ref() else {
            return;
        };

        if state.show_success_dialog {
            let file_name = state.file_name.clone();
            let mut open = true;
            egui::Window::new(self.locale.tr("UPLOAD_SUCCESS_TITLE"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!(
                        "{}: {}",
                        self.locale.tr("UPLOAD_SUCCESS"),
                        file_name
                    ));
                    ui.add_space(8.0);
                    if ui.button(self.locale.tr("UPLOAD_OK")).clicked() {
                        open = false;
                    }
                });
            if !open {
                self.finder_upload = None;
                self.upload_send_rx = None;
            }
            return;
        }

        if state.phase == FinderUploadPhase::Failed {
            let err = state.error.clone();
            let mut open = true;
            egui::Window::new(self.locale.tr("UPLOAD_FAILED_TITLE"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new(err)
                            .size(12.0)
                            .color(crate::theme::STATUS_ERROR),
                    );
                    ui.add_space(8.0);
                    if ui.button(self.locale.tr("UPLOAD_OK")).clicked() {
                        open = false;
                    }
                });
            if !open {
                self.finder_upload = None;
                self.upload_send_rx = None;
            }
            return;
        }

        if matches!(
            state.phase,
            FinderUploadPhase::Preparing
                | FinderUploadPhase::Sending
                | FinderUploadPhase::Finalizing
        ) {
            let file_name = state.file_name.clone();
            let progress = state.progress;
            let status = self.locale.tr("UPLOAD_PROGRESS");
            egui::Window::new(self.locale.tr("UPLOAD_PROGRESS_TITLE"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(&file_name);
                    ui.add_space(6.0);
                    ui.add(
                        egui::ProgressBar::new(progress)
                            .text(status)
                            .animate(progress < 0.95),
                    );
                });
        }
    }

    fn draw_new_entry_modal(&mut self, ctx: &egui::Context) {
        let Some(mut state) = self.new_entry_modal.take() else {
            return;
        };
        if !state.open {
            return;
        }

        let title = match state.kind {
            NewEntryKind::Folder => self.locale.tr("NEW_FOLDER_TITLE"),
            NewEntryKind::File => self.locale.tr("NEW_FILE_TITLE"),
        };

        let mut keep_open = true;
        let mut submit: Option<(NewEntryKind, u32, String, String)> = None;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .frame(crate::theme::neon_modal_frame())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(self.locale.tr("NEW_NAME_LABEL"))
                        .color(crate::theme::NEON_POPUP_TEXT),
                );
                ui.add_space(6.0);
                let resp = crate::theme::neon_input_frame()
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut state.name)
                                .hint_text(self.locale.tr("NEW_NAME_HINT"))
                                .desired_width(252.0)
                                .frame(false),
                        )
                    })
                    .inner;
                if resp.changed() {
                    state.error.clear();
                }
                if !state.error.is_empty() {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(&state.error)
                            .size(11.0)
                            .color(crate::theme::STATUS_ERROR),
                    );
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(crate::theme::neon_secondary_button(
                            self.locale.tr("NEW_CANCEL"),
                        ))
                        .clicked()
                    {
                        keep_open = false;
                    }
                    let ok_clicked = ui
                        .add(crate::theme::neon_primary_button(self.locale.tr("NEW_OK")))
                        .clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ok_clicked {
                        let name = state.name.trim().to_string();
                        if name.is_empty() {
                            state.error = self.locale.tr("NEW_NAME_EMPTY").to_string();
                            return;
                        }
                        if name.contains('/') || name.contains('\\') {
                            state.error = self.locale.tr("NEW_NAME_INVALID").to_string();
                            return;
                        }
                        if state.kind == NewEntryKind::File {
                            let ext_ok = std::path::Path::new(&name)
                                .extension()
                                .and_then(|e| e.to_str())
                                .is_some_and(|s| !s.is_empty());
                            if !ext_ok {
                                state.error = self.locale.tr("NEW_FILE_NEED_EXT").to_string();
                                return;
                            }
                        }
                        submit = Some((
                            state.kind,
                            state.session_id,
                            state.current_path.clone(),
                            name,
                        ));
                        keep_open = false;
                    }
                });
            });

        if let Some((kind, session_id, current_path, name)) = submit {
            match kind {
                NewEntryKind::Folder => self.request_mkdir(session_id, &current_path, &name),
                NewEntryKind::File => self.request_create_file(session_id, &current_path, &name),
            }
        }

        if keep_open {
            state.open = true;
            self.new_entry_modal = Some(state);
        }
    }

    fn draw_finder_blank_menu(&mut self, ctx: &egui::Context) {
        let Some(mut menu) = self.finder_blank_menu.take() else {
            return;
        };

        let mut keep_open = true;
        let mut picked: Option<NewEntryKind> = None;
        let mut main_rect: Option<egui::Rect> = None;
        let mut sub_rect: Option<egui::Rect> = None;

        let main = egui::Area::new(egui::Id::new("finder_blank_ctx_menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(menu.pos)
            .show(ctx, |ui| {
                crate::theme::neon_popup_frame().show(ui, |ui| {
                    let label = format!("{}  ▸", self.locale.tr("CTX_NEW"));
                    let resp = ui.add(crate::theme::neon_menu_button(label));
                    if resp.hovered() {
                        menu.show_new_submenu = true;
                    }
                    main_rect = Some(ui.min_rect());
                });
            });
        main_rect.get_or_insert(main.response.rect);

        if menu.show_new_submenu {
            let sub_pos = egui::pos2(main.response.rect.max.x + 6.0, main.response.rect.min.y);
            let sub = egui::Area::new(egui::Id::new("finder_blank_ctx_submenu"))
                .order(egui::Order::Foreground)
                .fixed_pos(sub_pos)
                .show(ctx, |ui| {
                    crate::theme::neon_popup_frame().show(ui, |ui| {
                        if ui
                            .add(crate::theme::neon_menu_button(self.locale.tr("CTX_NEW_FOLDER")))
                            .clicked()
                        {
                            picked = Some(NewEntryKind::Folder);
                        }
                        if ui
                            .add(crate::theme::neon_menu_button(self.locale.tr("CTX_NEW_FILE")))
                            .clicked()
                        {
                            picked = Some(NewEntryKind::File);
                        }
                        sub_rect = Some(ui.min_rect());
                    });
                });
            sub_rect.get_or_insert(sub.response.rect);
        }

        let any_click = ctx.input(|i| i.pointer.any_click());
        let pointer_pos = ctx.input(|i| i.pointer.interact_pos());
        if any_click {
            let in_main = main_rect
                .zip(pointer_pos)
                .is_some_and(|(r, p)| r.contains(p));
            let in_sub = sub_rect
                .zip(pointer_pos)
                .is_some_and(|(r, p)| r.contains(p));
            if !in_main && !in_sub {
                keep_open = false;
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            keep_open = false;
        }

        if let Some(kind) = picked {
            self.new_entry_modal = Some(NewEntryModalState {
                session_id: menu.session_id,
                current_path: menu.current_path.clone(),
                kind,
                name: String::new(),
                error: String::new(),
                open: true,
            });
            keep_open = false;
        }

        if keep_open {
            self.finder_blank_menu = Some(menu);
        }
    }

    fn sync_office_edits_for_session(&mut self, session_id: u32) {
        let entries: Vec<OfficeEditState> = self
            .office_edits
            .values()
            .filter(|e| e.session_id == session_id)
            .cloned()
            .collect();
        for edit in entries {
            if !Self::office_file_modified(&edit) {
                eprintln!(
                    "[mywebdisk-desktop][app] office skip unchanged api_path='{}'",
                    edit.api_path
                );
                continue;
            }
            eprintln!(
                "[mywebdisk-desktop][app] office syncing api_path='{}' file={}",
                edit.api_path,
                edit.temp_path.display()
            );
            match self.upload_office_file_blocking(session_id, &edit) {
                Ok(()) => {
                    let msg = self
                        .locale
                        .tr("OFFICE_SYNC_OK")
                        .replace("%s", &edit.display_name);
                    if let Some(session) = self.session_mut_by_id(session_id) {
                        session.last_error = msg;
                    }
                    eprintln!(
                        "[mywebdisk-desktop][app] office sync ok api_path='{}'",
                        edit.api_path
                    );
                }
                Err(e) => {
                    let msg = self.locale.tr("OFFICE_SYNC_FAILED").replace("%s", &e);
                    if let Some(session) = self.session_mut_by_id(session_id) {
                        session.last_error = msg;
                    }
                    eprintln!(
                        "[mywebdisk-desktop][app] office sync failed api_path='{}' err={e}",
                        edit.api_path
                    );
                }
            }
        }
        self.office_edits
            .retain(|_, e| e.session_id != session_id);
    }

    fn finish_image_preview_bytes(&mut self, name: String, data: Vec<u8>) {
        if data.len() > MAX_FILE_PREVIEW_BYTES {
            self.image_preview = Some(ImagePreviewState::Error {
                name,
                message: self.locale.tr("IMAGE_PREVIEW_TOO_LARGE"),
            });
            return;
        }
        match image::load_from_memory(&data) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let color_image =
                    egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                self.image_preview = Some(ImagePreviewState::Ready {
                    name,
                    color_image: Some(color_image),
                    texture: None,
                });
            }
            Err(e) => {
                eprintln!("[mywebdisk-desktop][app] image decode failed: {e}");
                self.image_preview = Some(ImagePreviewState::Error {
                    name,
                    message: format!("{} ({e})", self.locale.tr("IMAGE_PREVIEW_ERROR")),
                });
            }
        }
    }

    fn draw_image_preview(&mut self, ctx: &egui::Context) {
        let Some(state) = self.image_preview.as_mut() else {
            return;
        };

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.image_preview = None;
            return;
        }

        if let ImagePreviewState::Ready {
            color_image,
            texture,
            name,
        } = state
        {
            if texture.is_none() {
                if let Some(ci) = color_image.take() {
                    *texture = Some(ctx.load_texture(
                        format!("img_preview_{name}"),
                        ci,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
        }

        let screen = ctx.screen_rect();
        let mut request_close = false;

        // 遮罩仅绘制、不拦截点击（此前 Foreground 全屏 Area 会盖住预览窗和标题栏关闭钮）
        let backdrop_layer =
            egui::LayerId::new(egui::Order::Middle, egui::Id::new("image_preview_backdrop"));
        ctx.layer_painter(backdrop_layer).rect_filled(
            screen,
            0.0,
            egui::Color32::from_black_alpha(120),
        );

        let title = match state {
            ImagePreviewState::Loading { name } => name.clone(),
            ImagePreviewState::Ready { name, .. } => name.clone(),
            ImagePreviewState::Error { name, .. } => name.clone(),
        };
        let close_tip = self.locale.tr("IMAGE_PREVIEW_CLOSE");
        let mut open = true;
        egui::Window::new("image_preview")
            .open(&mut open)
            .order(egui::Order::Foreground)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(crate::theme::MAC_PANEL)
                    .stroke(egui::Stroke::new(1.0, crate::theme::MAC_BORDER))
                    .inner_margin(egui::Margin::symmetric(12, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(&title);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::theme::mac_toolbar_close_button(ui, &close_tip, true).clicked() {
                            request_close = true;
                        }
                    });
                });
                ui.separator();
                match state {
                    ImagePreviewState::Loading { .. } => {
                        ui.add_space(40.0);
                        ui.vertical_centered(|ui| {
                            ui.spinner();
                            ui.label(self.locale.tr("IMAGE_PREVIEW_LOADING"));
                        });
                        ui.add_space(40.0);
                    }
                    ImagePreviewState::Ready { texture, .. } => {
                        if let Some(tex) = texture {
                            let max_w = (screen.width() * 0.82).max(200.0);
                            let max_h = (screen.height() * 0.72).max(160.0);
                            let tex_size = tex.size_vec2();
                            let scale = (max_w / tex_size.x).min(max_h / tex_size.y).min(1.0);
                            let show_size = tex_size * scale;
                            ui.vertical_centered(|ui| {
                                ui.add(
                                    egui::Image::new(egui::load::SizedTexture::new(
                                        tex.id(),
                                        show_size,
                                    ))
                                    .fit_to_exact_size(show_size)
                                    .tint(egui::Color32::WHITE),
                                );
                            });
                        }
                    }
                    ImagePreviewState::Error { message, .. } => {
                        let msg = message.clone();
                        ui.add_space(24.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new(msg)
                                    .size(13.0)
                                    .color(crate::theme::STATUS_ERROR),
                            );
                        });
                        ui.add_space(24.0);
                    }
                }
            });

        if !open || request_close {
            self.image_preview = None;
        }
    }

    fn finish_text_preview_bytes(
        &mut self,
        name: String,
        path: String,
        session_id: u32,
        data: Vec<u8>,
    ) {
        if data.len() > MAX_FILE_PREVIEW_BYTES {
            self.text_preview = Some(TextPreviewState::Error {
                name,
                message: self.locale.tr("TEXT_PREVIEW_TOO_LARGE"),
            });
            return;
        }
        match String::from_utf8(data) {
            Ok(content) => {
                self.text_preview = Some(TextPreviewState::Ready {
                    name,
                    path,
                    session_id,
                    saved_snapshot: content.clone(),
                    content,
                    saving: false,
                    status_line: String::new(),
                    status_error: false,
                });
            }
            Err(e) => {
                eprintln!("[mywebdisk-desktop][app] text decode not utf-8: {e}");
                self.text_preview = Some(TextPreviewState::Error {
                    name,
                    message: self.locale.tr("TEXT_PREVIEW_NOT_UTF8"),
                });
            }
        }
    }

    fn text_preview_dirty(&self) -> bool {
        match self.text_preview.as_ref() {
            Some(TextPreviewState::Ready {
                content,
                saved_snapshot,
                ..
            }) => content != saved_snapshot,
            _ => false,
        }
    }

    fn try_close_text_preview(&mut self, force: bool) {
        let can_close = match self.text_preview.as_ref() {
            Some(TextPreviewState::Ready { saving, .. }) => {
                if *saving {
                    return;
                }
                force || !self.text_preview_dirty()
            }
            Some(TextPreviewState::Loading { .. }) => false,
            _ => true,
        };
        if can_close {
            self.text_preview = None;
        } else if let Some(TextPreviewState::Ready {
            status_line,
            status_error,
            ..
        }) = self.text_preview.as_mut()
        {
            *status_line = self.locale.tr("TEXT_PREVIEW_UNSAVED");
            *status_error = true;
        }
    }

    fn save_text_preview(&mut self) {
        let (session_id, path, content) = match self.text_preview.as_ref() {
            Some(TextPreviewState::Ready {
                session_id,
                path,
                content,
                saving: false,
                ..
            }) => (*session_id, path.clone(), content.clone()),
            _ => return,
        };
        if content.len() > MAX_FILE_PREVIEW_BYTES {
            if let Some(TextPreviewState::Ready {
                status_line,
                status_error,
                ..
            }) = self.text_preview.as_mut()
            {
                *status_line = self.locale.tr("TEXT_PREVIEW_TOO_LARGE");
                *status_error = true;
            }
            return;
        }
        if let Some(TextPreviewState::Ready {
            saving,
            status_line,
            status_error,
            ..
        }) = self.text_preview.as_mut()
        {
            *saving = true;
            *status_line = self.locale.tr("TEXT_PREVIEW_SAVING");
            *status_error = false;
        }
        self.request_write(session_id, &path, &content);
    }

    fn request_write(&mut self, session_id: u32, path: &str, content: &str) {
        let req_id = format!("wr-{:016x}", rand::random::<u64>());
        let body = json!({
            "id": req_id,
            "op": "write",
            "path": path,
            "content": content,
        })
        .to_string();
        eprintln!(
            "[mywebdisk-desktop][app] request_write session_id={} req_id={} path='{}' bytes={}",
            session_id,
            req_id,
            path,
            content.len()
        );
        self.pending_write_requests.insert(
            req_id.clone(),
            PendingWrite {
                session_id,
                path: path.to_string(),
                purpose: WritePurpose::SaveText,
            },
        );
        let ipc_holder = Arc::clone(&self.ipc);
        let req_id_log = req_id.clone();
        thread::spawn(move || {
            let send_result = match ipc_holder.lock() {
                Ok(guard) => {
                    if let Some(ipc) = guard.as_ref() {
                        ipc.send_data_with_timeout(
                            session_id,
                            body.as_bytes(),
                            crate::ipc::SEND_DATA_WRITE_TIMEOUT_MS,
                        )
                    } else {
                        Err("webrpc client is not ready".into())
                    }
                }
                Err(_) => Err("webrpc client lock failed".into()),
            };
            if let Err(e) = send_result {
                eprintln!(
                    "[mywebdisk-desktop][app] request_write send failed req_id={req_id_log} err={e}"
                );
            }
        });
    }

    fn request_create_file(&mut self, session_id: u32, current_path: &str, file_name: &str) {
        let api_path = Self::join_api_path(current_path, file_name);
        let req_id = format!("wr-{:016x}", rand::random::<u64>());
        let body = json!({
            "id": req_id,
            "op": "write",
            "path": api_path,
            "content": "",
        })
        .to_string();
        eprintln!(
            "[mywebdisk-desktop][app] request_create_file session_id={} req_id={} name='{}' path='{}'",
            session_id, req_id, file_name, current_path
        );
        self.pending_write_requests.insert(
            req_id.clone(),
            PendingWrite {
                session_id,
                path: current_path.to_string(),
                purpose: WritePurpose::CreateFile {
                    current_path: current_path.to_string(),
                    display_name: file_name.to_string(),
                },
            },
        );
        let ipc_holder = Arc::clone(&self.ipc);
        let req_id_log = req_id.clone();
        thread::spawn(move || {
            let send_result = match ipc_holder.lock() {
                Ok(guard) => {
                    if let Some(ipc) = guard.as_ref() {
                        ipc.send_data_with_timeout(
                            session_id,
                            body.as_bytes(),
                            crate::ipc::SEND_DATA_WRITE_TIMEOUT_MS,
                        )
                    } else {
                        Err("webrpc client is not ready".into())
                    }
                }
                Err(_) => Err("webrpc client lock failed".into()),
            };
            if let Err(e) = send_result {
                eprintln!(
                    "[mywebdisk-desktop][app] request_create_file send failed req_id={req_id_log} err={e}"
                );
            }
        });
    }

    fn handle_upload_response(&mut self, resp: &RpcResponse) {
        let Some(wait) = self.upload_waits.get_mut(&resp.id) else {
            return;
        };
        if !wait.prepare_done {
            wait.prepare_done = true;
            wait.prepare_ok = resp.ok;
            if !resp.ok {
                wait.error = resp
                    .error
                    .clone()
                    .unwrap_or_else(|| "upload prepare failed".into());
            }
            return;
        }
        wait.complete_done = true;
        wait.complete_ok = resp.ok;
        if !resp.ok {
            wait.error = resp
                .error
                .clone()
                .unwrap_or_else(|| "upload failed".into());
        }
    }

    fn handle_write_response(&mut self, session_id: u32, resp: &RpcResponse) {
        let Some(pending) = self.pending_write_requests.remove(&resp.id) else {
            return;
        };
        if pending.session_id != session_id {
            return;
        }
        match pending.purpose {
            WritePurpose::CreateFile {
                current_path,
                display_name,
            } => {
                if !resp.ok {
                    let detail = resp.error.clone().unwrap_or_default();
                    let msg = if detail.is_empty() {
                        self.locale
                            .tr("CREATE_FILE_FAILED")
                            .replace("%s", &display_name)
                    } else {
                        self.locale.tr("CREATE_FILE_FAILED").replace("%s", &detail)
                    };
                    if let Some(session) = self.session_mut_by_id(session_id) {
                        session.last_error = msg;
                    }
                    return;
                }
                let msg = self
                    .locale
                    .tr("CREATE_FILE_OK")
                    .replace("%s", &display_name);
                if let Some(session) = self.session_mut_by_id(session_id) {
                    session.last_error = msg;
                }
                self.request_list(session_id, &current_path);
                return;
            }
            WritePurpose::SaveText => {}
        }
        let Some(TextPreviewState::Ready {
            path,
            content,
            saved_snapshot,
            saving,
            status_line,
            status_error,
            ..
        }) = self.text_preview.as_mut()
        else {
            return;
        };
        if path.as_str() != pending.path.as_str() {
            return;
        }
        *saving = false;
        if !resp.ok {
            *status_line = resp
                .error
                .clone()
                .unwrap_or_else(|| self.locale.tr("TEXT_PREVIEW_SAVE_FAILED"));
            *status_error = true;
            return;
        }
        *saved_snapshot = content.clone();
        *status_line = self.locale.tr("TEXT_PREVIEW_SAVED");
        *status_error = false;
        let new_size = content.len() as i64;
        if let Some(session) = self.session_mut_by_id(session_id) {
            if let Some(entry) = session.resources.iter_mut().find(|e| e.path == pending.path) {
                entry.size = new_size;
            }
        }
        eprintln!(
            "[mywebdisk-desktop][app] write ok id={} path='{}' bytes={}",
            resp.id, pending.path, new_size
        );
    }

    fn draw_text_preview(&mut self, ctx: &egui::Context) {
        if self.text_preview.is_none() {
            return;
        }

        let save_shortcut = ctx.input(|i| {
            i.key_pressed(egui::Key::S) && (i.modifiers.command || i.modifiers.ctrl)
        });
        if save_shortcut {
            self.save_text_preview();
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.try_close_text_preview(false);
            if self.text_preview.is_none() {
                return;
            }
        }

        let screen = ctx.screen_rect();
        let mut request_close = false;
        let mut request_discard = false;
        let mut request_save = false;

        let backdrop_layer =
            egui::LayerId::new(egui::Order::Middle, egui::Id::new("text_preview_backdrop"));
        ctx.layer_painter(backdrop_layer)
            .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(120));

        let title = self
            .text_preview
            .as_ref()
            .map(|s| match s {
                TextPreviewState::Loading { name } => name.clone(),
                TextPreviewState::Ready { name, .. } => name.clone(),
                TextPreviewState::Error { name, .. } => name.clone(),
            })
            .unwrap_or_default();
        let close_tip = self.locale.tr("TEXT_PREVIEW_CLOSE");
        let save_label = self.locale.tr("TEXT_PREVIEW_SAVE");
        let discard_label = self.locale.tr("TEXT_PREVIEW_DISCARD");
        let dirty = self.text_preview_dirty();
        let saving_now = matches!(
            self.text_preview.as_ref(),
            Some(TextPreviewState::Ready { saving: true, .. })
        );
        let mut open = true;
        let win_w = (screen.width() * 0.88).clamp(420.0, 680.0);
        let win_h = (screen.height() * 0.78).clamp(280.0, 520.0);

        egui::Window::new("text_preview")
            .open(&mut open)
            .order(egui::Order::Foreground)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .default_size(egui::vec2(win_w, win_h))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(crate::theme::MAC_PANEL)
                    .stroke(egui::Stroke::new(1.0, crate::theme::MAC_BORDER))
                    .inner_margin(egui::Margin::symmetric(12, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(&title);
                    if dirty {
                        ui.label(
                            egui::RichText::new("●")
                                .size(12.0)
                                .color(crate::theme::ORANGE),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::theme::mac_toolbar_close_button(ui, &close_tip, !saving_now)
                            .clicked()
                        {
                            request_close = true;
                        }
                        if dirty
                            && ui
                                .add_enabled(!saving_now, egui::Button::new(&discard_label))
                                .clicked()
                        {
                            request_discard = true;
                        }
                        if crate::theme::primary_button(ui, &save_label, !saving_now).clicked() {
                            request_save = true;
                        }
                    });
                });
                if let Some(TextPreviewState::Ready {
                    status_line,
                    status_error,
                    ..
                }) = self.text_preview.as_ref()
                {
                    if !status_line.is_empty() {
                        let color = if *status_error {
                            crate::theme::STATUS_ERROR
                        } else {
                            crate::theme::STATUS_SUCCESS
                        };
                        ui.label(egui::RichText::new(status_line).size(11.0).color(color));
                    }
                }
                ui.separator();
                match self.text_preview.as_mut() {
                    Some(TextPreviewState::Loading { .. }) => {
                        ui.add_space(40.0);
                        ui.vertical_centered(|ui| {
                            ui.spinner();
                            ui.label(self.locale.tr("TEXT_PREVIEW_LOADING"));
                        });
                        ui.add_space(40.0);
                    }
                    Some(TextPreviewState::Ready { content, saving, .. }) => {
                        let saving_h = if *saving { 26.0 } else { 0.0 };
                        let edit_h = (ui.available_height() - saving_h).max(120.0);
                        let edit_w = ui.available_width().max(200.0);
                        let edit_rect =
                            egui::Rect::from_min_size(ui.cursor().min, egui::vec2(edit_w, edit_h));
                        ui.add_enabled_ui(!*saving, |ui| {
                            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(edit_rect), |ui| {
                                egui::Frame::new()
                                    .fill(egui::Color32::from_rgb(252, 252, 254))
                                    .stroke(egui::Stroke::new(
                                        1.0,
                                        egui::Color32::from_rgb(72, 72, 76),
                                    ))
                                    .corner_radius(egui::CornerRadius::same(4))
                                    .inner_margin(egui::Margin::symmetric(4, 4))
                                    .show(ui, |ui| {
                                        let inner_h = ui.available_height().max(80.0);
                                        let inner_w = ui.available_width().max(100.0);
                                        egui::ScrollArea::both()
                                            .auto_shrink([false, false])
                                            .min_scrolled_height(inner_h)
                                            .show(ui, |ui| {
                                                ui.set_min_size(egui::vec2(inner_w, inner_h));
                                                ui.add(
                                                    egui::TextEdit::multiline(content)
                                                        .font(egui::TextStyle::Monospace)
                                                        .code_editor()
                                                        .desired_width(inner_w)
                                                        .min_size(egui::vec2(inner_w, inner_h)),
                                                );
                                            });
                                    });
                            });
                        });
                        if *saving {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(self.locale.tr("TEXT_PREVIEW_SAVING"));
                            });
                        }
                    }
                    Some(TextPreviewState::Error { message, .. }) => {
                        let msg = message.clone();
                        ui.add_space(24.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new(msg)
                                    .size(13.0)
                                    .color(crate::theme::STATUS_ERROR),
                            );
                        });
                        ui.add_space(24.0);
                    }
                    None => {}
                }
            });

        if request_save {
            self.save_text_preview();
        }
        if request_discard {
            self.text_preview = None;
        } else if request_close || !open {
            self.try_close_text_preview(false);
        }
    }

    fn close_disk_session(&mut self, session_id: u32) {
        eprintln!("[mywebdisk-desktop][app] close_disk_session session_id={session_id}");
        self.sync_office_edits_for_session(session_id);
        if let Ok(guard) = self.ipc.lock() {
            if let Some(ipc) = guard.as_ref() {
                if let Err(e) = ipc.close_session(session_id) {
                    eprintln!(
                        "[mywebdisk-desktop][app] close_disk_session ipc err session_id={session_id} err={e}"
                    );
                }
            }
        }
        self.pending_list_requests
            .retain(|_, sid| *sid != session_id);
        self.pending_download_requests
            .retain(|_, p| p.session_id != session_id);
        self.pending_delete_requests
            .retain(|_, p| p.session_id != session_id);
        self.awaiting_file_downloads.remove(&session_id);
        self.pending_write_requests
            .retain(|_, p| p.session_id != session_id);
        self.pending_mkdir_requests
            .retain(|_, p| p.session_id != session_id);
        self.pending_stream_downloads
            .retain(|_, p| p.session_id != session_id);
        self.upload_waits.clear();
        if self
            .finder_upload
            .as_ref()
            .is_some_and(|u| u.session_id == session_id)
        {
            self.finder_upload = None;
            self.upload_send_rx = None;
        }
        if self
            .finder_blank_menu
            .as_ref()
            .is_some_and(|m| m.session_id == session_id)
        {
            self.finder_blank_menu = None;
        }
        self.cancel_session_media_streams(session_id);
        let target_token = self
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .map(|s| s.target_token.clone());
        if let Some(token) = target_token {
            crate::session_workspace::clean_session_temp(&token);
        }
        if self.text_preview.as_ref().is_some_and(|p| {
            matches!(
                p,
                TextPreviewState::Ready { session_id: sid, .. } if *sid == session_id
            )
        }) {
            self.text_preview = None;
        }
        if self
            .image_preview
            .as_ref()
            .is_some_and(|p| matches!(p, ImagePreviewState::Loading { .. }))
        {
            self.image_preview = None;
        }
        if self
            .text_preview
            .as_ref()
            .is_some_and(|p| matches!(p, TextPreviewState::Loading { .. }))
        {
            self.text_preview = None;
        }
        let Some(idx) = self.sessions.iter().position(|s| s.id == session_id) else {
            return;
        };
        self.sessions.remove(idx);
        if let Some(sel) = self.selected_session {
            if sel == idx {
                self.selected_session = if self.sessions.is_empty() {
                    None
                } else {
                    Some(sel.min(self.sessions.len() - 1))
                };
            } else if sel > idx {
                self.selected_session = Some(sel - 1);
            }
        }
    }

    fn session_by_id(&self, session_id: u32) -> Option<&SessionEntry> {
        self.sessions.iter().find(|s| s.id == session_id)
    }

    fn session_mut_by_id(&mut self, session_id: u32) -> Option<&mut SessionEntry> {
        self.sessions.iter_mut().find(|s| s.id == session_id)
    }

    fn poll_callback_events(&mut self) {
        while let Ok(event) = self.callback_rx.try_recv() {
            match event {
                CallbackEvent::Connected => {
                    self.callback_connected = true;
                    eprintln!("[mywebdisk-desktop][app] callback state connected=true");
                }
                CallbackEvent::Data { session_id, payload } => {
                    self.handle_callback_data(session_id, &payload);
                }
                CallbackEvent::File {
                    session_id,
                    file_name,
                    data,
                } => {
                    self.handle_callback_file(session_id, &file_name, &data);
                }
                CallbackEvent::Error(msg) => {
                    self.callback_connected = false;
                    self.connect_status = msg;
                    self.connect_status_is_error = true;
                }
            }
        }
    }

    fn handle_callback_data(&mut self, session_id: u32, payload: &[u8]) {
        if crate::stream_protocol::is_stream_frame(payload) {
            self.handle_stream_frame(session_id, payload);
            return;
        }
        eprintln!(
            "[mywebdisk-desktop][app] handle_callback_data session_id={} json_len={}",
            session_id,
            payload.len()
        );
        let Ok(resp) = serde_json::from_slice::<RpcResponse>(payload) else {
            eprintln!("[mywebdisk-desktop][app] callback json parse failed");
            return;
        };
        if self.pending_stream_downloads.contains_key(&resp.id) {
            self.handle_stream_download_response(session_id, &resp);
            return;
        }
        if self.pending_write_requests.contains_key(&resp.id) {
            self.handle_write_response(session_id, &resp);
            return;
        }
        if self.upload_waits.contains_key(&resp.id) {
            self.handle_upload_response(&resp);
            return;
        }
        if self
            .awaiting_file_downloads
            .get(&session_id)
            .is_some_and(|i| {
                i.req_id == resp.id
                    && resp
                        .data
                        .as_ref()
                        .and_then(|d| d.get("fileSent"))
                        .is_some()
            })
        {
            self.handle_file_sent_notification(session_id, &resp);
            return;
        }
        if self.pending_download_requests.contains_key(&resp.id) {
            self.handle_download_response(session_id, &resp);
            return;
        }
        if self.pending_delete_requests.contains_key(&resp.id) {
            self.handle_delete_response(session_id, &resp);
            return;
        }
        if self.pending_mkdir_requests.contains_key(&resp.id) {
            self.handle_mkdir_response(session_id, &resp);
            return;
        }

        if !self.pending_list_requests.contains_key(&resp.id) {
            eprintln!(
                "[mywebdisk-desktop][app] callback id={} not in pending list",
                resp.id
            );
            return;
        }
        let _ = self.pending_list_requests.remove(&resp.id);

        if !resp.ok {
            eprintln!(
                "[mywebdisk-desktop][app] callback resp not ok id={} error={}",
                resp.id,
                resp.error.clone().unwrap_or_default()
            );
            if let Some(session) = self.session_mut_by_id(session_id) {
                session.last_error = resp.error.unwrap_or_else(|| "list failed".into());
            }
            return;
        }
        let Some(data_val) = resp.data else {
            return;
        };
        let Ok(list_data) = serde_json::from_value::<RpcListData>(data_val) else {
            eprintln!("[mywebdisk-desktop][app] callback list data parse failed id={}", resp.id);
            if let Some(session) = self.session_mut_by_id(session_id) {
                session.last_error = "invalid list response payload".into();
            }
            return;
        };
        eprintln!(
            "[mywebdisk-desktop][app] callback list ok id={} path='{}' items={}",
            resp.id,
            list_data.path,
            list_data.items.len()
        );
        if let Some(session) = self.session_mut_by_id(session_id) {
            session.current_path = list_data.path;
            session.last_error.clear();
            let mut resources: Vec<ResourceEntry> = list_data
                .items
                .into_iter()
                .map(|item| ResourceEntry {
                    name: item.name,
                    path: item.path,
                    is_dir: item.is_dir,
                    size: item.size,
                })
                .collect();
            resources.sort_by(|a, b| {
                b.is_dir
                    .cmp(&a.is_dir)
                    .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
            });
            session.resources = resources;
        }
    }

    fn draw_login(&mut self, ui: &mut egui::Ui) {
        let status_text = self.status_text();
        let logging_in = self.logging_in;
        let can_login = self.can_login();
        let locale = &self.locale;
        let token = &mut self.token;
        let passwd = &mut self.passwd;
        let status_kind = self.status_kind;

        match login_page::draw(
            ui,
            LoginView {
                locale,
                token,
                passwd,
                status_text: &status_text,
                status_kind,
                logging_in,
                can_login,
            },
        ) {
            LoginAction::None => {}
            LoginAction::Login => self.start_login(),
            LoginAction::OpenWebrpc => {
                let _ = webbrowser::open("https://webrpc.cn/en/");
            }
            LoginAction::SetLang(lang) => {
                self.locale.lang = lang;
            }
        }
    }

    fn draw_sessions(&mut self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        let top_h = 48.0;
        let gap = 10.0;
        let left_w = (full.width() * 0.32).clamp(120.0, 200.0);
        let top_rect = egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, full.min.y + top_h));
        let body_rect = egui::Rect::from_min_max(egui::pos2(full.min.x, top_rect.max.y + 6.0), full.max);
        let left_rect = egui::Rect::from_min_max(body_rect.min, egui::pos2(body_rect.min.x + left_w, body_rect.max.y));
        let right_rect = egui::Rect::from_min_max(egui::pos2(left_rect.max.x + gap, body_rect.min.y), body_rect.max);

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(top_rect), |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(self.locale.tr("SESSIONS_TITLE")).size(18.0));
                ui.add_space(16.0);
                if crate::theme::primary_button(
                    ui,
                    &self.locale.tr("BTN_CONNECT_DISK"),
                    !self.connecting_session,
                )
                .clicked()
                    && !self.connecting_session
                {
                    self.show_connect_modal = true;
                    self.connect_status.clear();
                    self.connect_status_is_error = false;
                }
            });
        });

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(left_rect), |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(12, 20, 30))
                .stroke(egui::Stroke::new(1.0, crate::theme::CYAN_DIM))
                .inner_margin(egui::Margin::symmetric(8, 8))
                .show(ui, |ui| {
                    ui.label(self.locale.tr("SESSION_LIST_TITLE"));
                    ui.separator();
                    if self.sessions.is_empty() {
                        ui.label(self.locale.tr("SESSION_EMPTY"));
                    } else {
                        let mut clicked_idx: Option<usize> = None;
                        for (idx, session) in self.sessions.iter().enumerate() {
                            let label = format!("{} #{}", session.target_token, session.id);
                            if ui
                                .selectable_label(self.selected_session == Some(idx), label)
                                .clicked()
                            {
                                clicked_idx = Some(idx);
                            }
                        }
                        if let Some(idx) = clicked_idx {
                            self.selected_session = Some(idx);
                            if let Some(session) = self.sessions.get(idx) {
                                self.request_list(session.id, "");
                            }
                        }
                    }
                });
        });

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(right_rect), |ui| {
            self.draw_resource_finder(ui);
        });
    }

    fn draw_connect_modal(&mut self, ctx: &egui::Context) {
        if !self.show_connect_modal {
            return;
        }

        let mut open = self.show_connect_modal;
        egui::Window::new(self.locale.tr("CONNECT_MODAL_TITLE"))
            .collapsible(false)
            .resizable(false)
            .fixed_size(egui::vec2(320.0, 210.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(self.locale.tr("CONNECT_MODAL_TARGET_TOKEN"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.connect_target_token)
                        .hint_text(self.locale.tr("CONNECT_MODAL_TARGET_TOKEN_PLACEHOLDER"))
                        .interactive(!self.connecting_session)
                        .desired_width(f32::INFINITY),
                );
                ui.label(self.locale.tr("CONNECT_MODAL_PASSWD"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.connect_passwd)
                        .password(true)
                        .hint_text(self.locale.tr("CONNECT_MODAL_PASSWD_PLACEHOLDER"))
                        .interactive(!self.connecting_session)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
                if !self.connect_status.is_empty() {
                    let color = if self.connect_status_is_error {
                        crate::theme::STATUS_ERROR
                    } else {
                        crate::theme::CYAN_SOFT
                    };
                    ui.label(egui::RichText::new(&self.connect_status).color(color));
                } else {
                    ui.add_space(20.0);
                }
                ui.horizontal(|ui| {
                    let can_connect = self.can_connect_session();
                    let connect_label = if self.connecting_session {
                        self.locale.tr("CONNECT_MODAL_CONNECTING")
                    } else {
                        self.locale.tr("CONNECT_MODAL_CONNECT")
                    };
                    if crate::theme::primary_button(ui, &connect_label, can_connect).clicked() && can_connect {
                        self.start_open_session();
                    }
                    if ui.button(self.locale.tr("CONNECT_MODAL_CANCEL")).clicked() && !self.connecting_session {
                        self.show_connect_modal = false;
                        self.connect_status.clear();
                        self.connect_status_is_error = false;
                    }
                });
            });
        self.show_connect_modal = open;
    }

    fn draw_title_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let title_h = 22.0;
        let (rect, drag_resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), title_h),
            egui::Sense::click_and_drag(),
        );
        if drag_resp.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }

        let mut title_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        title_ui.spacing_mut().item_spacing.x = 6.0;
        let glyph_color = title_ui.style().visuals.text_color();

        let circle_size = egui::vec2(12.0, 12.0);
        let (min_rect, min_resp) = title_ui.allocate_exact_size(circle_size, egui::Sense::click());
        title_ui.painter().circle_filled(
            min_rect.center(),
            min_rect.width() * 0.5,
            egui::Color32::from_rgb(255, 189, 46),
        );
        title_ui.painter().circle_stroke(
            min_rect.center(),
            min_rect.width() * 0.5 - 0.5,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(210, 150, 35)),
        );
        if min_resp.hovered() {
            let p = title_ui.painter();
            let c = min_rect.center();
            p.line_segment(
                [
                    egui::pos2(c.x - 3.0, c.y + 0.5),
                    egui::pos2(c.x + 3.0, c.y + 0.5),
                ],
                egui::Stroke::new(1.0, glyph_color),
            );
        }
        if min_resp.clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        }

        let (close_rect, close_resp) = title_ui.allocate_exact_size(circle_size, egui::Sense::click());
        title_ui.painter().circle_filled(
            close_rect.center(),
            close_rect.width() * 0.5,
            egui::Color32::from_rgb(255, 95, 87),
        );
        title_ui.painter().circle_stroke(
            close_rect.center(),
            close_rect.width() * 0.5 - 0.5,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(210, 80, 74)),
        );
        if close_resp.hovered() {
            let p = title_ui.painter();
            let c = close_rect.center();
            p.line_segment(
                [
                    egui::pos2(c.x - 2.5, c.y - 2.5),
                    egui::pos2(c.x + 2.5, c.y + 2.5),
                ],
                egui::Stroke::new(1.0, glyph_color),
            );
            p.line_segment(
                [
                    egui::pos2(c.x + 2.5, c.y - 2.5),
                    egui::pos2(c.x - 2.5, c.y + 2.5),
                ],
                egui::Stroke::new(1.0, glyph_color),
            );
        }
        if close_resp.clicked() {
            self.cleanup_ipc();
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
}

impl Drop for MyWebDiskApp {
    fn drop(&mut self) {
        self.cleanup_ipc();
    }
}

impl eframe::App for MyWebDiskApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_login();
        self.poll_open_session();
        self.poll_callback_events();
        self.poll_finder_upload(ctx);

        let (win_w, win_h) = match self.screen {
            Screen::Login => (WINDOW_W, WINDOW_H),
            Screen::Sessions => (SESSIONS_WINDOW_W, SESSIONS_WINDOW_H),
        };
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(win_w, win_h)));
        ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(egui::vec2(win_w, win_h)));
        ctx.send_viewport_cmd(ViewportCommand::MaxInnerSize(egui::vec2(win_w, win_h)));

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::BG))
            .show(ctx, |ui| {
                if !self.file_preview_open() {
                    self.draw_title_bar(ui, ctx);
                }
                ui.add_space(6.0);
                ui.set_width(win_w - 28.0);
                ui.vertical_centered(|ui| {
                    match self.screen {
                        Screen::Login => self.draw_login(ui),
                        Screen::Sessions => self.draw_sessions(ui),
                    }
                });
            });

        self.draw_connect_modal(ctx);
        self.draw_finder_upload_ui(ctx);
        self.draw_finder_blank_menu(ctx);
        self.draw_new_entry_modal(ctx);
        self.draw_image_preview(ctx);
        self.draw_text_preview(ctx);

        if self.file_preview_open() {
            let bar_w = win_w - 28.0;
            egui::Area::new(egui::Id::new("title_bar_above_preview"))
                .order(egui::Order::Foreground)
                .fixed_pos(ctx.screen_rect().min)
                .show(ctx, |ui| {
                    ui.set_width(bar_w);
                    self.draw_title_bar(ui, ctx);
                });
        }

        if self.logging_in
            || self.connecting_session
            || self.screen == Screen::Sessions
            || self.file_preview_open()
            || !self.media_streams.is_empty()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}
