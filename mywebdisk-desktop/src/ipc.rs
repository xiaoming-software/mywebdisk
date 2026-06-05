use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_uint};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use rand::Rng;

/// 常规 `WebrpcClient_SendData` 超时（毫秒）
pub const SEND_DATA_TIMEOUT_MS: i64 = 5_000;
/// 写入大文本时的超时（毫秒）
pub const SEND_DATA_WRITE_TIMEOUT_MS: i64 = 60_000;
/// 办公文档上传 `upload` 准备与 `SendFile` 超时（毫秒）
pub const SEND_DATA_UPLOAD_TIMEOUT_MS: i64 = 120_000;

pub struct WebrpcIpc {
    handle: usize,
}

unsafe extern "C" {
    fn WebrpcClient_New(token: *mut c_char, passwd: *mut c_char, permission: *mut c_char) -> usize;
    fn WebrpcClient_LoginStatus(handle: usize) -> c_int;
    fn WebrpcClient_GetReceivePort(handle: usize) -> c_int;
    fn WebrpcClient_OpenSession(handle: usize, to_token: *mut c_char, permission: *mut c_char) -> c_uint;
    fn WebrpcClient_CloseSession(handle: usize, session_id: c_uint) -> c_int;
    fn WebrpcClient_SendData(
        handle: usize,
        session_id: c_uint,
        data: *mut c_char,
        data_len: c_int,
        timeout_ms: i64,
    ) -> c_int;
    fn WebrpcClient_SendFile(handle: usize, session_id: c_uint, file_path: *mut c_char) -> c_int;
    fn WebrpcClient_Free(handle: usize);
}

impl WebrpcIpc {
    pub fn start() -> Result<Self, String> {
        Ok(Self { handle: 0 })
    }

    pub fn login(&mut self, token: &str, passwd: &str, permission: &str) -> Result<(), String> {
        eprintln!(
            "[mywebdisk-desktop][ipc] login start token_len={} passwd_len={} permission_len={}",
            token.len(),
            passwd.len(),
            permission.len()
        );
        if self.handle != 0 {
            self.free();
        }

        let token_c = CString::new(token).map_err(|_| "token contains NUL byte".to_string())?;
        let passwd_c = CString::new(passwd).map_err(|_| "passwd contains NUL byte".to_string())?;
        let permission_c =
            CString::new(permission).map_err(|_| "permission contains NUL byte".to_string())?;

        self.handle = unsafe {
            WebrpcClient_New(
                token_c.as_ptr() as *mut c_char,
                passwd_c.as_ptr() as *mut c_char,
                permission_c.as_ptr() as *mut c_char,
            )
        };
        if self.handle == 0 {
            eprintln!("[mywebdisk-desktop][ipc] WebrpcClient_New returned 0");
            return Err("WebrpcClient_New returned null handle".into());
        }
        eprintln!("[mywebdisk-desktop][ipc] WebrpcClient_New handle={}", self.handle);

        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let status = unsafe { WebrpcClient_LoginStatus(self.handle) };
            eprintln!("[mywebdisk-desktop][ipc] login status={status}");
            if status != 0 {
                eprintln!("[mywebdisk-desktop][ipc] login success");
                return Ok(());
            }
            thread::sleep(Duration::from_millis(200));
        }

        self.free();
        eprintln!("[mywebdisk-desktop][ipc] login timeout");
        Err("login timeout (10s)".into())
    }

    pub fn open_session(&mut self, to_token: &str, permission: &str) -> Result<u32, String> {
        eprintln!(
            "[mywebdisk-desktop][ipc] open_session start to_token='{}' permission_len={}",
            to_token,
            permission.len()
        );
        if self.handle == 0 {
            return Err("webrpc handle is not ready".into());
        }

        let to_token_c =
            CString::new(to_token).map_err(|_| "target token contains NUL byte".to_string())?;
        let permission_c =
            CString::new(permission).map_err(|_| "permission contains NUL byte".to_string())?;

        let t0 = Instant::now();
        let session_id = unsafe {
            WebrpcClient_OpenSession(
                self.handle,
                to_token_c.as_ptr() as *mut c_char,
                permission_c.as_ptr() as *mut c_char,
            )
        };
        eprintln!(
            "[mywebdisk-desktop][ipc] WebrpcClient_OpenSession elapsed_ms={}",
            t0.elapsed().as_millis()
        );
        if session_id == 0 {
            eprintln!("[mywebdisk-desktop][ipc] WebrpcClient_OpenSession returned 0");
            return Err("WebrpcClient_OpenSession failed".into());
        }
        eprintln!("[mywebdisk-desktop][ipc] open_session raw session_id={session_id}");

        // 以 SDK 约定为准：OpenSession 返回非 0 即成功，会话后续由收发链路判定可用性。
        eprintln!("[mywebdisk-desktop][ipc] open_session accepted session_id={session_id}");
        Ok(session_id)
    }

    /// `WebrpcClient_CloseSession`：返回 1 表示成功，0 表示失败。
    pub fn close_session(&self, session_id: u32) -> Result<(), String> {
        if self.handle == 0 {
            return Err("webrpc handle is not ready".into());
        }
        let t0 = Instant::now();
        let ret = unsafe { WebrpcClient_CloseSession(self.handle, session_id) };
        eprintln!(
            "[mywebdisk-desktop][ipc] WebrpcClient_CloseSession elapsed_ms={} session_id={} ret={}",
            t0.elapsed().as_millis(),
            session_id,
            ret
        );
        if ret == 0 {
            return Err(format!("WebrpcClient_CloseSession failed: {ret}"));
        }
        eprintln!("[mywebdisk-desktop][ipc] close_session ok session_id={session_id}");
        Ok(())
    }

    pub fn receive_port(&self) -> Result<i32, String> {
        if self.handle == 0 {
            return Err("webrpc handle is not ready".into());
        }
        let port = unsafe { WebrpcClient_GetReceivePort(self.handle) };
        if port <= 0 {
            eprintln!("[mywebdisk-desktop][ipc] invalid callback port={port}");
            return Err(format!("invalid callback port: {port}"));
        }
        eprintln!("[mywebdisk-desktop][ipc] callback port={port}");
        Ok(port)
    }

    pub fn send_data(&self, session_id: u32, data: &[u8]) -> Result<(), String> {
        self.send_data_with_timeout(session_id, data, SEND_DATA_TIMEOUT_MS)
    }

    pub fn send_data_with_timeout(
        &self,
        session_id: u32,
        data: &[u8],
        timeout_ms: i64,
    ) -> Result<(), String> {
        if self.handle == 0 {
            return Err("webrpc handle is not ready".into());
        }
        if data.is_empty() {
            return Err("payload is empty".into());
        }
        let t0 = Instant::now();
        let ret = unsafe {
            WebrpcClient_SendData(
                self.handle,
                session_id,
                data.as_ptr() as *mut c_char,
                data.len() as c_int,
                timeout_ms,
            )
        };
        eprintln!(
            "[mywebdisk-desktop][ipc] WebrpcClient_SendData elapsed_ms={} session_id={} timeout_ms={}",
            t0.elapsed().as_millis(),
            session_id,
            timeout_ms
        );
        if ret == 0 {
            eprintln!(
                "[mywebdisk-desktop][ipc] SendData failed session_id={} ret={} payload={}",
                session_id,
                ret,
                String::from_utf8_lossy(data)
            );
            return Err(format!("WebrpcClient_SendData failed: {ret}"));
        }
        eprintln!(
            "[mywebdisk-desktop][ipc] SendData ok session_id={} payload={}",
            session_id,
            String::from_utf8_lossy(data)
        );
        Ok(())
    }

    /// `WebrpcClient_SendFile`：返回非 0 表示成功（与 `SendData` 一致）。
    pub fn send_file(&self, session_id: u32, file_path: &Path) -> Result<(), String> {
        if self.handle == 0 {
            return Err("webrpc handle is not ready".into());
        }
        let path_str = file_path
            .to_str()
            .ok_or_else(|| format!("file path is not valid UTF-8: {}", file_path.display()))?;
        let path_c =
            CString::new(path_str).map_err(|_| "file path contains NUL byte".to_string())?;
        let t0 = Instant::now();
        let ret = unsafe {
            WebrpcClient_SendFile(
                self.handle,
                session_id,
                path_c.as_ptr() as *mut c_char,
            )
        };
        eprintln!(
            "[mywebdisk-desktop][ipc] WebrpcClient_SendFile elapsed_ms={} session_id={} path={} ret={}",
            t0.elapsed().as_millis(),
            session_id,
            path_str,
            ret
        );
        if ret == 0 {
            return Err(format!("WebrpcClient_SendFile failed: {ret}"));
        }
        Ok(())
    }

    fn free(&mut self) {
        if self.handle != 0 {
            unsafe { WebrpcClient_Free(self.handle) };
            self.handle = 0;
        }
    }

    pub fn close(&mut self) {
        self.free();
    }
}

impl Drop for WebrpcIpc {
    fn drop(&mut self) {
        self.free();
    }
}

pub fn generate_permission() -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

