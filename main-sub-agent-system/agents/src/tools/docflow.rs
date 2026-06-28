//! DocFlow 文档转换工具执行器
//!
//! 统一的文档转换工具，支持转换、启动服务、查询状态。
//! 自动检测并启动 DocFlow 服务。

use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};

use agent_teams_core::tool::{
    Tool, ToolBuilder, ToolCall, ToolExecutionContext, ToolExecutor, ToolResult, tool_error, tool_success,
};
use agent_teams_core::error::Result;

/// DocFlow 服务地址
const DOCFLOW_BASE_URL: &str = "http://localhost:5000";

/// DocFlow 服务启动状态
static DOCFLOW_STARTED: AtomicBool = AtomicBool::new(false);

/// DocFlow 文档转换工具
pub struct DocFlowTool {
    client: reqwest::Client,
}

impl DocFlowTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .connect_timeout(std::time::Duration::from_secs(5))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// 检查 DocFlow 服务是否运行
    async fn is_service_running(&self) -> bool {
        match self.client
            .get(format!("{}/api/engine", DOCFLOW_BASE_URL))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// 启动 DocFlow 服务
    async fn start_service(&self) -> Result<()> {
        if DOCFLOW_STARTED.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if self.is_service_running().await {
                return Ok(());
            }
            return Err(agent_teams_core::error::AgentTeamsError::NotFound(
                "DocFlow 服务启动失败，请手动启动".to_string()
            ));
        }

        tracing::info!("Starting DocFlow service...");
        let docflow_dir = self.find_docflow_dir()?;

        // Prefer packaged exe, fallback to python script
        let docflow_exe = docflow_dir.join("docflow_server.exe");
        let server_py = docflow_dir.join("server.py");
        let use_exe = docflow_exe.exists();

        if !use_exe && !server_py.exists() {
            return Err(agent_teams_core::error::AgentTeamsError::NotFound(
                format!("找不到 DocFlow 服务文件: {} 或 docflow_server.exe", server_py.display())
            ));
        }

        let spawn_result = {
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                let mut cmd = if use_exe {
                    let mut c = std::process::Command::new(&docflow_exe);
                    c.current_dir(&docflow_dir)
                        .creation_flags(0x08000000)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null());
                    c
                } else {
                    // Prefer embedded Python (bundled deployment)
                    let python_exe = {
                        let embedded = docflow_dir.join("python").join("python.exe");
                        if embedded.exists() {
                            embedded.to_string_lossy().to_string()
                        } else {
                            std::env::var("PYTHON_PATH").unwrap_or_else(|_| "python".to_string())
                        }
                    };
                    let mut c = std::process::Command::new(&python_exe);
                    c.arg("server.py")
                        .current_dir(&docflow_dir)
                        .creation_flags(0x08000000)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null());
                    c
                };
                cmd.spawn()
            }
            #[cfg(not(target_os = "windows"))]
            {
                let mut cmd = std::process::Command::new("python3");
                cmd.arg("server.py")
                    .current_dir(&docflow_dir)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                cmd.spawn()
            }
        };

        match spawn_result {
            Ok(_) => {
                tracing::info!("DocFlow service started (mode: {})", if use_exe { "exe" } else { "python" });
                DOCFLOW_STARTED.store(true, Ordering::Relaxed);
            }
            Err(e) => {
                return Err(agent_teams_core::error::AgentTeamsError::NotFound(
                    format!("启动 DocFlow 失败: {}", e)
                ));
            }
        }

        for i in 0..10 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if self.is_service_running().await {
                tracing::info!("DocFlow service ready after {}s", i + 1);
                return Ok(());
            }
        }

        Err(agent_teams_core::error::AgentTeamsError::NotFound(
            "DocFlow 服务启动超时".to_string()
        ))
    }

    fn find_docflow_dir(&self) -> Result<std::path::PathBuf> {
        let possible_paths = vec![
            std::path::PathBuf::from("tools/DocFlow"),
            std::path::PathBuf::from("../tools/DocFlow"),
            std::path::PathBuf::from("../../tools/DocFlow"),
            std::path::PathBuf::from("main-sub-agent-system/tools/DocFlow"),
        ];

        let cwd = std::env::current_dir().unwrap_or_default();

        for path in &possible_paths {
            let full_path = cwd.join(path);
            if full_path.exists() && (full_path.join("server.py").exists() || full_path.join("docflow_server.exe").exists()) {
                return Ok(full_path);
            }
        }

        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let docflow_path = exe_dir.join("tools/DocFlow");
                if docflow_path.exists() && (docflow_path.join("server.py").exists() || docflow_path.join("docflow_server.exe").exists()) {
                    return Ok(docflow_path);
                }
            }
        }

        Err(agent_teams_core::error::AgentTeamsError::NotFound(
            "找不到 DocFlow 目录，请确保 tools/DocFlow 目录存在".to_string()
        ))
    }

    async fn ensure_service(&self) -> Result<()> {
        if self.is_service_running().await {
            return Ok(());
        }
        self.start_service().await
    }

    async fn upload_file(&self, file_path: &str) -> Result<serde_json::Value> {
        let path = std::path::Path::new(file_path);
        if !path.exists() {
            return Err(agent_teams_core::error::AgentTeamsError::NotFound(
                format!("文件不存在: {}", file_path)
            ));
        }

        let file_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let file_bytes = tokio::fs::read(path).await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(
                format!("读取文件失败: {}", e)
            ))?;

        let file_part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.clone())
            .mime_str("application/octet-stream")
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(
                format!("创建文件部分失败: {}", e)
            ))?;

        let form = reqwest::multipart::Form::new()
            .part("files", file_part);

        let resp = self.client
            .post(format!("{}/api/upload", DOCFLOW_BASE_URL))
            .multipart(form)
            .send()
            .await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(
                format!("上传请求失败: {}", e)
            ))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(agent_teams_core::error::AgentTeamsError::NotFound(
                format!("上传失败 ({}): {}", status, text)
            ));
        }

        resp.json().await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(
                format!("解析上传响应失败: {}", e)
            ))
    }

    async fn start_conversion(&self, job_id: &str, conversion_type: &str, options: Option<serde_json::Value>) -> Result<serde_json::Value> {
        let mut body = json!({
            "conversion_type": conversion_type
        });

        if let Some(opts) = options {
            body["options"] = opts;
        }

        let resp = self.client
            .post(format!("{}/api/convert/{}", DOCFLOW_BASE_URL, job_id))
            .json(&body)
            .send()
            .await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(
                format!("转换请求失败: {}", e)
            ))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(agent_teams_core::error::AgentTeamsError::NotFound(
                format!("转换失败 ({}): {}", status, text)
            ));
        }

        resp.json().await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(
                format!("解析转换响应失败: {}", e)
            ))
    }

    async fn poll_status(&self, job_id: &str) -> Result<serde_json::Value> {
        let resp = self.client
            .get(format!("{}/api/status/{}", DOCFLOW_BASE_URL, job_id))
            .send()
            .await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(
                format!("状态查询失败: {}", e)
            ))?;

        resp.json().await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(
                format!("解析状态响应失败: {}", e)
            ))
    }

    async fn wait_for_completion(&self, job_id: &str) -> Result<serde_json::Value> {
        let max_attempts = 60;
        let mut attempts = 0;

        loop {
            if attempts >= max_attempts {
                return Err(agent_teams_core::error::AgentTeamsError::NotFound(
                    "转换超时".to_string()
                ));
            }

            let status = self.poll_status(job_id).await?;
            let status_str = status["status"].as_str().unwrap_or("unknown");

            match status_str {
                "done" => return Ok(status),
                "error" => {
                    let error_msg = status["error"].as_str().unwrap_or("未知错误");
                    return Err(agent_teams_core::error::AgentTeamsError::NotFound(
                        format!("转换失败: {}", error_msg)
                    ));
                }
                _ => {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    attempts += 1;
                }
            }
        }
    }

    async fn download_result(&self, job_id: &str, output_path: &str) -> Result<serde_json::Value> {
        let resp = self.client
            .get(format!("{}/api/download/{}", DOCFLOW_BASE_URL, job_id))
            .send()
            .await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(
                format!("下载请求失败: {}", e)
            ))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(agent_teams_core::error::AgentTeamsError::NotFound(
                format!("下载失败 ({}): {}", status, text)
            ));
        }

        let bytes = resp.bytes().await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(
                format!("读取下载内容失败: {}", e)
            ))?;

        if let Some(parent) = std::path::Path::new(output_path).parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(
                    format!("创建输出目录失败: {}", e)
                ))?;
        }

        tokio::fs::write(output_path, &bytes).await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::NotFound(
                format!("写入文件失败: {}", e)
            ))?;

        Ok(json!({
            "success": true,
            "output_path": output_path,
            "size": bytes.len()
        }))
    }

    /// 执行 convert 操作
    async fn execute_convert(&self, call: &ToolCall, ms_fn: impl Fn() -> u64) -> Result<ToolResult> {
        let input_path = match call.arguments["input_path"].as_str() {
            Some(p) => p,
            None => return Ok(tool_error(call,
                "缺少必需参数 'input_path'",
                "参数 'input_path' 未提供",
                "请提供输入文件路径",
                ms_fn(),
            )),
        };

        let conversion_type = match call.arguments["conversion_type"].as_str() {
            Some(t) => t,
            None => return Ok(tool_error(call,
                "缺少必需参数 'conversion_type'",
                "参数 'conversion_type' 未提供",
                "请提供转换类型：doc_to_pdf, pdf_to_docx, to_markdown",
                ms_fn(),
            )),
        };

        let output_path = call.arguments["output_path"].as_str().map(|s| s.to_string());

        // 构建质量选项（默认最高质量）
        let mut options = serde_json::Map::new();

        // image_dpi: 默认 600（最高质量）
        let image_dpi = call.arguments["image_dpi"].as_u64().unwrap_or(600);
        options.insert("imageDpi".to_string(), serde_json::json!(image_dpi));

        // lossless: 默认 true（无损模式）
        let lossless = call.arguments["lossless"].as_bool().unwrap_or(true);
        options.insert("losslessImages".to_string(), serde_json::json!(lossless));

        // page_size: 默认 A4
        if let Some(page_size) = call.arguments["page_size"].as_str() {
            options.insert("pageSize".to_string(), serde_json::json!(page_size));
        }

        // orientation: 默认 Portrait
        if let Some(orientation) = call.arguments["orientation"].as_str() {
            options.insert("orientation".to_string(), serde_json::json!(orientation));
        }

        // embed_fonts: 默认 true
        let embed_fonts = call.arguments["embed_fonts"].as_bool().unwrap_or(true);
        options.insert("embedFonts".to_string(), serde_json::json!(embed_fonts));

        let options_value = if options.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(options))
        };

        // 确保服务可用
        if let Err(e) = self.ensure_service().await {
            return Ok(tool_error(call,
                format!("DocFlow 服务不可用: {}", e),
                "无法启动或连接到 DocFlow 服务",
                "请检查 Python 环境和 DocFlow 依赖是否安装",
                ms_fn(),
            ));
        }

        // 上传文件
        let upload_result = match self.upload_file(input_path).await {
            Ok(r) => r,
            Err(e) => return Ok(tool_error(call,
                format!("上传文件失败: {}", e),
                format!("路径: {}", input_path),
                "请检查文件路径是否正确",
                ms_fn(),
            )),
        };

        // 解析 job_id
        let job_id = if let Some(jobs) = upload_result["jobs"].as_array() {
            jobs.first().and_then(|j| j["id"].as_str()).unwrap_or("")
        } else {
            upload_result["id"].as_str().unwrap_or("")
        };

        if job_id.is_empty() {
            return Ok(tool_error(call,
                "上传响应缺少 job_id",
                format!("响应: {:?}", upload_result),
                "请检查 DocFlow 服务状态",
                ms_fn(),
            ));
        }

        // 开始转换
        if let Err(e) = self.start_conversion(job_id, conversion_type, options_value).await {
            return Ok(tool_error(call,
                format!("启动转换失败: {}", e),
                format!("job_id: {}, type: {}", job_id, conversion_type),
                "请检查转换类型是否正确",
                ms_fn(),
            ));
        }

        // 等待完成
        let status = match self.wait_for_completion(job_id).await {
            Ok(s) => s,
            Err(e) => return Ok(tool_error(call,
                format!("转换过程失败: {}", e),
                format!("job_id: {}", job_id),
                "请检查 DocFlow 服务日志",
                ms_fn(),
            )),
        };

        // 下载结果
        let default_output = {
            let stem = if let Some(pos) = input_path.rfind('.') {
                &input_path[..pos]
            } else {
                input_path
            };
            format!("{}.{}",
                stem,
                match conversion_type {
                    "doc_to_pdf" => "pdf",
                    "pdf_to_docx" => "docx",
                    "to_markdown" => "md",
                    _ => "out",
                }
            )
        };
        let final_output_path = output_path.as_deref().unwrap_or(&default_output);

        match self.download_result(job_id, final_output_path).await {
            Ok(_) => Ok(tool_success(call, json!({
                "success": true,
                "job_id": job_id,
                "input_path": input_path,
                "output_path": final_output_path,
                "conversion_type": conversion_type,
                "pages": status["pages"].as_u64().unwrap_or(0),
            }), ms_fn())),
            Err(e) => Ok(tool_error(call,
                format!("下载结果失败: {}", e),
                format!("job_id: {}", job_id),
                "文件可能已转换但下载失败，请手动从 DocFlow 下载",
                ms_fn(),
            )),
        }
    }

    /// 执行 start 操作
    async fn execute_start(&self, call: &ToolCall, ms_fn: impl Fn() -> u64) -> Result<ToolResult> {
        if self.is_service_running().await {
            return Ok(tool_success(call, json!({
                "status": "already_running",
                "url": DOCFLOW_BASE_URL,
                "message": "DocFlow 服务已在运行"
            }), ms_fn()));
        }

        match self.start_service().await {
            Ok(_) => Ok(tool_success(call, json!({
                "status": "started",
                "url": DOCFLOW_BASE_URL,
                "message": "DocFlow 服务已启动"
            }), ms_fn())),
            Err(e) => Ok(tool_error(call,
                format!("启动 DocFlow 失败: {}", e),
                "无法启动 DocFlow 服务",
                "请检查 Python 环境和依赖是否安装",
                ms_fn(),
            )),
        }
    }

    /// 执行 status 操作
    async fn execute_status(&self, call: &ToolCall, ms_fn: impl Fn() -> u64) -> Result<ToolResult> {
        let job_id = match call.arguments["job_id"].as_str() {
            Some(id) => id,
            None => return Ok(tool_error(call,
                "缺少必需参数 'job_id'",
                "参数 'job_id' 未提供",
                "请提供任务 ID",
                ms_fn(),
            )),
        };

        match self.poll_status(job_id).await {
            Ok(result) => Ok(tool_success(call, result, ms_fn())),
            Err(e) => Ok(tool_error(call,
                format!("查询状态失败: {}", e),
                format!("job_id: {}", job_id),
                "请检查 job_id 是否正确",
                ms_fn(),
            )),
        }
    }
}

impl Default for DocFlowTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolExecutor for DocFlowTool {
    fn executor_id(&self) -> &str {
        "docflow"
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            ToolBuilder::new("docflow")
                .description(concat!(
                    "统一文档转换工具。支持：\n",
                    "- DOC/DOCX → PDF（需要安装 Microsoft Word）\n",
                    "- PDF → DOCX\n",
                    "- PDF/DOC/DOCX → Markdown\n\n",
                    "适用场景：\n",
                    "  - 需要将文档转换为另一种格式\n",
                    "  - 需要将 PDF 转为可编辑的 DOCX\n",
                    "  - 需要将文档转为 Markdown 以便分析\n\n",
                    "工具联动：\n",
                    "  - 读取文档内容 → 使用 docreader\n",
                    "  - 转换文档格式 → 使用 docflow\n",
                    "  - 操作普通文本文件 → 使用 file\n",
                    "  - 转为 Markdown 后可用 file(read) 读取\n",
                    "  - http_request/http_get 下载的文档可直接传入 input_path\n",
                    "  - 转换后的文件可用 file(copy/move) 移动到目标位置\n\n",
                    "action 参数：\n",
                    "  convert — 转换文档格式（自动启动服务、上传、转换、下载）\n",
                    "  start — 启动 DocFlow 服务\n",
                    "  status — 查询转换任务状态\n\n",
                    "质量参数（可选，不填则默认最高质量）：\n",
                    "  image_dpi — 图片DPI（150/300/600），默认600\n",
                    "  lossless — 无损图片模式，默认true\n",
                    "  page_size — 页面大小（A4/Letter/Legal），默认A4\n",
                    "  orientation — 方向（Portrait/Landscape），默认Portrait\n",
                    "  embed_fonts — 嵌入字体，默认true\n\n",
                    "如果服务未运行，convert 会自动启动。"
                ))
                .executor("docflow")
                .tag("document")
                .tag("conversion")
                .timeout(300_000)
                .data_flow("convert: 接受文件路径，输出转换后的文件路径")
                .output_field("output_path: 转换后的文件路径 (convert)")
                .output_field("job_id: 任务 ID (convert/status)")
                .output_field("status: 服务状态 (start) 或任务状态 (status)")
                .param_enum("action", "操作类型", &["convert", "start", "status"], true)
                .param_string("input_path", "输入文件路径（convert 时必填）", false)
                .param_enum("conversion_type", "转换类型（convert 时必填）", &["doc_to_pdf", "pdf_to_docx", "to_markdown"], false)
                .param_string("output_path", "输出文件路径（convert 时可选）", false)
                .param_string("job_id", "任务 ID（status 时必填）", false)
                .param_integer("image_dpi", "图片DPI：150/300/600，默认600（最高质量）", false)
                .param_bool("lossless", "无损图片模式，默认true", false)
                .param_enum("page_size", "页面大小", &["A4", "Letter", "Legal"], false)
                .param_enum("orientation", "页面方向", &["Portrait", "Landscape"], false)
                .param_bool("embed_fonts", "嵌入字体，默认true", false)
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

        let action = match call.arguments["action"].as_str() {
            Some(a) => a,
            None => return Ok(tool_error(call,
                "缺少必需参数 'action'",
                "参数 'action' 未提供",
                "请提供操作类型：convert, start, status",
                ms_fn(),
            )),
        };

        match action {
            "convert" => self.execute_convert(call, ms_fn).await,
            "start" => self.execute_start(call, ms_fn).await,
            "status" => self.execute_status(call, ms_fn).await,
            _ => Ok(tool_error(call,
                format!("未知操作 '{}'", action),
                format!("action='{}' 不是有效的操作类型", action),
                "请使用：convert, start, status",
                ms_fn(),
            )),
        }
    }
}
