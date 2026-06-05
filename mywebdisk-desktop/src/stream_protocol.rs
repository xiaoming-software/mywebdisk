//! MWDK 二进制分块帧（与服务端 `internal/protocol/stream.go` 一致）

pub const STREAM_MAGIC: &[u8; 4] = b"MWDK";
pub const STREAM_VERSION: u8 = 1;
pub const STREAM_FLAG_CHUNK: u8 = 0;
pub const STREAM_FLAG_DONE: u8 = 1;
pub const STREAM_FLAG_ERROR: u8 = 2;

/// 视频/音频在线播放（分块拉流）体积上限
pub const MAX_MEDIA_STREAM_BYTES: u64 = 200 * 1024 * 1024;
/// 缓冲达到该大小后尝试用系统播放器打开（边下边播）
pub const MIN_VIDEO_START_BYTES: u64 = 3 * 1024 * 1024;
pub const MIN_AUDIO_START_BYTES: u64 = 512 * 1024;

pub fn is_stream_frame(payload: &[u8]) -> bool {
    payload.len() >= 4 && payload.starts_with(STREAM_MAGIC)
}

pub fn decode_stream_frame(
    payload: &[u8],
) -> Option<Result<StreamFrame, &'static str>> {
    if !is_stream_frame(payload) {
        return None;
    }
    if payload.len() < 20 {
        return Some(Err("stream frame too short"));
    }
    if payload[4] != STREAM_VERSION {
        return Some(Err("stream frame bad version"));
    }
    let flag = payload[5];
    let id_len = u16::from_be_bytes([payload[6], payload[7]]) as usize;
    let head = 8 + id_len;
    if payload.len() < head + 8 + 4 {
        return Some(Err("stream frame truncated"));
    }
    let req_id = match std::str::from_utf8(&payload[8..head]) {
        Ok(s) => s.to_string(),
        Err(_) => return Some(Err("stream frame invalid request id")),
    };
    let offset = u64::from_be_bytes(payload[head..head + 8].try_into().ok()?);
    let data_len = u32::from_be_bytes(payload[head + 8..head + 12].try_into().ok()?) as usize;
    let data_off = head + 12;
    if payload.len() < data_off + data_len {
        return Some(Err("stream frame data truncated"));
    }
    let data = payload[data_off..data_off + data_len].to_vec();
    Some(Ok(StreamFrame {
        req_id,
        flag,
        offset,
        data,
    }))
}

pub struct StreamFrame {
    pub req_id: String,
    pub flag: u8,
    pub offset: u64,
    pub data: Vec<u8>,
}
