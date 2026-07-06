//! 超星学习通自动答题工具执行器
//!
//! 封装 Python xxt 工具，将其集成到 Rust 工具系统中。
//! 支持 Agent 上下文注入——自动将 working_memory、system_instructions 等
//! 通过环境变量传递给 Python 进程。

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value;

use agent_core::tool::{
    Tool, ToolBuilder, ToolCall, ToolExecutionContext, ToolExecutor, ToolResult, tool_error, tool_success,
};
use agent_core::error::Result;

/// 获取 xxt 工具目录路径
fn xxt_dir() -> PathBuf {
    // 优先使用环境变量
    if let Ok(dir) = std::env::var("XXT_TOOL_DIR") {
        return PathBuf::from(dir);
    }
    // 默认: 从 crate 根目录向上找 tools/xxt
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(|p| p.join("tools").join("xxt"))
        .unwrap_or_else(|| manifest_dir.join("tools").join("xxt"))
}

/// 获取 Python 解释器路径
fn python_exe() -> String {
    // 优先使用嵌入式 Python（打包部署）
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let embedded = exe_dir.join("tools").join("xxt").join("python").join("python.exe");
            if embedded.exists() {
                return embedded.to_string_lossy().to_string();
            }
        }
    }
    // 其次使用环境变量
    std::env::var("XXT_PYTHON").unwrap_or_else(|_| "python".to_string())
}

/// 超星学习通工具执行器
pub struct XxtToolExecutor {
    xxt_dir: PathBuf,
    python: String,
}

impl XxtToolExecutor {
    pub fn new() -> Self {
        Self {
            xxt_dir: xxt_dir(),
            python: python_exe(),
        }
    }

    /// 从 agent context 构建环境变量注入
    ///
    /// 将 working_memory、system_instructions、recent_history 等
    /// 序列化为临时 JSON 文件，通过环境变量传递文件路径给 Python 进程。
    /// 避免超过 OS 环境变量大小限制（Windows 32KB, Linux ~128KB per var）。
    fn build_context_env(ctx: &ToolExecutionContext) -> Vec<(String, String)> {
        let mut env_vars = Vec::new();

        // Session info (small, safe as env vars)
        env_vars.push(("XXT_SESSION_ID".to_string(), ctx.session_id.clone()));
        if let Some(ref uid) = ctx.user_id {
            env_vars.push(("XXT_USER_ID".to_string(), uid.clone()));
        }
        env_vars.push(("XXT_AGENT_ID".to_string(), ctx.agent_id.clone()));

        // Large context data — write to a temp file and pass the path
        if let Some(ref agent_ctx) = ctx.agent_context {
            let context_data = serde_json::json!({
                "working_memory": &*agent_ctx.working_memory,
                "system_instructions": &*agent_ctx.system_instructions,
                "domain_state": &*agent_ctx.domain_state,
                "entity": &agent_ctx.entity,
                "recent_history": agent_ctx.recent_history.iter().rev().take(10).collect::<Vec<_>>(),
                "memory_prompt": agent_ctx.build_memory_prompt(),
            });

            if let Ok(json) = serde_json::to_string(&context_data) {
                let context_file = tempfile::Builder::new()
                    .prefix(&format!("xxt_ctx_{}_", ctx.session_id))
                    .suffix(".json")
                    .tempfile();
                match context_file.and_then(|f| std::fs::write(f.path(), &json).map_err(std::io::Error::other).and(Ok(f))) {
                    Ok(file) => {
                        // Keep the file alive; Python will read it via the path
                        if let Ok((_, path)) = file.keep() {
                            env_vars.push(("XXT_CONTEXT_FILE".to_string(), path.to_string_lossy().to_string()));
                        }
                    }
                    Err(_) => {
                        tracing::warn!("Failed to write context file, falling back to env vars");
                        if let Ok(mem) = serde_json::to_string(&*agent_ctx.working_memory) {
                            env_vars.push(("XXT_WORKING_MEMORY".to_string(), mem));
                        }
                    }
                }
            }
        }

        // Tool history (current turn) — also via temp file
        if !ctx.tool_history.is_empty() {
            let history: Vec<Value> = ctx
                .tool_history
                .iter()
                .map(|(call, result)| {
                    serde_json::json!({
                        "tool": call.name,
                        "success": result.success,
                        "output": result.output,
                    })
                })
                .collect();
            if let Ok(json) = serde_json::to_string(&history) {
                let history_file = tempfile::Builder::new()
                    .prefix(&format!("xxt_hist_{}_", ctx.session_id))
                    .suffix(".json")
                    .tempfile();
                if let Ok(file) = history_file {
                    if std::fs::write(file.path(), &json).is_ok() {
                        if let Ok((_, path)) = file.keep() {
                            env_vars.push(("XXT_TOOL_HISTORY_FILE".to_string(), path.to_string_lossy().to_string()));
                        }
                    }
                }
            }
        }

        env_vars
    }

    /// 执行 xxt Python 脚本
    async fn execute_xxt(
        &self,
        subcommand: &str,
        url: &str,
        args: &[(String, String)],
        ctx: &ToolExecutionContext,
    ) -> Result<Value> {
        // Prefer packaged exe, fallback to python script
        let exe_path = self.xxt_dir.join("auto_answer.exe");
        let script = self.xxt_dir.join("auto_answer.py");

        let use_exe = exe_path.exists();

        if !use_exe && !script.exists() {
            tracing::error!(tool = "xxt", script = %script.display(), "xxt script not found");
            return Ok(serde_json::json!({
                "success": false,
                "error": format!("xxt 脚本不存在: {}", script.display()),
                "details": "auto_answer.py 或 auto_answer.exe 未找到",
                "suggestion": "请检查 tools/xxt/ 目录是否存在，或设置 XXT_TOOL_DIR 环境变量指向正确路径",
            }));
        }

        let mut cmd = if use_exe {
            let mut c = tokio::process::Command::new(&exe_path);
            c.arg(subcommand)
                .arg("--url")
                .arg(url)
                .current_dir(&self.xxt_dir);
            c
        } else {
            let mut c = tokio::process::Command::new(&self.python);
            c.arg(&script)
                .arg(subcommand)
                .arg("--url")
                .arg(url)
                .current_dir(&self.xxt_dir);
            c
        };

        // Inject agent context as environment variables
        for (key, value) in Self::build_context_env(ctx) {
            cmd.env(&key, &value);
        }

        // Add extra arguments
        // answers 通过环境变量传递（避免命令行 JSON 转义问题）
        for (key, value) in args {
            match key.as_str() {
                "answers" => {
                    cmd.env("XXT_ANSWERS", value);
                }
                "answers_file" => {
                    cmd.arg("--answers-file").arg(value);
                }
                "output" => {
                    cmd.arg("--output").arg(value);
                }
                "headless" => {
                    if value == "true" {
                        cmd.arg("--headless");
                    }
                }
                _ => {}
            }
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| agent_core::error::AgentTeamsError::Provider(
                format!("Failed to execute xxt: {}", e)
            ))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            return Ok(serde_json::json!({
                "success": false,
                "error": format!("xxt exited with code {:?}: {}", output.status.code(), stderr),
                "stdout": stdout,
            }));
        }

        // Try to parse stdout as JSON
        let result: Value = serde_json::from_str(stdout.trim())
            .unwrap_or_else(|_| serde_json::json!({
                "success": true,
                "output": stdout.trim(),
            }));

        Ok(result)
    }
}

impl Default for XxtToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolExecutor for XxtToolExecutor {
    fn executor_id(&self) -> &str {
        "xxt"
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            ToolBuilder::new("xxt")
                .description(concat!(
                    "超星学习通自动答题工具。\n",
                    "完整流程：\n",
                    "1. crawl 获取题目（返回 questions 和 answers_template）\n",
                    "2. 靠模型自身知识生成答案（优先）\n",
                    "3. fill 传入 answers（直接传JSON，无需保存文件）\n",
                    "4. check 确认 → submit 提交\n",
                    "fill 的 answers 格式：{\"1\":\"A\",\"2\":\"C\",\"3\":\"答案文本\"}\n\n",
                    "答案生成策略：\n",
                    "- 优先用模型自身知识，大多数题目足以应对\n",
                    "- 仅当涉及冷门专业知识/最新数据时才搜索\n",
                    "- 不要每道题都搜索，会增加耗时和失败率\n\n",
                    "工具联动：\n",
                    "- crawl 获取的题目可用 file(write) 保存备份\n",
                    "- 可用 docreader 读取参考资料辅助生成答案\n",
                    "- 答案量大时可用 file(write) 写入文件，再用 answers_file 参数传入"
                ))
                .executor("xxt")
                .tag("browser")
                .tag("education")
                .tag("automation")
                .timeout(40_000)
                .param_enum(
                    "subcommand",
                    "操作类型：login=登录, crawl=爬取题目（返回answers_template）, fill=填充答案（直接传answers）, submit=提交, screenshot=截图, check=检查填充状态",
                    &["login", "crawl", "fill", "submit", "screenshot", "check"],
                    true,
                )
                .param_string("url", "作业页面URL", true)
                .param_string("answers", "答案JSON，crawl会返回answers_template，填入答案后直接传。如: {\"1\":\"A\",\"2\":\"B\",\"3\":\"答案文本\"}", false)
                .param_string("answers_file", "答案文件路径（仅当答案量极大时使用，一般用answers参数即可）", false)
                .param_string("output", "截图保存文件名，仅screenshot命令可选", false)
                .param_bool("headless", "是否使用无头模式（不显示浏览器窗口），默认false", false)
                .data_flow("crawl: 输入URL，输出题目列表和 answers_template JSON")
                .data_flow("fill: 输入 answers JSON 字符串（如 {\"1\":\"A\",\"2\":\"B\"}），无文件依赖")
                .data_flow("当答案量极大时，可用 answers_file 参数传入文件路径，先用 file(write) 写入答案文件")
                .output_field("questions: 题目列表（crawl 操作）")
                .output_field("answers_template: 答案模板JSON（crawl 操作）")
                .output_field("success: 操作是否成功")
                .output_field("message: 操作结果消息")
                .build(),
        ]
    }

    async fn execute(&self, call: &ToolCall, ctx: &ToolExecutionContext) -> Result<ToolResult> {
        let start = std::time::Instant::now();

        let subcommand = match call.arguments["subcommand"].as_str() {
            Some(s) => s,
            None => {
                let ms = start.elapsed().as_millis() as u64;
                return Ok(tool_error(call,
                    "缺少必需参数 'subcommand'",
                    "参数 'subcommand' 未提供或不是字符串",
                    "请提供操作类型：login（登录）、crawl（爬取题目）、fill（填充答案）、submit（提交）、screenshot（截图）、check（检查状态）",
                    ms,
                ));
            }
        };
        let url = match call.arguments["url"].as_str() {
            Some(u) => u,
            None => {
                let ms = start.elapsed().as_millis() as u64;
                return Ok(tool_error(call,
                    "缺少必需参数 'url'",
                    "参数 'url' 未提供或不是字符串",
                    "请提供作业页面的完整 URL",
                    ms,
                ));
            }
        };

        // Collect extra arguments
        let mut extra_args = Vec::new();
        if let Some(answers) = call.arguments.get("answers") {
            if let Some(s) = answers.as_str() {
                extra_args.push(("answers".to_string(), s.to_string()));
            }
        }
        if let Some(answers_file) = call.arguments.get("answers_file") {
            if let Some(s) = answers_file.as_str() {
                extra_args.push(("answers_file".to_string(), s.to_string()));
            }
        }
        if let Some(output) = call.arguments.get("output") {
            if let Some(s) = output.as_str() {
                extra_args.push(("output".to_string(), s.to_string()));
            }
        }
        if let Some(headless) = call.arguments.get("headless") {
            if headless.as_bool().unwrap_or(false) {
                extra_args.push(("headless".to_string(), "true".to_string()));
            }
        }

        let result = self.execute_xxt(subcommand, url, &extra_args, ctx).await;
        let ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(output) => {
                let success = output.get("success").and_then(|v| v.as_bool()).unwrap_or(true);
                if success {
                    Ok(tool_success(call, output, ms))
                } else {
                    let error = output.get("error").and_then(|v| v.as_str()).unwrap_or("未知错误");
                    let message = output.get("message").and_then(|v| v.as_str()).unwrap_or("");
                    tracing::error!(tool = "xxt", subcommand = subcommand, error = error, "xxt reported failure");
                    Ok(tool_error(call,
                        format!("xxt {} 失败: {}", subcommand, error),
                        if message.is_empty() { error.to_string() } else { message.to_string() },
                        match subcommand {
                            "login" => "登录失败，请检查网络连接，或尝试手动在浏览器中登录",
                            "crawl" => "爬取失败，请检查 URL 是否为有效的作业页面，或先执行 login",
                            "fill" => "填充失败，请先 crawl 获取题目，再提供正确的 answers",
                            "submit" => "提交失败，请先 fill 填充答案",
                            "screenshot" => "截图失败，请检查页面是否已加载",
                            "check" => "检查失败，请先 crawl 获取题目",
                            _ => "请检查参数和页面状态",
                        },
                        ms,
                    ))
                }
            }
            Err(e) => {
                tracing::error!(tool = "xxt", subcommand = subcommand, error = %e, "xxt process execution failed");
                Ok(tool_error(call,
                    format!("xxt 进程执行失败: {}", e),
                    format!("无法启动 Python 进程: {}", e),
                    "请检查 Python 是否安装、xxt 脚本是否存在，或设置 XXT_PYTHON 环境变量",
                    ms,
                ))
            }
        }
    }
}
