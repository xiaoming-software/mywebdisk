//! 用户主目录下的会话工作区：`~/mywebdisk_worckspase/<server_token>/tem/`

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

pub const WORKSPACE_DIR_NAME: &str = "mywebdisk_worckspase";
pub const SESSION_TEMP_DIR_NAME: &str = "tem";

pub fn home_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            return PathBuf::from(profile);
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    std::env::temp_dir()
}

pub fn workspace_root() -> PathBuf {
    home_dir().join(WORKSPACE_DIR_NAME)
}

/// 将 server token 转为安全目录名（保留用户给的 token 字符，仅替换路径非法字符）。
pub fn sanitize_token(token: &str) -> String {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return "session".to_string();
    }
    trimmed
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0') {
                '_'
            } else {
                c
            }
        })
        .collect()
}

pub fn session_dir(token: &str) -> PathBuf {
    workspace_root().join(sanitize_token(token))
}

pub fn session_temp_dir(token: &str) -> PathBuf {
    session_dir(token).join(SESSION_TEMP_DIR_NAME)
}

pub fn ensure_session_temp(token: &str) -> std::io::Result<PathBuf> {
    let dir = session_temp_dir(token);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn sanitize_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let sanitized: String = base
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0') {
                '_'
            } else {
                c
            }
        })
        .collect();
    if sanitized.is_empty() {
        "video".to_string()
    } else {
        sanitized
    }
}

/// 关闭会话时清空 `tem` 目录内所有临时文件。
pub fn clean_session_temp(token: &str) {
    let dir = session_temp_dir(token);
    if !dir.exists() {
        return;
    }
    match std::fs::read_dir(&dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let _ = std::fs::remove_dir_all(&path);
                } else {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        Err(e) => {
            eprintln!(
                "[mywebdisk-desktop][workspace] clean_session_temp read_dir {}: {e}",
                dir.display()
            );
        }
    }
}

fn hash_api_path(api_path: &str) -> String {
    let mut hasher = DefaultHasher::new();
    api_path.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// 临时媒体文件路径（含 api 路径哈希，避免同名文件互相覆盖或播错）。
pub fn temp_media_path(token: &str, req_id: &str, api_path: &str, display_name: &str) -> PathBuf {
    let safe = sanitize_file_name(display_name);
    let path_tag = hash_api_path(api_path);
    session_temp_dir(token).join(format!("{req_id}_{path_tag}_{safe}"))
}

/// 开始新的在线播放前清空会话 `tem` 目录，避免系统播放器打开旧文件。
pub fn clear_session_media_temps(token: &str) {
    let dir = session_temp_dir(token);
    if !dir.exists() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[allow(dead_code)]
pub fn is_under_session_temp(token: &str, path: &Path) -> bool {
    path.starts_with(session_temp_dir(token))
}
