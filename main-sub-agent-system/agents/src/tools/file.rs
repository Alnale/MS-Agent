//! 文件工具执行器
//!
//! 统一的文件操作工具，支持读写、列表、查询、删除。
//! 合并了原 file_read/file_write/file_list/file_exists/file_delete/file_info 6 个工具。

use async_trait::async_trait;

use agent_teams_core::tool::{
    Tool, ToolBuilder, ToolCall, ToolExecutionContext, ToolExecutor, ToolResult, tool_error, tool_success,
};
use agent_teams_core::error::Result;

/// Normalize a file path that may come from an LLM with various encoding issues.
///
/// Handles:
/// - URL-encoded paths (e.g., `C%3A%5CUsers` -> `C:\Users`)
/// - Forward slashes on Windows (`C:/Users` -> `C:\Users`)
/// - Double backslashes (`C:\\Users` -> `C:\Users`)
/// - Leading/trailing whitespace and quotes
/// - Unicode escape sequences
fn normalize_path(raw: &str) -> String {
    let original = raw;
    let mut path = raw.trim().to_string();

    // Remove surrounding quotes if present
    if (path.starts_with('"') && path.ends_with('"'))
        || (path.starts_with('\'') && path.ends_with('\''))
    {
        path = path[1..path.len() - 1].to_string();
    }

    // URL-decode (handle %XX sequences, including multi-byte UTF-8)
    if path.contains('%') {
        let mut decoded_bytes = Vec::with_capacity(path.len());
        let bytes: Vec<u8> = path.as_bytes().to_vec();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    decoded_bytes.push(byte);
                    i += 3;
                    continue;
                }
            }
            decoded_bytes.push(bytes[i]);
            i += 1;
        }
        // Convert decoded bytes to UTF-8 string
        path = String::from_utf8_lossy(&decoded_bytes).into_owned();
    }

    // Replace forward slashes with backslashes on Windows
    #[cfg(target_os = "windows")]
    {
        path = path.replace('/', "\\");
    }

    // Collapse double backslashes (but keep UNC paths \\server\share)
    if path.starts_with("\\\\") {
        // UNC path: keep the leading \\, collapse the rest
        let rest = &path[2..];
        path = format!("\\\\{}", rest.replace("\\\\", "\\"));
    } else {
        path = path.replace("\\\\", "\\");
    }

    // Trim trailing backslash (unless it's a root like C:\)
    if path.ends_with('\\') && !path.ends_with(":\\") && path.len() > 1 {
        path.pop();
    }

    // Log the normalization for debugging
    if original != path {
        tracing::debug!(
            original = %original,
            normalized = %path,
            "normalize_path: path was transformed"
        );
    }

    path
}

/// 统一文件工具（合并 6 个原工具）
pub struct FileTool;

impl FileTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolExecutor for FileTool {
    fn executor_id(&self) -> &str {
        "file"
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            ToolBuilder::new("file")
                .description(concat!(
                    "统一文件与目录操作工具。处理所有本地文件系统操作。\n",
                    "当用户提到本地路径（C:\\、/home/、./、../等）或要求查看/操作文件时，必须使用此工具。\n",
                    "路径格式：Windows 使用 C:\\Users\\... 或 C:/Users/...（正斜杠也可），Linux/macOS 使用 /home/...\n\n",
                    "⚠️ 重要：read 操作仅适用于纯文本文件（.txt/.md/.py/.js/.json/.csv/.html/.css 等）。\n",
                    "对于 PDF/DOCX/DOC 等文档文件，必须使用 docreader 工具读取内容。\n",
                    "如果用 file(read) 读取文档文件返回乱码或二进制数据，请改用 docreader。\n\n",
                    "action 参数：\n",
                    "  read — 读取文本文件内容（支持编码检测、行范围截取、最大字节数限制）\n",
                    "  write — 写入文件（自动创建目录，支持追加模式）\n",
                    "  list — 列出目录内容（查看目录下有哪些文件和子目录，支持通配符过滤、递归、隐藏文件）\n",
                    "  info — 获取文件详细信息（大小、修改时间、权限、是否存在）\n",
                    "  delete — 删除文件或目录\n",
                    "  exists — 快速检查文件/目录是否存在\n",
                    "  search — 在文件中搜索文本（grep 功能，支持正则）\n",
                    "  copy — 复制文件或目录（需 dest 参数）\n",
                    "  move — 移动/重命名文件或目录（需 dest 参数）\n",
                    "  mkdir — 创建目录（自动创建父目录）\n",
                    "  glob — 按模式匹配查找文件（需 pattern 参数，如 '*.rs' 或 '**/*.json'）\n\n",
                    "工具联动：\n",
                    "- http_request/http_get 下载内容可用 write 保存\n",
                    "- write 写入的文件可作为 docflow/docreader/xxt 的输入\n",
                    "- list/glob 查找的媒体文件可用 media 导入\n",
                    "- read 读取的答案文件可传给 xxt fill",
                ))
                .executor("file")
                .tag("filesystem")
                .tag("io")
                .timeout(30_000)
                .data_flow("write: 接受内联 content 字符串，输出文件路径")
                .data_flow("read: 接受文件路径，输出文件内容字符串")
                .data_flow("当其它工具需要从本地文件读取参数时，可先用 file(write) 将上下文数据写入临时文件")
                .output_field("path: 操作的文件路径")
                .output_field("content: 读取的文件内容（read 操作）")
                .output_field("bytes_written: 写入字节数（write 操作）")
                .output_field("success: 操作是否成功")
                .param_enum("action", "操作类型", &["read", "write", "list", "info", "delete", "exists", "search", "copy", "move", "mkdir", "glob"], true)
                .param_string("path", "文件或目录路径（绝对路径或相对路径）", true)
                .param_string("content", "要写入的内容（write 操作时必填）", false)
                .param_string("dest", "目标路径（copy/move 操作时必填）", false)
                .param_string("pattern", "通配符模式（list/glob 操作时使用，如 '*.rs' 或 '**/*.json'）或搜索文本（search 操作时使用）", false)
                .param_bool("append", "是否追加模式（write 操作），默认 false（覆盖）", false)
                .param_bool("recursive", "是否递归（list/copy/delete 操作默认 false）", false)
                .param_bool("show_hidden", "是否显示隐藏文件（list 操作），默认 false", false)
                .param_integer("max_depth", "递归最大深度（list/glob 操作），默认 5", false)
                .param_integer("max_bytes", "最大读取字节数（read 操作），默认 1MB", false)
                .param_string("encoding", "编码格式（read 操作：utf-8/gbk/gb2312/auto），默认 auto", false)
                .param_integer("start_line", "起始行号（read 操作，从 1 开始）", false)
                .param_integer("end_line", "结束行号（read 操作）", false)
                .param_integer("context_lines", "搜索结果上下文行数（search 操作），默认 0", false)
                .param_bool("case_sensitive", "搜索是否区分大小写（search 操作），默认 true", false)
                .build(),
        ]
    }

    async fn execute(
        &self,
        call: &ToolCall,
        _ctx: &ToolExecutionContext,
    ) -> Result<ToolResult> {
        let start = std::time::Instant::now();

        let action = match call.arguments["action"].as_str() {
            Some(a) => a,
            None => {
                let ms = start.elapsed().as_millis() as u64;
                return Ok(tool_error(call,
                    "缺少必需参数 'action'",
                    "参数 'action' 未提供或不是字符串",
                    "请提供 action 参数：read/write/list/info/delete/exists/search",
                    ms,
                ));
            }
        };

        let raw_path = match call.arguments["path"].as_str() {
            Some(p) => p,
            None => {
                let ms = start.elapsed().as_millis() as u64;
                return Ok(tool_error(call,
                    "缺少必需参数 'path'",
                    "参数 'path' 未提供或不是字符串",
                    "请提供文件或目录路径",
                    ms,
                ));
            }
        };
        let path = &normalize_path(raw_path);

        // Debug logging for path normalization
        if raw_path != path {
            tracing::info!(
                tool = "file",
                action = action,
                raw_path = %raw_path,
                normalized_path = %path,
                "Path normalized"
            );
        }

        let ms_fn = || start.elapsed().as_millis() as u64;

        match action {
            "read" => match self.execute_read(call, path).await {
                Ok(output) => Ok(tool_success(call, output, ms_fn())),
                Err(e) => Ok(tool_error(call, format!("读取文件失败: {}", e), format!("路径: {}", path), "请检查文件路径和编码参数", ms_fn())),
            },
            "write" => match self.execute_write(call, path).await {
                Ok(output) => Ok(tool_success(call, output, ms_fn())),
                Err(e) => Ok(tool_error(call, format!("写入文件失败: {}", e), format!("路径: {}", path), "请检查路径和写入权限", ms_fn())),
            },
            "list" => match self.execute_list(call, path).await {
                Ok(output) => Ok(tool_success(call, output, ms_fn())),
                Err(e) => Ok(tool_error(call, format!("列出目录失败: {}", e), format!("路径: {}", path), "请检查路径是否存在且有读取权限", ms_fn())),
            },
            "info" => match self.execute_info(path).await {
                Ok(output) => Ok(tool_success(call, output, ms_fn())),
                Err(e) => Ok(tool_error(call, format!("获取信息失败: {}", e), format!("路径: {}", path), "请检查路径是否存在", ms_fn())),
            },
            "delete" => match self.execute_delete(call, path).await {
                Ok(output) => Ok(tool_success(call, output, ms_fn())),
                Err(e) => Ok(tool_error(call, format!("删除失败: {}", e), format!("路径: {}", path), "请检查路径和权限", ms_fn())),
            },
            "exists" => {
                let meta = tokio::fs::metadata(path).await;
                let exists = meta.is_ok();
                let is_file = meta.as_ref().map(|m| m.is_file()).unwrap_or(false);
                let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                Ok(tool_success(call, serde_json::json!({
                    "exists": exists, "is_file": is_file, "is_dir": is_dir, "path": path
                }), ms_fn()))
            },
            "search" => match self.execute_search(call, path).await {
                Ok(output) => Ok(tool_success(call, output, ms_fn())),
                Err(e) => Ok(tool_error(call, format!("搜索失败: {}", e), format!("路径: {}", path), "请检查路径和搜索参数", ms_fn())),
            },
            "copy" => match self.execute_copy(call, path).await {
                Ok(output) => Ok(tool_success(call, output, ms_fn())),
                Err(e) => Ok(tool_error(call, format!("复制失败: {}", e), format!("路径: {}", path), "请检查源路径和目标路径", ms_fn())),
            },
            "move" => match self.execute_move(call, path).await {
                Ok(output) => Ok(tool_success(call, output, ms_fn())),
                Err(e) => Ok(tool_error(call, format!("移动失败: {}", e), format!("路径: {}", path), "请检查源路径和目标路径", ms_fn())),
            },
            "mkdir" => match self.execute_mkdir(path).await {
                Ok(output) => Ok(tool_success(call, output, ms_fn())),
                Err(e) => Ok(tool_error(call, format!("创建目录失败: {}", e), format!("路径: {}", path), "请检查路径和权限", ms_fn())),
            },
            "glob" => match self.execute_glob(call, path).await {
                Ok(output) => Ok(tool_success(call, output, ms_fn())),
                Err(e) => Ok(tool_error(call, format!("glob匹配失败: {}", e), format!("路径: {}", path), "请检查路径和pattern", ms_fn())),
            },
            _ => Ok(tool_error(call,
                format!("未知操作 '{}'", action),
                format!("action='{}' 不是有效的操作类型", action),
                "请使用：read/write/list/info/delete/exists/search",
                ms_fn(),
            )),
        }
    }
}

impl FileTool {
    /// 读取文件内容
    async fn execute_read(&self, call: &ToolCall, path: &str) -> Result<serde_json::Value> {
        let max_bytes = call.arguments["max_bytes"].as_u64().unwrap_or(1_048_576) as usize;
        let start_line = call.arguments["start_line"].as_u64().map(|v| v as usize);
        let end_line = call.arguments["end_line"].as_u64().map(|v| v as usize);

        let bytes = tokio::fs::read(path).await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("IO错误: {}", e)))?;

        let truncated: &[u8] = if bytes.len() > max_bytes { &bytes[..max_bytes] } else { &bytes };
        let content = String::from_utf8_lossy(truncated).to_string();

        let final_content = if let (Some(s), Some(e)) = (start_line, end_line) {
            let lines: Vec<&str> = content.lines().collect();
            let s = s.saturating_sub(1);
            let e = e.min(lines.len());
            if s < lines.len() { lines[s..e].join("\n") } else { content }
        } else {
            content
        };

        Ok(serde_json::json!({
            "content": final_content,
            "bytes_read": truncated.len(),
            "total_bytes": bytes.len(),
            "truncated": bytes.len() > max_bytes,
            "line_count": final_content.lines().count(),
            "path": path
        }))
    }

    /// 写入文件
    async fn execute_write(&self, call: &ToolCall, path: &str) -> Result<serde_json::Value> {
        let content = call.arguments["content"]
            .as_str()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("write 操作需要 content 参数".to_string()))?;
        let append = call.arguments["append"].as_bool().unwrap_or(false);

        if let Some(parent) = std::path::Path::new(path).parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("创建目录失败: {}", e)))?;
        }

        if append {
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::OpenOptions::new().create(true).append(true).open(path).await
                .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("打开文件失败: {}", e)))?;
            file.write_all(content.as_bytes()).await
                .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("写入失败: {}", e)))?;
        } else {
            tokio::fs::write(path, content).await
                .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("写入失败: {}", e)))?;
        }

        Ok(serde_json::json!({
            "success": true,
            "path": path,
            "bytes_written": content.len(),
            "mode": if append { "append" } else { "overwrite" }
        }))
    }

    /// 列出目录内容
    async fn execute_list(&self, call: &ToolCall, path: &str) -> Result<serde_json::Value> {
        let pattern = call.arguments["pattern"].as_str();
        let recursive = call.arguments["recursive"].as_bool().unwrap_or(false);
        let show_hidden = call.arguments["show_hidden"].as_bool().unwrap_or(false);
        let max_depth = call.arguments["max_depth"].as_u64().unwrap_or(3) as usize;

        let mut entries = Vec::new();
        list_directory(path, pattern, recursive, show_hidden, 0, max_depth, &mut entries).await?;

        Ok(serde_json::json!({
            "entries": entries,
            "count": entries.len(),
            "path": path,
            "recursive": recursive
        }))
    }

    /// 获取文件信息
    async fn execute_info(&self, path: &str) -> Result<serde_json::Value> {
        let meta = tokio::fs::metadata(path).await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("无法访问: {}", e)))?;

        Ok(serde_json::json!({
            "path": path,
            "exists": true,
            "is_file": meta.is_file(),
            "is_dir": meta.is_dir(),
            "size": meta.len(),
            "readonly": meta.permissions().readonly(),
            "modified": meta.modified().ok().and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
            }),
            "created": meta.created().ok().and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
            }),
        }))
    }

    /// 删除文件或目录
    async fn execute_delete(&self, call: &ToolCall, path: &str) -> Result<serde_json::Value> {
        let recursive = call.arguments["recursive"].as_bool().unwrap_or(false);
        let meta = tokio::fs::metadata(path).await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("路径不存在: {}", e)))?;

        let result = if meta.is_dir() && recursive {
            tokio::fs::remove_dir_all(path).await
        } else if meta.is_dir() {
            tokio::fs::remove_dir(path).await
        } else {
            tokio::fs::remove_file(path).await
        };

        match result {
            Ok(()) => Ok(serde_json::json!({
                "success": true,
                "path": path,
                "type": if meta.is_dir() { "directory" } else { "file" }
            })),
            Err(e) => {
                let details = if meta.is_dir() && !recursive {
                    "目录非空，请添加 recursive=true 参数".to_string()
                } else {
                    format!("IO错误: {}", e)
                };
                Err(agent_teams_core::error::AgentTeamsError::NotFound(details))
            }
        }
    }

    /// 在文件中搜索文本（grep 功能）
    async fn execute_search(&self, call: &ToolCall, path: &str) -> Result<serde_json::Value> {
        let pattern = call.arguments["pattern"]
            .as_str()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("search 操作需要 pattern 参数".to_string()))?;
        let context_lines = call.arguments["context_lines"].as_u64().unwrap_or(0) as usize;
        let case_sensitive = call.arguments["case_sensitive"].as_bool().unwrap_or(true);

        let content = tokio::fs::read_to_string(path).await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("读取失败: {}", e)))?;

        let lines: Vec<&str> = content.lines().collect();
        let mut matches = Vec::new();
        let pattern_lower = if case_sensitive { String::new() } else { pattern.to_lowercase() };

        for (i, line) in lines.iter().enumerate() {
            let found = if case_sensitive {
                line.contains(pattern)
            } else {
                line.to_lowercase().contains(&pattern_lower)
            };

            if found {
                let mut context_before = Vec::new();
                let mut context_after = Vec::new();

                if context_lines > 0 {
                    let start = i.saturating_sub(context_lines);
                    for (j, line) in lines[start..i].iter().enumerate() {
                        context_before.push(format!("{}: {}", start + j + 1, line));
                    }
                    let end = (i + context_lines + 1).min(lines.len());
                    for (j, line) in lines[(i + 1)..end].iter().enumerate() {
                        context_after.push(format!("{}: {}", i + j + 2, line));
                    }
                }

                matches.push(serde_json::json!({
                    "line_number": i + 1,
                    "line": line,
                    "context_before": context_before,
                    "context_after": context_after,
                }));
            }
        }

        Ok(serde_json::json!({
            "path": path,
            "pattern": pattern,
            "match_count": matches.len(),
            "matches": matches,
            "total_lines": lines.len(),
        }))
    }

    /// 复制文件或目录
    async fn execute_copy(&self, call: &ToolCall, path: &str) -> Result<serde_json::Value> {
        let raw_dest = call.arguments["dest"]
            .as_str()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("copy 操作需要 dest 参数".to_string()))?;
        let dest = normalize_path(raw_dest);
        let recursive = call.arguments["recursive"].as_bool().unwrap_or(false);

        let meta = tokio::fs::metadata(path).await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("源路径不存在: {}", e)))?;

        if meta.is_dir() {
            if recursive {
                copy_dir_recursive(path, &dest).await?;
            } else {
                return Err(agent_teams_core::error::AgentTeamsError::NotFound(
                    "复制目录需要 recursive=true 参数".to_string()
                ));
            }
        } else {
            if let Some(parent) = std::path::Path::new(&dest).parent() {
                tokio::fs::create_dir_all(parent).await
                    .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("创建目标目录失败: {}", e)))?;
            }
            tokio::fs::copy(path, &dest).await
                .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("复制失败: {}", e)))?;
        }

        Ok(serde_json::json!({
            "success": true,
            "source": path,
            "dest": dest,
            "is_dir": meta.is_dir(),
        }))
    }

    /// 移动/重命名文件或目录
    async fn execute_move(&self, call: &ToolCall, path: &str) -> Result<serde_json::Value> {
        let raw_dest = call.arguments["dest"]
            .as_str()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("move 操作需要 dest 参数".to_string()))?;
        let dest = normalize_path(raw_dest);

        if let Some(parent) = std::path::Path::new(&dest).parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("创建目标目录失败: {}", e)))?;
        }

        tokio::fs::rename(path, &dest).await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("移动失败: {}", e)))?;

        Ok(serde_json::json!({
            "success": true,
            "source": path,
            "dest": dest,
        }))
    }

    /// 创建目录
    async fn execute_mkdir(&self, path: &str) -> Result<serde_json::Value> {
        tokio::fs::create_dir_all(path).await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("创建目录失败: {}", e)))?;

        Ok(serde_json::json!({
            "success": true,
            "path": path,
            "created": true,
        }))
    }

    /// 按模式匹配查找文件
    async fn execute_glob(&self, call: &ToolCall, path: &str) -> Result<serde_json::Value> {
        let pattern = call.arguments["pattern"]
            .as_str()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("glob 操作需要 pattern 参数".to_string()))?;
        let max_depth = call.arguments["max_depth"].as_u64().unwrap_or(5) as usize;
        let show_hidden = call.arguments["show_hidden"].as_bool().unwrap_or(false);

        let mut entries = Vec::new();
        glob_recursive(path, pattern, show_hidden, 0, max_depth, &mut entries).await?;

        Ok(serde_json::json!({
            "path": path,
            "pattern": pattern,
            "matches": entries,
            "count": entries.len(),
        }))
    }
}

/// Helper: list directory contents recursively
async fn list_directory(
    path: &str,
    pattern: Option<&str>,
    recursive: bool,
    show_hidden: bool,
    current_depth: usize,
    max_depth: usize,
    entries: &mut Vec<serde_json::Value>,
) -> Result<()> {
    if current_depth > max_depth {
        return Ok(());
    }

    tracing::info!(path = %path, depth = current_depth, "list_directory: reading directory");
    tracing::info!(path_bytes = ?path.as_bytes(), "list_directory: path bytes");

    let mut dir = tokio::fs::read_dir(path).await
        .map_err(|e| {
            tracing::error!(path = %path, error = %e, "list_directory: failed to read directory");
            agent_teams_core::error::AgentTeamsError::NotFound(format!("读取目录失败: {}", e))
        })?;

    loop {
        let entry = match dir.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(e) => { tracing::warn!("读取目录项失败: {}", e); continue; }
        };

        let name = entry.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') { continue; }
        if let Some(pat) = pattern { if !glob_match(pat, &name) { continue; } }

        let file_type = entry.file_type().await;
        let is_dir = file_type.map(|ft| ft.is_dir()).unwrap_or(false);

        // Use appropriate path separator for the platform
        #[cfg(target_os = "windows")]
        let entry_path = format!("{}\\{}", path.trim_end_matches('\\'), name);
        #[cfg(not(target_os = "windows"))]
        let entry_path = format!("{}/{}", path.trim_end_matches('/'), name);

        // Get file size and modification time
        let meta = entry.metadata().await.ok();
        let size = meta.as_ref().map(|m| m.len());
        let modified = meta.as_ref().and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs()));

        entries.push(serde_json::json!({
            "name": name,
            "path": entry_path,
            "is_dir": is_dir,
            "depth": current_depth,
            "size": size,
            "modified": modified,
        }));

        if recursive && is_dir {
            Box::pin(list_directory(&entry_path, pattern, recursive, show_hidden, current_depth + 1, max_depth, entries)).await?;
        }
    }
    Ok(())
}

/// Simple glob pattern matching (supports * wildcard)
fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" { return true; }
    if let Some(suffix) = pattern.strip_prefix("*.") { return text.ends_with(&format!(".{}", suffix)); }
    if let Some(prefix) = pattern.strip_suffix(".*") { return text.starts_with(prefix); }
    pattern == text
}

/// Helper: copy directory recursively
async fn copy_dir_recursive(src: &str, dest: &str) -> Result<()> {
    tokio::fs::create_dir_all(dest).await
        .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("创建目标目录失败: {}", e)))?;

    let mut dir = tokio::fs::read_dir(src).await
        .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("读取源目录失败: {}", e)))?;

    loop {
        let entry = match dir.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(e) => { tracing::warn!("读取目录项失败: {}", e); continue; }
        };

        let name = entry.file_name().to_string_lossy().to_string();

        #[cfg(target_os = "windows")]
        let src_path = format!("{}\\{}", src.trim_end_matches('\\'), name);
        #[cfg(not(target_os = "windows"))]
        let src_path = format!("{}/{}", src.trim_end_matches('/'), name);

        #[cfg(target_os = "windows")]
        let dest_path = format!("{}\\{}", dest.trim_end_matches('\\'), name);
        #[cfg(not(target_os = "windows"))]
        let dest_path = format!("{}/{}", dest.trim_end_matches('/'), name);

        let file_type = entry.file_type().await;
        let is_dir = file_type.map(|ft| ft.is_dir()).unwrap_or(false);

        if is_dir {
            Box::pin(copy_dir_recursive(&src_path, &dest_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dest_path).await
                .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(format!("复制文件失败: {}", e)))?;
        }
    }
    Ok(())
}

/// Helper: glob pattern matching recursively
async fn glob_recursive(
    path: &str,
    pattern: &str,
    show_hidden: bool,
    current_depth: usize,
    max_depth: usize,
    entries: &mut Vec<serde_json::Value>,
) -> Result<()> {
    if current_depth > max_depth {
        return Ok(());
    }

    let mut dir = match tokio::fs::read_dir(path).await {
        Ok(d) => d,
        Err(_) => return Ok(()),
    };

    loop {
        let entry = match dir.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(_) => continue,
        };

        let name = entry.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') { continue; }

        #[cfg(target_os = "windows")]
        let entry_path = format!("{}\\{}", path.trim_end_matches('\\'), name);
        #[cfg(not(target_os = "windows"))]
        let entry_path = format!("{}/{}", path.trim_end_matches('/'), name);

        let file_type = entry.file_type().await;
        let is_dir = file_type.map(|ft| ft.is_dir()).unwrap_or(false);

        // Check if name matches the glob pattern
        if glob_match(pattern, &name) {
            let meta = entry.metadata().await.ok();
            let size = meta.as_ref().map(|m| m.len());
            entries.push(serde_json::json!({
                "name": name,
                "path": entry_path,
                "is_dir": is_dir,
                "size": size,
                "depth": current_depth,
            }));
        }

        // Recurse into directories
        if is_dir {
            // For ** patterns, recurse with same pattern
            // For non-** patterns, only recurse if pattern contains no wildcard at this level
            let should_recurse = pattern.starts_with("**/") || !pattern.contains('/');
            if should_recurse {
                Box::pin(glob_recursive(&entry_path, pattern, show_hidden, current_depth + 1, max_depth, entries)).await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path_forward_slashes() {
        let result = normalize_path("C:/Users/asus/Desktop/数据库");
        assert_eq!(result, "C:\\Users\\asus\\Desktop\\数据库");
    }

    #[test]
    fn test_normalize_path_double_backslashes() {
        let result = normalize_path("C:\\\\Users\\\\asus\\\\Desktop\\\\数据库");
        assert_eq!(result, "C:\\Users\\asus\\Desktop\\数据库");
    }

    #[test]
    fn test_normalize_path_url_encoded() {
        let result = normalize_path("C%3A%5CUsers%5Casus%5CDesktop%5C%E6%95%B0%E6%8D%AE%E5%BA%93");
        // %3A = ':', %5C = '\', %E6%95%B0 = '数', %E6%8D%AE = '据', %E5%BA%93 = '库'
        assert_eq!(result, "C:\\Users\\asus\\Desktop\\数据库");
    }

    #[test]
    fn test_normalize_path_with_quotes() {
        let result = normalize_path("\"C:\\Users\\asus\\Desktop\\数据库\"");
        assert_eq!(result, "C:\\Users\\asus\\Desktop\\数据库");
    }

    #[test]
    fn test_normalize_path_with_whitespace() {
        let result = normalize_path("  C:\\Users\\asus\\Desktop\\数据库  ");
        assert_eq!(result, "C:\\Users\\asus\\Desktop\\数据库");
    }

    #[test]
    fn test_normalize_path_trailing_backslash() {
        let result = normalize_path("C:\\Users\\asus\\Desktop\\数据库\\");
        assert_eq!(result, "C:\\Users\\asus\\Desktop\\数据库");
    }

    #[test]
    fn test_normalize_path_root_drive() {
        let result = normalize_path("C:\\");
        assert_eq!(result, "C:\\");
    }

    #[test]
    fn test_normalize_path_unc() {
        let result = normalize_path("\\\\server\\share\\file.txt");
        assert_eq!(result, "\\\\server\\share\\file.txt");
    }
}
