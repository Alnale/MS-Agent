//! DocReader 文档读取工具执行器
//!
//! 读取 PDF/DOCX/DOC 文件的文本内容，供模型理解文档。
//! 自动检测并启动 DocReader 服务。

use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};

use agent_core::tool::{
    Tool, ToolBuilder, ToolCall, ToolExecutionContext, ToolExecutor, ToolResult, tool_error, tool_success,
};
use agent_core::error::Result;

/// DocReader 服务地址
const DOCREADER_BASE_URL: &str = "http://localhost:5002";

/// DocReader 服务启动状态
static DOCREADER_STARTED: AtomicBool = AtomicBool::new(false);

/// DocReader 文档读取工具
pub struct DocReaderTool {
    client: reqwest::Client,
}

impl Default for DocReaderTool {
    fn default() -> Self {
        Self::new()
    }
}

impl DocReaderTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .connect_timeout(std::time::Duration::from_secs(3))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    async fn is_service_running(&self) -> bool {
        match self.client
            .get(format!("{}/health", DOCREADER_BASE_URL))
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    async fn start_service(&self) -> Result<()> {
        if DOCREADER_STARTED.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if self.is_service_running().await {
                return Ok(());
            }
            return Err(agent_core::error::AgentTeamsError::NotFound(
                "DocReader 服务启动失败".to_string()
            ));
        }

        tracing::info!("Starting DocReader service...");
        let reader_dir = self.find_reader_dir()?;

        let server_py = reader_dir.join("server.py");
        if !server_py.exists() {
            return Err(agent_core::error::AgentTeamsError::NotFound(
                format!("找不到 DocReader 服务文件: {}", server_py.display())
            ));
        }

        let python_exe = {
            let embedded = reader_dir.join("python").join("python.exe");
            if embedded.exists() {
                embedded.to_string_lossy().to_string()
            } else {
                std::env::var("PYTHON_PATH").unwrap_or_else(|_| "python".to_string())
            }
        };

        let spawn_result = {
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                let mut c = std::process::Command::new(&python_exe);
                c.arg("server.py")
                    .current_dir(&reader_dir)
                    .env("DOCREADER_PORT", "5002")
                    .creation_flags(0x08000000)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                c.spawn()
            }
            #[cfg(not(target_os = "windows"))]
            {
                let mut c = std::process::Command::new("python3");
                c.arg("server.py")
                    .current_dir(&reader_dir)
                    .env("DOCREADER_PORT", "5002")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                c.spawn()
            }
        };

        match spawn_result {
            Ok(_) => {
                tracing::info!("DocReader service started");
                DOCREADER_STARTED.store(true, Ordering::Relaxed);
            }
            Err(e) => {
                return Err(agent_core::error::AgentTeamsError::NotFound(
                    format!("启动 DocReader 失败: {}", e)
                ));
            }
        }

        for i in 0..8 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if self.is_service_running().await {
                tracing::info!("DocReader service ready after {}s", i + 1);
                return Ok(());
            }
        }

        Err(agent_core::error::AgentTeamsError::NotFound(
            "DocReader 服务启动超时".to_string()
        ))
    }

    fn find_reader_dir(&self) -> Result<std::path::PathBuf> {
        let possible_paths = vec![
            std::path::PathBuf::from("tools/DocReader"),
            std::path::PathBuf::from("../tools/DocReader"),
            std::path::PathBuf::from("../../tools/DocReader"),
        ];
        for p in &possible_paths {
            if p.join("server.py").exists() {
                return Ok(p.canonicalize().unwrap_or_else(|_| p.clone()));
            }
        }
        Err(agent_core::error::AgentTeamsError::NotFound(
            "找不到 DocReader 服务目录 (tools/DocReader)".to_string()
        ))
    }

    async fn execute_read(&self, call: &ToolCall, ms_fn: impl Fn() -> u64) -> Result<ToolResult> {
        let input_path = match call.arguments["input_path"].as_str() {
            Some(p) => p,
            None => return Ok(tool_error(call,
                "缺少必需参数 'input_path'",
                "参数 'input_path' 未提供",
                "请提供输入文件路径",
                ms_fn(),
            )),
        };

        let pages = call.arguments["pages"].as_str().map(|s| s.to_string());

        // 确保服务运行
        if !self.is_service_running().await {
            if let Err(e) = self.start_service().await {
                return Ok(tool_error(call,
                    format!("DocReader 服务不可用: {}", e),
                    "无法启动 DocReader 服务",
                    "请确保 Python 和 pymupdf/python-docx 已安装",
                    ms_fn(),
                ));
            }
        }

        // 验证文件存在
        let path = std::path::Path::new(input_path);
        if !path.exists() {
            return Ok(tool_error(call,
                format!("文件不存在: {}", input_path),
                format!("路径 '{}' 不存在或无法访问", input_path),
                "请检查文件路径是否正确",
                ms_fn(),
            ));
        }

        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !["pdf", "docx", "doc"].contains(&ext.as_str()) {
            return Ok(tool_error(call,
                format!("不支持的文件格式: .{}", ext),
                format!("文件扩展名 '.{}' 不在支持列表中", ext),
                "支持的格式: PDF、DOCX、DOC",
                ms_fn(),
            ));
        }

        // 获取绝对路径
        let abs_path = path.canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_string();

        // 构建请求体
        let mut body = json!({ "path": abs_path });
        if let Some(ref p) = pages {
            if !p.trim().is_empty() {
                body["pages"] = json!(p);
            }
        }

        // 调用服务
        let resp = match self.client
            .post(format!("{}/read", DOCREADER_BASE_URL))
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return Ok(tool_error(call,
                format!("请求 DocReader 服务失败: {}", e),
                "HTTP 请求失败",
                "请检查 DocReader 服务是否正常运行",
                ms_fn(),
            )),
        };

        if !resp.status().is_success() {
            let err_text = resp.text().await.unwrap_or_else(|_| "未知错误".to_string());
            return Ok(tool_error(call,
                format!("DocReader 服务返回错误: {}", err_text),
                format!("HTTP {}", err_text),
                "请检查文件格式是否正确",
                ms_fn(),
            ));
        }

        let result: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => return Ok(tool_error(call,
                format!("解析 DocReader 响应失败: {}", e),
                "JSON 解析失败",
                "请检查 DocReader 服务版本",
                ms_fn(),
            )),
        };

        if let Some(err) = result.get("error") {
            return Ok(tool_error(call,
                err.as_str().unwrap_or("读取失败"),
                "文档读取失败",
                result.get("hint").and_then(|v| v.as_str()).unwrap_or("请检查文件格式"),
                ms_fn(),
            ));
        }

        // 构建输出
        let text = result.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let file_name = result.get("file").and_then(|v| v.as_str()).unwrap_or("");
        let format = result.get("format").and_then(|v| v.as_str()).unwrap_or("");

        let mut summary = format!("📄 已读取: {} ({})", file_name, format.to_uppercase());

        if format == "pdf" {
            if let Some(total) = result.get("total_pages").and_then(|v| v.as_i64()) {
                summary.push_str(&format!(" | 总页数: {}", total));
            }
            if let Some(extracted) = result.get("extracted_pages").and_then(|v| v.as_i64()) {
                summary.push_str(&format!(" | 提取: {}", extracted));
            }
            if let Some(tables) = result.get("tables").and_then(|v| v.as_i64()) {
                if tables > 0 {
                    summary.push_str(&format!(" | 表格: {}", tables));
                }
            }
            if let Some(images) = result.get("images").and_then(|v| v.as_i64()) {
                if images > 0 {
                    summary.push_str(&format!(" | 图片: {}", images));
                }
            }
            if let Some(links) = result.get("links").and_then(|v| v.as_i64()) {
                if links > 0 {
                    summary.push_str(&format!(" | 链接: {}", links));
                }
            }
            if let Some(annotations) = result.get("annotations").and_then(|v| v.as_i64()) {
                if annotations > 0 {
                    summary.push_str(&format!(" | 注释: {}", annotations));
                }
            }
        } else if format == "docx" {
            if let Some(paras) = result.get("paragraphs").and_then(|v| v.as_i64()) {
                summary.push_str(&format!(" | 段落: {}", paras));
            }
            if let Some(headings) = result.get("headings").and_then(|v| v.as_i64()) {
                if headings > 0 {
                    summary.push_str(&format!(" | 标题: {}", headings));
                }
            }
            if let Some(lists) = result.get("lists").and_then(|v| v.as_i64()) {
                if lists > 0 {
                    summary.push_str(&format!(" | 列表项: {}", lists));
                }
            }
            if let Some(tables) = result.get("tables").and_then(|v| v.as_i64()) {
                if tables > 0 {
                    summary.push_str(&format!(" | 表格: {}", tables));
                }
            }
            if let Some(images) = result.get("images").and_then(|v| v.as_i64()) {
                if images > 0 {
                    summary.push_str(&format!(" | 图片: {}", images));
                }
            }
            if let Some(hyperlinks) = result.get("hyperlinks").and_then(|v| v.as_array()) {
                if !hyperlinks.is_empty() {
                    summary.push_str(&format!(" | 超链接: {}", hyperlinks.len()));
                }
            }
            if let Some(footnotes) = result.get("footnotes").and_then(|v| v.as_array()) {
                if !footnotes.is_empty() {
                    summary.push_str(&format!(" | 脚注: {}", footnotes.len()));
                }
            }
            if let Some(endnotes) = result.get("endnotes").and_then(|v| v.as_array()) {
                if !endnotes.is_empty() {
                    summary.push_str(&format!(" | 尾注: {}", endnotes.len()));
                }
            }
            if let Some(comments) = result.get("comments").and_then(|v| v.as_array()) {
                if !comments.is_empty() {
                    summary.push_str(&format!(" | 评论: {}", comments.len()));
                }
            }
        }

        // Append document metadata (title, author, dates)
        if let Some(meta) = result.get("metadata").and_then(|v| v.as_object()) {
            let mut meta_parts = Vec::new();
            for (key, val) in meta {
                if let Some(s) = val.as_str() {
                    let label = match key.as_str() {
                        "title" => Some("标题"),
                        "author" => Some("作者"),
                        "subject" => Some("主题"),
                        "created" => Some("创建时间"),
                        "modified" => Some("修改时间"),
                        "creationDate" => Some("创建时间"),
                        "modDate" => Some("修改时间"),
                        _ => None,
                    };
                    if let Some(l) = label {
                        meta_parts.push(format!("{}: {}", l, s));
                    }
                }
            }
            if !meta_parts.is_empty() {
                summary.push_str(&format!("\n📝 {}", meta_parts.join(" | ")));
            }
        }

        // Append bookmarks if available
        if let Some(bookmarks) = result.get("bookmarks").and_then(|v| v.as_array()) {
            if !bookmarks.is_empty() {
                summary.push_str("\n📑 书签/目录:");
                for (i, bm) in bookmarks.iter().take(20).enumerate() {
                    if let (Some(title), Some(page), Some(level)) = (
                        bm.get("title").and_then(|v| v.as_str()),
                        bm.get("page").and_then(|v| v.as_i64()),
                        bm.get("level").and_then(|v| v.as_i64()),
                    ) {
                        let indent = "  ".repeat(level.saturating_sub(1) as usize);
                        summary.push_str(&format!("\n{}{}. {} (页{})", indent, i + 1, title, page));
                    }
                }
                if bookmarks.len() > 20 {
                    summary.push_str(&format!("\n... 还有 {} 个书签", bookmarks.len() - 20));
                }
            }
        }

        // Append hyperlinks if available
        if let Some(hyperlinks) = result.get("hyperlinks").and_then(|v| v.as_array()) {
            if !hyperlinks.is_empty() {
                summary.push_str("\n🔗 超链接:");
                for (i, link) in hyperlinks.iter().take(10).enumerate() {
                    if let Some(target) = link.get("target").and_then(|v| v.as_str()) {
                        summary.push_str(&format!("\n{}. {}", i + 1, target));
                    }
                }
                if hyperlinks.len() > 10 {
                    summary.push_str(&format!("\n... 还有 {} 个链接", hyperlinks.len() - 10));
                }
            }
        }

        // Append footnotes if available
        if let Some(footnotes) = result.get("footnotes").and_then(|v| v.as_array()) {
            if !footnotes.is_empty() {
                summary.push_str("\n📎 脚注:");
                for (i, fn_) in footnotes.iter().take(5).enumerate() {
                    if let Some(text) = fn_.get("text").and_then(|v| v.as_str()) {
                        let display_text = if text.len() > 100 {
                            format!("{}...", &text[..100])
                        } else {
                            text.to_string()
                        };
                        summary.push_str(&format!("\n{}. {}", i + 1, display_text));
                    }
                }
                if footnotes.len() > 5 {
                    summary.push_str(&format!("\n... 还有 {} 个脚注", footnotes.len() - 5));
                }
            }
        }

        // Append endnotes if available
        if let Some(endnotes) = result.get("endnotes").and_then(|v| v.as_array()) {
            if !endnotes.is_empty() {
                summary.push_str("\n📝 尾注:");
                for (i, en) in endnotes.iter().take(5).enumerate() {
                    if let Some(text) = en.get("text").and_then(|v| v.as_str()) {
                        let display_text = if text.len() > 100 {
                            format!("{}...", &text[..100])
                        } else {
                            text.to_string()
                        };
                        summary.push_str(&format!("\n{}. {}", i + 1, display_text));
                    }
                }
                if endnotes.len() > 5 {
                    summary.push_str(&format!("\n... 还有 {} 个尾注", endnotes.len() - 5));
                }
            }
        }

        // Append comments if available
        if let Some(comments) = result.get("comments").and_then(|v| v.as_array()) {
            if !comments.is_empty() {
                summary.push_str("\n💬 评论:");
                for (i, cm) in comments.iter().take(5).enumerate() {
                    if let (Some(author), Some(text)) = (
                        cm.get("author").and_then(|v| v.as_str()),
                        cm.get("text").and_then(|v| v.as_str()),
                    ) {
                        let display_text = if text.len() > 100 {
                            format!("{}...", &text[..100])
                        } else {
                            text.to_string()
                        };
                        summary.push_str(&format!("\n{}. {}: {}", i + 1, author, display_text));
                    }
                }
                if comments.len() > 5 {
                    summary.push_str(&format!("\n... 还有 {} 条评论", comments.len() - 5));
                }
            }
        }

        let char_count = text.chars().count();
        summary.push_str(&format!("\n字符: {}", char_count));

        // 截断过长文本
        const MAX_CHARS: usize = 50_000;
        let display_text = if char_count > MAX_CHARS {
            let truncated: String = text.chars().take(MAX_CHARS).collect();
            format!("{}\n\n... [内容过长，已截断，共 {} 字符]", truncated, char_count)
        } else {
            text.to_string()
        };

        let output = format!("{}\n\n── 文本内容 ──\n{}", summary, display_text);
        Ok(tool_success(call, json!(output), ms_fn()))
    }
}

#[async_trait]
impl ToolExecutor for DocReaderTool {
    fn executor_id(&self) -> &str {
        "docreader"
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            ToolBuilder::new("docreader")
                .description(concat!(
                    "文档读取工具。读取 PDF/DOCX/DOC 文件的文本内容，供模型理解文档。\n",
                    "这是读取文档文件的首选工具，不要用 file(read) 读取文档文件。\n\n",
                    "适用场景：\n",
                    "  - 读取 PDF 文件内容\n",
                    "  - 读取 Word 文档（DOCX/DOC）内容\n",
                    "  - 提取文档中的文本、表格、图片描述、超链接等\n\n",
                    "支持提取：文本、表格、图片、超链接、书签、注释、脚注、尾注、评论等。\n",
                    "参数：\n",
                    "  input_path — 文件路径（必填）\n",
                    "  pages — 页码范围（仅 PDF 有效，可选，如 \"1-3,5,8-\"）\n\n",
                    "工具联动：\n",
                    "- 读取的内容可用 file(write) 保存为文本文件\n",
                    "- 如果需要转换格式再读取，先用 docflow 转为 Markdown\n",
                    "- 读取的题目内容可用于 xxt 的答案生成\n\n",
                    "自动启动读取服务。"
                ))
                .executor("docreader")
                .tag("document")
                .tag("reading")
                .timeout(60_000)
                .data_flow("接受文件路径，输出文本内容及结构化信息")
                .output_field("text: 提取的文本内容")
                .output_field("bookmarks: PDF书签/目录结构")
                .output_field("hyperlinks: 超链接列表")
                .output_field("annotations: PDF注释/批注")
                .output_field("footnotes: DOCX脚注")
                .output_field("endnotes: DOCX尾注")
                .output_field("comments: DOCX评论/批注")
                .param_string("input_path", "文件路径（必填，支持 PDF/DOCX/DOC）", true)
                .param_string("pages", "页码范围（仅 PDF，可选，如 1-3,5,8-）", false)
                .build(),
        ]
    }

    async fn execute(
        &self,
        call: &ToolCall,
        _ctx: &ToolExecutionContext,
    ) -> Result<ToolResult> {
        let start = std::time::Instant::now();
        let ms_fn = || start.elapsed().as_millis() as u64;

        match call.name.as_str() {
            "docreader" => self.execute_read(call, ms_fn).await,
            _ => Ok(tool_error(call,
                format!("未知工具: {}", call.name),
                "工具名不匹配",
                "请使用 docreader 工具",
                ms_fn(),
            )),
        }
    }
}
