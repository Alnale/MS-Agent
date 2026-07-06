//! Example: Using agent-toolkit as a standalone library
//!
//! This demonstrates how an external project can use agent-toolkit
//! to build an LLM-powered tool-calling agent with just 2 dependencies.

use std::sync::Arc;

use agent_toolkit::{
    AgentToolLoop, ToolExecutionEngine, UnifiedToolRegistry,
    Tool, ToolBuilder, ToolCall, ToolExecutionContext, ToolExecutor, ToolResult,
    ChatMessage, tool_success, Result,
};
use agent_llm::openai::OpenAiProvider;

// ─── Step 1: Define your tools ───────────────────────────────────────

/// A simple calculator tool
struct CalculatorTool;

#[async_trait::async_trait]
impl ToolExecutor for CalculatorTool {
    fn executor_id(&self) -> &str {
        "calculator"
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            ToolBuilder::new("calculator")
                .description("计算数学表达式。支持加减乘除。参数: expression (如 '2+3*4')")
                .executor("calculator")
                .param_string("expression", "数学表达式", true)
                .build(),
        ]
    }

    async fn execute(
        &self,
        call: &ToolCall,
        _ctx: &ToolExecutionContext,
    ) -> Result<ToolResult> {
        let expr = call.arguments["expression"]
            .as_str()
            .unwrap_or("0");

        // Simple eval (demo only — real code should use a proper math parser)
        let result = evaluate_simple(expr).unwrap_or(0.0);

        Ok(tool_success(
            call,
            serde_json::json!({
                "expression": expr,
                "result": result,
            }),
            0,
        ))
    }
}

/// A greeting tool
struct GreetTool;

#[async_trait::async_trait]
impl ToolExecutor for GreetTool {
    fn executor_id(&self) -> &str {
        "greet"
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            ToolBuilder::new("greet")
                .description("生成问候语。参数: name (名字), language (语言: 'zh' 或 'en')")
                .executor("greet")
                .param_string("name", "要问候的人名", true)
                .param_enum("language", "语言", &["zh", "en"], false)
                .build(),
        ]
    }

    async fn execute(
        &self,
        call: &ToolCall,
        _ctx: &ToolExecutionContext,
    ) -> Result<ToolResult> {
        let name = call.arguments["name"].as_str().unwrap_or("World");
        let lang = call.arguments["language"].as_str().unwrap_or("zh");

        let greeting = match lang {
            "en" => format!("Hello, {}! Nice to meet you.", name),
            _ => format!("你好，{}！很高兴认识你。", name),
        };

        Ok(tool_success(call, serde_json::json!({ "greeting": greeting }), 0))
    }
}

// ─── Step 2: Wire everything together ────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Create tool registry and register tools
    let registry = Arc::new(UnifiedToolRegistry::new());
    registry.register_executor(Arc::new(CalculatorTool));
    registry.register_executor(Arc::new(GreetTool));

    println!("Registered tools:");
    for tool in registry.list_tools() {
        println!("  - {}: {}", tool.name, tool.description.lines().next().unwrap_or(""));
    }

    // 2. Create LLM provider (uses MIMO_API_KEY env var)
    let api_key = std::env::var("MIMO_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .unwrap_or_else(|_| {
            eprintln!("Set MIMO_API_KEY or OPENAI_API_KEY environment variable");
            std::process::exit(1);
        });

    let base_url = std::env::var("MIMO_BASE_URL")
        .unwrap_or_else(|_| "https://api.xiaomimimo.com/v1".to_string());

    let provider = Arc::new(OpenAiProvider::new(&base_url, &api_key, "mimo-v2.5-pro"));

    // 3. Create tool execution engine (with retry + circuit breaker)
    let engine = Arc::new(ToolExecutionEngine::new(registry.clone()));

    // 4. Create the agent tool loop
    let loop_agent = AgentToolLoop::new(provider, engine)
        .with_max_iterations(5)
        .with_system_prompt("你是一个有用的助手，可以使用计算器和问候工具。请用中文回答。".to_string());

    // 5. Run!
    let messages = vec![
        ChatMessage::simple("user", "请帮我计算 (12 + 8) * 3 的结果，然后用中文问候一下张三"),
    ];

    let ctx = ToolExecutionContext {
        session_id: "demo".to_string(),
        user_id: Some("demo-user".to_string()),
        agent_id: "demo-agent".to_string(),
        request_id: uuid::Uuid::new_v4().to_string(),
        tool_history: vec![],
        resources: Arc::new(agent_toolkit::ResourcePool::new()),
        agent_context: None,
    };

    println!("\n--- Running agent ---\n");

    let (output, tool_history) = loop_agent.run(messages, registry.list_tools(), &ctx).await?;

    println!("Agent response:\n{}", output.content);

    if !tool_history.is_empty() {
        println!("\nTool calls made:");
        for (call, result) in &tool_history {
            println!("  {} -> success={}", call.name, result.success);
        }
    }

    Ok(())
}

/// Simple expression evaluator (handles +, -, *, / on numbers)
fn evaluate_simple(expr: &str) -> Option<f64> {
    let expr: String = expr.chars().filter(|c| !c.is_whitespace()).collect();
    // Try parsing as a single number first
    if let Ok(n) = expr.parse::<f64>() {
        return Some(n);
    }
    // Split on + or - (lowest precedence, left to right)
    for op in ['+', '-'] {
        if let Some(pos) = expr.rfind(op) {
            if pos == 0 { continue; } // negative sign
            let left = evaluate_simple(&expr[..pos])?;
            let right = evaluate_simple(&expr[pos + 1..])?;
            return Some(if op == '+' { left + right } else { left - right });
        }
    }
    // Split on * or /
    for op in ['*', '/'] {
        if let Some(pos) = expr.rfind(op) {
            let left = evaluate_simple(&expr[..pos])?;
            let right = evaluate_simple(&expr[pos + 1..])?;
            if op == '/' && right == 0.0 { return None; }
            return Some(if op == '*' { left * right } else { left / right });
        }
    }
    // Handle parentheses
    if expr.starts_with('(') && expr.ends_with(')') {
        return evaluate_simple(&expr[1..expr.len() - 1]);
    }
    None
}
