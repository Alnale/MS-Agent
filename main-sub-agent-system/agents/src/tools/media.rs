//! 媒体控制工具执行器
//!
//! 控制前端的图片背景、视频背景、音乐播放功能。
//! 支持：导入文件到媒体库、设置背景、播放/暂停/切歌等。

use async_trait::async_trait;
use agent_core::tool::{
    Tool, ToolBuilder, ToolCall, ToolExecutionContext, ToolExecutor, ToolResult, tool_error, tool_success,
};
use agent_core::error::Result;

pub struct MediaTool;

impl MediaTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MediaTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolExecutor for MediaTool {
    fn executor_id(&self) -> &str {
        "media"
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            ToolBuilder::new("media")
                .description(concat!(
                    "媒体控制工具：控制图片背景、视频背景、音乐播放。\n",
                    "action 参数：\n",
                    "  import_and_set_bg_image — 导入图片文件并设为背景（需 file_path）\n",
                    "  import_and_set_bg_video — 导入视频文件并设为背景（需 file_path）\n",
                    "  import_and_play_music — 导入音乐文件并播放（需 file_path）\n",
                    "  set_bg_image — 切换到已有的图片背景（需 file_name 或 file_path）\n",
                    "  set_bg_video — 切换到已有的视频背景（需 file_name 或 file_path）\n",
                    "  activate_bg_video — 激活已存储的自定义视频背景（无需参数，适用于「切换到自定义视频背景」）\n",
                    "  activate_bg_image — 激活已存储的自定义图片背景（无需参数，适用于「切换到自定义图片背景」）\n",
                    "  play_music — 播放音乐（可选 file_name 指定曲目，否则播放当前曲目）\n",
                    "  pause_music — 暂停音乐播放\n",
                    "  resume_music — 恢复音乐播放\n",
                    "  next_track — 切换到下一首歌\n",
                    "  prev_track — 切换到上一首歌\n",
                    "  toggle_mute — 切换静音状态\n",
                    "  set_volume — 设置音量（需 volume，0-100）\n",
                    "  clear_bg — 清除背景（恢复默认）\n",
                    "  get_status — 获取当前媒体状态\n\n",
                    "工具联动：\n",
                    "- file(list/glob) 可查找本地媒体文件\n",
                    "- file(info) 可查看媒体文件信息",
                ))
                .executor("media")
                .tag("media")
                .tag("control")
                .timeout(30_000)
                .param_enum("action", "操作类型", &[
                    "import_and_set_bg_image", "import_and_set_bg_video", "import_and_play_music",
                    "set_bg_image", "set_bg_video", "activate_bg_video", "activate_bg_image",
                    "play_music", "pause_music", "resume_music", "next_track", "prev_track",
                    "toggle_mute", "set_volume", "clear_bg", "get_status",
                ], true)
                .param_string("file_path", "文件完整路径（用于导入和切换背景/音乐）", false)
                .param_string("file_name", "文件名（用于在已有库中查找）", false)
                .param_integer("volume", "音量 0-100（用于 set_volume）", false)
                .build(),
        ]
    }

    async fn execute(&self, call: &ToolCall, _ctx: &ToolExecutionContext) -> Result<ToolResult> {
        let start = std::time::Instant::now();
        let action = match call.arguments["action"].as_str() {
            Some(a) => a,
            None => {
                let ms = start.elapsed().as_millis() as u64;
                return Ok(tool_error(call, "缺少必需参数 'action'", "参数 'action' 未提供", "请提供 action 参数", ms));
            }
        };

        let result = match action {
            "import_and_set_bg_image" | "import_and_set_bg_video" | "import_and_play_music" => {
                self.handle_import(call, action)
            }
            "set_bg_image" | "set_bg_video" | "play_music" => {
                self.handle_set_existing(call, action)
            }
            "pause_music" | "resume_music" | "next_track" | "prev_track" | "toggle_mute" | "clear_bg"
            | "activate_bg_video" | "activate_bg_image" => {
                Ok(serde_json::json!({ "command": action }))
            }
            "set_volume" => {
                let volume = call.arguments["volume"].as_i64().unwrap_or(50).clamp(0, 100);
                Ok(serde_json::json!({ "command": "set_volume", "volume": volume }))
            }
            "get_status" => {
                Ok(serde_json::json!({ "command": "get_status" }))
            }
            _ => {
                let ms = start.elapsed().as_millis() as u64;
                return Ok(tool_error(call, format!("未知操作 '{}'", action), "", "请使用有效的 action", ms));
            }
        };

        let ms = start.elapsed().as_millis() as u64;
        match result {
            Ok(output) => Ok(tool_success(call, output, ms)),
            Err(e) => Ok(tool_error(call, "媒体操作失败", e.to_string(), "请检查文件路径是否正确", ms)),
        }
    }
}

impl MediaTool {
    fn handle_import(&self, call: &ToolCall, action: &str) -> Result<serde_json::Value> {
        let file_path = call.arguments["file_path"]
            .as_str()
            .ok_or_else(|| agent_core::error::AgentTeamsError::NotFound("file_path 参数不能为空".to_string()))?;

        let path = std::path::Path::new(file_path);
        if !path.exists() {
            return Err(agent_core::error::AgentTeamsError::NotFound(
                format!("文件不存在: {}", file_path)
            ));
        }

        // Size check (100MB limit for base64 transfer)
        let metadata = std::fs::metadata(path).map_err(|e|
            agent_core::error::AgentTeamsError::NotFound(format!("无法读取文件元数据: {}", e))
        )?;
        if metadata.len() > 100 * 1024 * 1024 {
            return Err(agent_core::error::AgentTeamsError::NotFound(
                format!("文件过大 ({:.1}MB)，超过100MB限制", metadata.len() as f64 / 1024.0 / 1024.0)
            ));
        }

        let data = std::fs::read(path).map_err(|e|
            agent_core::error::AgentTeamsError::NotFound(format!("无法读取文件: {}", e))
        )?;
        let b64 = base64_encode(&data);
        let file_name = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let ext = path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let mime = guess_mime(&ext);

        let media_type = match action {
            "import_and_set_bg_image" => "image",
            "import_and_set_bg_video" => "video",
            "import_and_play_music" => "music",
            _ => "unknown",
        };

        let command = match action {
            "import_and_set_bg_image" => "import_and_set_bg_image",
            "import_and_set_bg_video" => "import_and_set_bg_video",
            "import_and_play_music" => "import_and_play_music",
            _ => "unknown",
        };

        Ok(serde_json::json!({
            "command": command,
            "file_name": file_name,
            "file_data": b64,
            "mime_type": mime,
            "media_type": media_type,
            "size": metadata.len(),
        }))
    }

    fn handle_set_existing(&self, call: &ToolCall, action: &str) -> Result<serde_json::Value> {
        let file_name = call.arguments["file_name"]
            .as_str()
            .or_else(|| {
                call.arguments["file_path"].as_str().and_then(|p| {
                    std::path::Path::new(p).file_stem().and_then(|s| s.to_str())
                })
            })
            .unwrap_or("");

        let command = match action {
            "set_bg_image" => "set_bg_image",
            "set_bg_video" => "set_bg_video",
            "play_music" => "play_music",
            _ => "unknown",
        };

        Ok(serde_json::json!({
            "command": command,
            "file_name": file_name,
        }))
    }
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 { result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char); } else { result.push('='); }
        if chunk.len() > 2 { result.push(CHARS[(triple & 0x3F) as usize] as char); } else { result.push('='); }
    }
    result
}

fn guess_mime(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "ogg" | "ogv" => "video/ogg",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        "m4a" => "audio/mp4",
        "wma" => "audio/x-ms-wma",
        _ => "application/octet-stream",
    }
}
