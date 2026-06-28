use std::path::PathBuf;
use std::time::Instant;

use agent_teams_core::error::Result;
use agent_teams_core::tool::{
    Tool, ToolCall, ToolExecutionContext, ToolExecutor, ToolParameters, ToolResult,
};
use async_trait::async_trait;
use serde_json::Value;

/// Parse a natural language message into CLI args for the xxt tool.
/// Extracts subcommand and URL from Chinese text like "分析页面并将题目和答案保存在目录...下，URL为https://..."
pub fn parse_natural_language_to_args(message: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();

    // Determine subcommand from keywords
    let subcommand = if message.contains("爬取") || message.contains("分析") || message.contains("提取") || message.contains("crawl") {
        "crawl"
    } else if message.contains("填充") || message.contains("填写") || message.contains("fill") {
        "fill"
    } else if message.contains("提交") || message.contains("submit") {
        "submit"
    } else if message.contains("截图") || message.contains("screenshot") {
        "screenshot"
    } else if message.contains("检查") || message.contains("状态") || message.contains("check") {
        "check"
    } else if message.contains("登录") || message.contains("login") {
        "login"
    } else {
        "crawl"
    };

    args.push(subcommand.to_string());

    // Extract URL - look for http:// or https://
    if let Some(url_start) = message.find("http://").or_else(|| message.find("https://")) {
        let url = &message[url_start..];
        // Find end of URL (stop at spaces or Chinese punctuation that aren't part of URL)
        // Note: '.' is a valid URL character (domain names, paths) so we don't stop at it.
        // We stop at: whitespace, Chinese comma/period, ASCII comma, and the end of string.
        let url_end = url.find(|c: char| c.is_whitespace() || c == '，' || c == ',' || c == '。')
            .unwrap_or(url.len());
        let url = &url[..url_end];
        args.push("--url".to_string());
        args.push(url.to_string());
    }

    // Extract answers if present (for fill command)
    if subcommand == "fill" {
        // Look for JSON-like answer patterns like {"1":"C","2":"A"}
        if let Some(json_start) = message.find('{') {
            if let Some(json_end) = message[json_start..].find('}') {
                let json_str = &message[json_start..=json_start + json_end];
                args.push("--answers".to_string());
                args.push(json_str.to_string());
            }
        }
        // Look for answers_file patterns like "answers_file": "tools/xxt/answers.json"
        // or file paths ending with .json
        if !args.iter().any(|a| a == "--answers") {
            // Only look for answers_file if no direct answers JSON was found
            if let Some(file_start) = message.find("answers_file") {
                let remaining = &message[file_start..];
                if let Some(path_start) = remaining.find('"') {
                    let path_remaining = &remaining[path_start + 1..];
                    if let Some(path_end) = path_remaining.find('"') {
                        let file_path = &path_remaining[..path_end];
                        args.push("--answers-file".to_string());
                        args.push(file_path.to_string());
                    }
                }
            }
        }
    }

    if args.len() >= 2 {
        Some(args)
    } else {
        None
    }
}

/// A script definition that can be registered as a tool.
pub struct ScriptDef {
    pub name: String,
    pub description: String,
    pub script_path: PathBuf,
    /// Interpreter: "python", "node", "bash", etc. If None, the script is executed directly.
    pub interpreter: Option<String>,
    /// Working directory for the script. If None, uses the script's parent directory.
    pub working_dir: Option<PathBuf>,
    /// Timeout in seconds
    pub timeout_secs: u64,
    /// Optional custom JSON Schema for tool parameters. If None, uses the default generic schema.
    pub parameters_schema: Option<Value>,
}

/// Tool executor that runs external scripts.
pub struct ScriptToolExecutor {
    scripts: Vec<ScriptDef>,
}

impl Default for ScriptToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptToolExecutor {
    pub fn new() -> Self {
        Self {
            scripts: Vec::new(),
        }
    }

    pub fn register_script(&mut self, script: ScriptDef) {
        self.scripts.push(script);
    }
}

#[async_trait]
impl ToolExecutor for ScriptToolExecutor {
    fn executor_id(&self) -> &str {
        "script"
    }

    fn list_tools(&self) -> Vec<Tool> {
        self.scripts
            .iter()
            .map(|s| {
                let schema = s.parameters_schema.clone().unwrap_or_else(|| {
                    serde_json::json!({
                        "type": "object",
                        "properties": {
                            "args": {
                                "type": "array",
                                "items": {"type": "string"},
                                "description": "命令行参数"
                            }
                        }
                    })
                });
                Tool {
                    name: s.name.clone(),
                    description: s.description.clone(),
                    parameters: ToolParameters {
                        schema,
                        required: vec![],
                    },
                    executor_id: "script".to_string(),
                    permission_tags: vec!["script".to_string()],
                    allow_parallel: false,
                    default_timeout_ms: s.timeout_secs * 1000,
                    data_flow_hints: vec![],
                    prerequisites: vec![],
                    output_fields: vec![],
                }
            })
            .collect()
    }

    async fn execute(&self, call: &ToolCall, _ctx: &ToolExecutionContext) -> Result<ToolResult> {
        let start = Instant::now();

        let script = self
            .scripts
            .iter()
            .find(|s| s.name == call.name)
            .ok_or_else(|| {
                agent_teams_core::error::AgentTeamsError::ToolNotFound(format!(
                    "Script not found: {}",
                    call.name
                ))
            })?;

        // Support both generic args array and custom typed parameters
        let args: Vec<String> = if let Some(args_val) = call.arguments.get("args") {
            // Generic schema: {"args": ["crawl", "--url", "..."]}
            let raw_args: Vec<String> = args_val
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            // Fallback: if args is a single string that looks like a natural language message
            // (not a valid subcommand), try to parse it into proper CLI args
            if raw_args.len() == 1 {
                let first_arg = &raw_args[0];
                let valid_subcommands = ["crawl", "fill", "submit", "screenshot", "check", "login"];
                if !valid_subcommands.contains(&first_arg.as_str()) {
                    // Parse natural language into CLI args
                    if let Some(parsed) = parse_natural_language_to_args(first_arg) {
                        tracing::info!(
                            "Parsed natural language into CLI args: {:?}",
                            parsed
                        );
                        parsed
                    } else {
                        raw_args
                    }
                } else {
                    raw_args
                }
            } else {
                raw_args
            }
        } else if script.parameters_schema.is_some() {
            // Custom schema: convert typed params to CLI args
            // e.g. {"subcommand": "crawl", "url": "https://..."} -> ["crawl", "--url", "https://..."]
            let mut cli_args: Vec<String> = Vec::new();
            // First arg is always the subcommand if present
            if let Some(sub) = call.arguments.get("subcommand").and_then(|v| v.as_str()) {
                cli_args.push(sub.to_string());
            }
            // Convert remaining params to --key value pairs
            if let Some(obj) = call.arguments.as_object() {
                for (key, val) in obj {
                    if key == "subcommand" || key == "headless" {
                        continue;
                    }
                    match val {
                        Value::String(s) => {
                            cli_args.push(format!("--{}", key));
                            cli_args.push(s.clone());
                        }
                        Value::Bool(b) => {
                            if *b {
                                cli_args.push(format!("--{}", key));
                            }
                        }
                        Value::Number(n) => {
                            cli_args.push(format!("--{}", key));
                            cli_args.push(n.to_string());
                        }
                        _ => {}
                    }
                }
            }
            // Handle headless flag
            if call.arguments.get("headless").and_then(|v| v.as_bool()).unwrap_or(false) {
                cli_args.push("--headless".to_string());
            }
            cli_args
        } else {
            Vec::new()
        };

        let working_dir = script
            .working_dir
            .as_deref()
            .or_else(|| script.script_path.parent())
            .unwrap_or_else(|| std::path::Path::new("."));

        let mut cmd = if let Some(ref interpreter) = script.interpreter {
            let mut c = tokio::process::Command::new(interpreter);
            c.arg(&script.script_path);
            c
        } else {
            tokio::process::Command::new(&script.script_path)
        };

        cmd.args(&args)
            .current_dir(working_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let timeout = std::time::Duration::from_secs(script.timeout_secs);

        match tokio::time::timeout(timeout, cmd.output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let success = output.status.success();

                Ok(ToolResult {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    success,
                    output: serde_json::json!({
                        "stdout": stdout,
                        "stderr": stderr,
                        "exit_code": output.status.code(),
                    }),
                    error: if success {
                        None
                    } else {
                        Some(format!(
                            "Script exited with code {:?}: {}",
                            output.status.code(),
                            stderr
                        ))
                    },
                    execution_duration_ms: start.elapsed().as_millis() as u64,
                })
            }
            Ok(Err(e)) => Ok(ToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                success: false,
                output: serde_json::Value::Null,
                error: Some(format!("Failed to execute script: {}", e)),
                execution_duration_ms: start.elapsed().as_millis() as u64,
            }),
            Err(_) => Ok(ToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                success: false,
                output: serde_json::Value::Null,
                error: Some(format!(
                    "Script execution timed out after {} seconds",
                    script.timeout_secs
                )),
                execution_duration_ms: start.elapsed().as_millis() as u64,
            }),
        }
    }
}
