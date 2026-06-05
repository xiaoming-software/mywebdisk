//! 上传前释放占用目标文件的进程（会话关闭同步办公文档时使用）。

use std::io;
use std::path::Path;
use std::process::Command;
use std::{thread, time::Duration};

pub fn can_read_file(path: &Path) -> bool {
    std::fs::File::open(path).is_ok()
}

/// 尝试结束正在占用该文件路径的进程，以便读取并上传。
pub fn force_release(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        force_release_unix(path)
    }
    #[cfg(windows)]
    {
        force_release_windows(path)
    }
}

#[cfg(unix)]
fn force_release_unix(path: &Path) -> io::Result<()> {
    let output = Command::new("lsof").args(["-t", "--"]).arg(path).output();
    let output = match output {
        Ok(o) => o,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if !output.status.success() {
        return Ok(());
    }
    let pids = String::from_utf8_lossy(&output.stdout);
    for pid in pids.split_whitespace() {
        if pid.is_empty() {
            continue;
        }
        let _ = Command::new("kill").args(["-TERM", pid]).status();
    }
    thread::sleep(Duration::from_millis(400));
    for pid in pids.split_whitespace() {
        if pid.is_empty() {
            continue;
        }
        let _ = Command::new("kill").args(["-9", pid]).status();
    }
    thread::sleep(Duration::from_millis(200));
    Ok(())
}

#[cfg(windows)]
fn force_release_windows(path: &Path) -> io::Result<()> {
    let path_str = path.to_string_lossy().replace('\'', "''");
    let script = format!(
        r#"$p = (Resolve-Path -LiteralPath '{path_str}' -ErrorAction SilentlyContinue).Path
if (-not $p) {{ exit 0 }}
Get-Process | Where-Object {{ $_.Path }} | ForEach-Object {{
  try {{
    $_.Modules | Where-Object {{ $_.FileName -eq $p }} | Out-Null
    if ($?) {{ Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue }}
  }} catch {{}}
}}
"#,
    );
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// 为上传准备可读文件：先直接读；失败则释放占用后重试复制到旁路文件。
pub fn prepare_file_for_upload(src: &Path) -> io::Result<std::path::PathBuf> {
    if can_read_file(src) {
        return Ok(src.to_path_buf());
    }
    force_release(src)?;
    thread::sleep(Duration::from_millis(300));
    if can_read_file(src) {
        return Ok(src.to_path_buf());
    }
    let staging = src.with_extension("mywebdisk-upload.part");
    let _ = std::fs::remove_file(&staging);
    std::fs::copy(src, &staging)?;
    Ok(staging)
}
