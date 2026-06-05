//! 彩色文件类型图标（egui Painter 绘制，风格接近 macOS / Windows 资源管理器）

use egui::{Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, Vec2};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileIconKind {
    Folder,
    Text,
    Word,
    Excel,
    PowerPoint,
    Pdf,
    Archive,
    Image,
    Video,
    Audio,
    Executable,
    Generic,
}

fn file_extension(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let ext = base.rsplit('.').next()?;
    if ext.eq_ignore_ascii_case(base) {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// 可预览/编辑的纯文本与源代码扩展名（小写）。
const TEXT_EXTS: &[&str] = &[
    "adoc",
    "asciidoc",
    "asm",
    "astro",
    "awk",
    "bash",
    "bat",
    "bib",
    "bzl",
    "c",
    "cc",
    "cfg",
    "cjs",
    "clj",
    "cljs",
    "cmake",
    "cmd",
    "cnf",
    "coffee",
    "conf",
    "cpp",
    "cr",
    "crt",
    "cs",
    "csx",
    "css",
    "csv",
    "cxx",
    "d",
    "dart",
    "diff",
    "dockerfile",
    "dpr",
    "edn",
    "ejs",
    "elm",
    "env",
    "erb",
    "erl",
    "ex",
    "exs",
    "f",
    "f90",
    "f95",
    "fish",
    "for",
    "frag",
    "fs",
    "fsx",
    "fpp",
    "gcode",
    "glsl",
    "go",
    "gql",
    "gradle",
    "graphql",
    "groovy",
    "h",
    "haml",
    "handlebars",
    "hbs",
    "hh",
    "hcl",
    "hlsl",
    "hpp",
    "hrl",
    "hs",
    "htm",
    "html",
    "hxx",
    "inc",
    "ini",
    "ino",
    "inl",
    "ipp",
    "ipynb",
    "jl",
    "j2",
    "java",
    "jinja",
    "js",
    "json",
    "json5",
    "jsonc",
    "jsx",
    "jade",
    "kts",
    "kt",
    "less",
    "lhs",
    "lock",
    "log",
    "litcoffee",
    "liquid",
    "lpr",
    "ltx",
    "lua",
    "m",
    "mak",
    "markdown",
    "md",
    "mdown",
    "mdx",
    "meson",
    "mjs",
    "mkd",
    "mm",
    "mod",
    "mount",
    "mustache",
    "nim",
    "nix",
    "njk",
    "org",
    "pas",
    "patch",
    "path",
    "pem",
    "pgp",
    "php",
    "phtml",
    "pl",
    "plist",
    "pm",
    "pod",
    "properties",
    "proto",
    "ps1",
    "psm1",
    "psql",
    "pub",
    "pug",
    "purs",
    "py",
    "pyi",
    "pyw",
    "pyx",
    "r",
    "rb",
    "rej",
    "rs",
    "rst",
    "rtf",
    "s",
    "sass",
    "scala",
    "sc",
    "scss",
    "sed",
    "service",
    "sh",
    "sig",
    "slim",
    "socket",
    "sql",
    "sty",
    "styl",
    "svelte",
    "sum",
    "sv",
    "swift",
    "t",
    "tex",
    "tf",
    "tfvars",
    "thrift",
    "timer",
    "toml",
    "tpl",
    "ts",
    "tsv",
    "tsx",
    "twig",
    "txt",
    "v",
    "vb",
    "vbx",
    "vert",
    "vh",
    "vsh",
    "vue",
    "wgsl",
    "xml",
    "xhtml",
    "yaml",
    "yml",
    "zig",
    "zsh",
    "ninja",
    "bazel",
];

fn is_text_extension(ext: &str) -> bool {
    TEXT_EXTS.contains(&ext)
}

/// 图标上显示的标签（大写扩展名；过长时用常见缩写）。
fn icon_label_for_file(name: &str) -> String {
    let Some(ext) = file_extension(name) else {
        return "TXT".to_string();
    };
    let label = match ext.as_str() {
        "markdown" | "mdown" | "mkd" => "MD",
        "asciidoc" => "ADOC",
        "dockerfile" => "DOCKER",
        "handlebars" => "HBS",
        "properties" => "PROP",
        "typescript" => "TS",
        _ => ext.as_str(),
    };
    label.to_ascii_uppercase()
}

pub fn is_image(name: &str, is_dir: bool) -> bool {
    !is_dir && kind_for(name, false) == FileIconKind::Image
}

pub fn is_video(name: &str, is_dir: bool) -> bool {
    !is_dir && kind_for(name, false) == FileIconKind::Video
}

pub fn is_audio(name: &str, is_dir: bool) -> bool {
    !is_dir && kind_for(name, false) == FileIconKind::Audio
}

pub fn is_office(name: &str, is_dir: bool) -> bool {
    !is_dir && matches!(
        kind_for(name, false),
        FileIconKind::Word | FileIconKind::Excel | FileIconKind::PowerPoint | FileIconKind::Pdf
    )
}

pub fn is_text(name: &str, is_dir: bool) -> bool {
    !is_dir && kind_for(name, false) == FileIconKind::Text
}

pub fn kind_for(name: &str, is_dir: bool) -> FileIconKind {
    if is_dir {
        return FileIconKind::Folder;
    }
    let Some(ext) = file_extension(name) else {
        return FileIconKind::Generic;
    };
    if is_text_extension(ext.as_str()) {
        return FileIconKind::Text;
    }

    match ext.as_str() {
        "doc" | "docx" | "docm" | "dot" | "dotx" | "odt" | "pages" | "wps" => FileIconKind::Word,

        "xls" | "xlsx" | "xlsm" | "xlsb" | "ods" | "numbers" | "et" => {
            FileIconKind::Excel
        }

        "ppt" | "pptx" | "pptm" | "pps" | "ppsx" | "odp" | "key" | "dps" => {
            FileIconKind::PowerPoint
        }

        "pdf" => FileIconKind::Pdf,

        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "lz" | "lzma"
        | "zst" | "jar" | "war" | "apk" | "ipa" | "deb" | "rpm" | "dmg" | "iso" => {
            FileIconKind::Archive
        }

        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "ico" | "heic"
        | "heif" | "tif" | "tiff" | "avif" | "raw" | "cr2" | "nef" | "psd" => {
            FileIconKind::Image
        }

        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "mpeg"
        | "mpg" | "3gp" | "m2ts" | "mts" | "vob" | "ogv" | "rm" | "rmvb" => {
            FileIconKind::Video
        }

        "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" | "oga" | "opus" | "wma"
        | "aiff" | "aif" | "mid" | "midi" => FileIconKind::Audio,

        "exe" | "msi" | "app" | "dll" | "so" | "dylib" | "bin" => FileIconKind::Executable,

        _ => FileIconKind::Generic,
    }
}

pub fn draw(painter: &Painter, rect: Rect, name: &str, is_dir: bool) {
    let kind = kind_for(name, is_dir);
    match kind {
        FileIconKind::Folder => draw_folder(painter, rect),
        FileIconKind::Text => draw_text_doc(painter, rect, &icon_label_for_file(name)),
        FileIconKind::Word => draw_office_doc(painter, rect, Color32::from_rgb(43, 87, 154), 'W'),
        FileIconKind::Excel => draw_excel(painter, rect),
        FileIconKind::PowerPoint => draw_office_doc(painter, rect, Color32::from_rgb(210, 71, 38), 'P'),
        FileIconKind::Pdf => draw_pdf(painter, rect),
        FileIconKind::Archive => draw_archive(painter, rect),
        FileIconKind::Image => draw_image(painter, rect),
        FileIconKind::Video => draw_video(painter, rect),
        FileIconKind::Audio => draw_audio(painter, rect),
        FileIconKind::Executable => draw_executable(painter, rect),
        FileIconKind::Generic => draw_generic_doc(painter, rect),
    }
}

fn scale_rect(r: Rect) -> f32 {
    r.width().max(r.height())
}

fn draw_folder(p: &Painter, r: Rect) {
    let s = scale_rect(r);
    let o = r.min;
    let tab = Rect::from_min_size(o + Vec2::new(s * 0.08, s * 0.10), Vec2::new(s * 0.44, s * 0.20));
    let body = Rect::from_min_size(o + Vec2::new(s * 0.06, s * 0.28), Vec2::new(s * 0.88, s * 0.62));
    let tab_fill = Color32::from_rgb(255, 220, 120);
    let body_fill = Color32::from_rgb(255, 196, 58);
    let outline = Color32::from_rgb(196, 142, 8);
    p.rect_filled(tab, CornerRadius::same(2), tab_fill);
    p.rect_filled(body, CornerRadius::same(3), body_fill);
    p.rect_stroke(
        body,
        CornerRadius::same(3),
        Stroke::new(0.6, outline),
        egui::StrokeKind::Outside,
    );
}

fn page_body(r: Rect, s: f32) -> Rect {
    Rect::from_min_size(r.min + Vec2::new(s * 0.14, s * 0.06), Vec2::new(s * 0.72, s * 0.88))
}

fn draw_page_fold(p: &Painter, r: Rect, fill: Color32) {
    let s = scale_rect(r);
    let body = page_body(r, s);
    p.rect_filled(body, CornerRadius::same(2), fill);
    // 折角
    let fold = [
        Pos2::new(body.max.x - s * 0.18, body.min.y),
        Pos2::new(body.max.x, body.min.y),
        Pos2::new(body.max.x, body.min.y + s * 0.18),
    ];
    p.add(Shape::convex_polygon(
        fold.to_vec(),
        Color32::from_rgb(230, 230, 230),
        Stroke::NONE,
    ));
    p.line_segment(
        [fold[0], fold[2]],
        Stroke::new(0.5, Color32::from_rgb(180, 180, 180)),
    );
}

fn ext_label_font_size(label: &str, s: f32) -> f32 {
    match label.len() {
        0..=2 => s * 0.30,
        3 => s * 0.24,
        4 => s * 0.20,
        5 => s * 0.17,
        6 => s * 0.15,
        _ => s * 0.13,
    }
}

fn draw_text_doc(p: &Painter, r: Rect, label: &str) {
    let s = scale_rect(r);
    let accent = Color32::from_rgb(66, 133, 244);
    let accent_dark = Color32::from_rgb(52, 108, 210);
    let body = page_body(r, s);
    p.rect_filled(body, CornerRadius::same(2), accent);
    let fold = [
        Pos2::new(body.max.x - s * 0.16, body.min.y),
        Pos2::new(body.max.x, body.min.y),
        Pos2::new(body.max.x, body.min.y + s * 0.16),
    ];
    p.add(Shape::convex_polygon(fold.to_vec(), accent_dark, Stroke::NONE));
    p.text(
        body.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(ext_label_font_size(label, s)),
        Color32::WHITE,
    );
}

fn draw_generic_doc(p: &Painter, r: Rect) {
    let s = scale_rect(r);
    draw_page_fold(p, r, Color32::from_rgb(245, 247, 250));
    let body = page_body(r, s);
    p.rect_filled(
        Rect::from_min_size(
            body.min + Vec2::new(s * 0.12, s * 0.38),
            Vec2::new(s * 0.48, s * 0.30),
        ),
        CornerRadius::same(1),
        Color32::from_rgb(200, 210, 225),
    );
}

fn draw_office_doc(p: &Painter, r: Rect, accent: Color32, letter: char) {
    let s = scale_rect(r);
    let body = page_body(r, s);
    p.rect_filled(body, CornerRadius::same(2), accent);
    let fold = [
        Pos2::new(body.max.x - s * 0.16, body.min.y),
        Pos2::new(body.max.x, body.min.y),
        Pos2::new(body.max.x, body.min.y + s * 0.16),
    ];
    p.add(Shape::convex_polygon(
        fold.to_vec(),
        accent.gamma_multiply(0.85),
        Stroke::NONE,
    ));
    p.text(
        body.center(),
        egui::Align2::CENTER_CENTER,
        letter.to_string(),
        egui::FontId::proportional(s * 0.52),
        Color32::WHITE,
    );
}

fn draw_excel(p: &Painter, r: Rect) {
    let s = scale_rect(r);
    let accent = Color32::from_rgb(33, 115, 70);
    let body = page_body(r, s);
    p.rect_filled(body, CornerRadius::same(2), accent);
    let fold = [
        Pos2::new(body.max.x - s * 0.16, body.min.y),
        Pos2::new(body.max.x, body.min.y),
        Pos2::new(body.max.x, body.min.y + s * 0.16),
    ];
    p.add(Shape::convex_polygon(
        fold.to_vec(),
        Color32::from_rgb(26, 92, 56),
        Stroke::NONE,
    ));
    let grid_origin = body.min + Vec2::new(s * 0.14, s * 0.34);
    let cell = s * 0.14;
    for row in 0..2 {
        for col in 0..2 {
            let c = Rect::from_min_size(
                grid_origin + Vec2::new(col as f32 * cell, row as f32 * cell),
                Vec2::splat(cell - 1.0),
            );
            p.rect_filled(c, CornerRadius::ZERO, Color32::from_rgba_premultiplied(255, 255, 255, 200));
        }
    }
}

fn draw_pdf(p: &Painter, r: Rect) {
    let s = scale_rect(r);
    let accent = Color32::from_rgb(229, 57, 53);
    let body = page_body(r, s);
    p.rect_filled(body, CornerRadius::same(2), accent);
    let fold = [
        Pos2::new(body.max.x - s * 0.16, body.min.y),
        Pos2::new(body.max.x, body.min.y),
        Pos2::new(body.max.x, body.min.y + s * 0.16),
    ];
    p.add(Shape::convex_polygon(
        fold.to_vec(),
        Color32::from_rgb(198, 40, 40),
        Stroke::NONE,
    ));
    p.text(
        body.center() + Vec2::new(0.0, s * 0.02),
        egui::Align2::CENTER_CENTER,
        "PDF",
        egui::FontId::proportional(s * 0.22),
        Color32::WHITE,
    );
}

fn draw_archive(p: &Painter, r: Rect) {
    let s = scale_rect(r);
    let green = Color32::from_rgb(76, 175, 80);
    let dark = Color32::from_rgb(46, 125, 50);
    let body = page_body(r, s);
    p.rect_filled(body, CornerRadius::same(2), green);
    let fold = [
        Pos2::new(body.max.x - s * 0.16, body.min.y),
        Pos2::new(body.max.x, body.min.y),
        Pos2::new(body.max.x, body.min.y + s * 0.16),
    ];
    p.add(Shape::convex_polygon(fold.to_vec(), dark, Stroke::NONE));
    // 拉链齿
    let cx = body.center().x;
    for i in 0..5 {
        let y = body.min.y + s * (0.28 + i as f32 * 0.11);
        p.line_segment(
            [Pos2::new(cx - s * 0.06, y), Pos2::new(cx + s * 0.06, y)],
            Stroke::new(1.2, Color32::WHITE),
        );
    }
    p.line_segment(
        [Pos2::new(cx, body.min.y + s * 0.24), Pos2::new(cx, body.max.y - s * 0.12)],
        Stroke::new(1.0, Color32::from_rgb(255, 235, 120)),
    );
}

fn draw_image(p: &Painter, r: Rect) {
    let s = scale_rect(r);
    let body = page_body(r, s);
    p.rect_filled(body, CornerRadius::same(2), Color32::from_rgb(240, 242, 245));
    let sky = Rect::from_min_max(body.min, Pos2::new(body.max.x, body.center().y + s * 0.04));
    p.rect_filled(sky, CornerRadius::same(2), Color32::from_rgb(100, 181, 246));
    p.circle_filled(
        body.min + Vec2::new(s * 0.26, s * 0.28),
        s * 0.07,
        Color32::from_rgb(255, 213, 79),
    );
    let hill = [
        Pos2::new(body.min.x, body.max.y - s * 0.08),
        Pos2::new(body.min.x + s * 0.30, body.center().y + s * 0.02),
        Pos2::new(body.max.x - s * 0.08, body.max.y - s * 0.18),
        Pos2::new(body.max.x, body.max.y - s * 0.08),
    ];
    p.add(Shape::convex_polygon(
        hill.to_vec(),
        Color32::from_rgb(102, 187, 106),
        Stroke::NONE,
    ));
}

fn draw_video(p: &Painter, r: Rect) {
    let s = scale_rect(r);
    let body = page_body(r, s);
    p.rect_filled(body, CornerRadius::same(2), Color32::from_rgb(126, 87, 194));
    let cx = body.center().x;
    let cy = body.center().y;
    let tri = [
        Pos2::new(cx - s * 0.08, cy - s * 0.12),
        Pos2::new(cx - s * 0.08, cy + s * 0.12),
        Pos2::new(cx + s * 0.14, cy),
    ];
    p.add(Shape::convex_polygon(tri.to_vec(), Color32::WHITE, Stroke::NONE));
}

fn draw_audio(p: &Painter, r: Rect) {
    let s = scale_rect(r);
    let center = r.center();
    p.circle_filled(center, s * 0.38, Color32::from_rgb(236, 64, 122));
    p.circle_stroke(
        center,
        s * 0.38,
        Stroke::new(0.5, Color32::from_rgb(194, 24, 91)),
    );
    // 音符简化
    p.circle_filled(center + Vec2::new(-s * 0.06, s * 0.06), s * 0.07, Color32::WHITE);
    p.line_segment(
        [
            center + Vec2::new(s * 0.01, s * 0.06),
            center + Vec2::new(s * 0.01, -s * 0.18),
        ],
        Stroke::new(s * 0.05, Color32::WHITE),
    );
    p.line_segment(
        [
            center + Vec2::new(s * 0.01, -s * 0.18),
            center + Vec2::new(s * 0.14, -s * 0.22),
        ],
        Stroke::new(s * 0.05, Color32::WHITE),
    );
}

fn draw_executable(p: &Painter, r: Rect) {
    let s = scale_rect(r);
    let center = r.center();
    p.circle_filled(center, s * 0.40, Color32::from_rgb(189, 189, 189));
    p.circle_stroke(
        center,
        s * 0.40,
        Stroke::new(0.5, Color32::from_rgb(120, 120, 120)),
    );
    // 齿轮简化为 6 条辐条
    for i in 0..6 {
        let a = i as f32 * std::f32::consts::FRAC_PI_3;
        let dir = Vec2::new(a.cos(), a.sin());
        p.line_segment(
            [center + dir * s * 0.12, center + dir * s * 0.28],
            Stroke::new(s * 0.07, Color32::from_rgb(90, 90, 90)),
        );
    }
    p.circle_filled(center, s * 0.10, Color32::from_rgb(90, 90, 90));
}
