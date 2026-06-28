use std::sync::Arc;

use async_trait::async_trait;

use agent_teams_core::agent_memory_cache::AgentMemoryCache;
use agent_teams_core::boxed_agent::AgentCapabilities;
use agent_teams_core::domain::DomainModule;
use agent_teams_core::provider::LlmProvider;
use agent_teams_core::registry::AgentRegistry;
use agent_teams_core::routing::{
    RouteCondition, RouteMode, RouteTarget, RoutingRule, RoutingTable,
};
use agent_teams_core::sub_agent::SubAgentDescriptor;
use agent_teams_core::tool::UnifiedToolRegistry;

use crate::sub_agents::script_exec::ScriptDef;
use crate::sub_agents::*;
use crate::tool_engine::ToolExecutionEngine;
use crate::tool_param_infer::ParameterInferrer;
use crate::tools::{DateTimeTool, FileTool, HttpToolExecutor, XxtToolExecutor, DocFlowTool, DocReaderTool, MediaTool};

fn register_all_executors(tool_reg: &UnifiedToolRegistry) {
    tool_reg.register_executor(Arc::new(HttpToolExecutor::new()));
    tool_reg.register_executor(Arc::new(DateTimeTool::new()));
    tool_reg.register_executor(Arc::new(FileTool::new()));
    tool_reg.register_executor(Arc::new(XxtToolExecutor::new()));
    tool_reg.register_executor(Arc::new(DocFlowTool::new()));
    tool_reg.register_executor(Arc::new(DocReaderTool::new()));
    tool_reg.register_executor(Arc::new(MediaTool::new()));
    tracing::info!("Registered dedicated xxt, docflow, docreader, and media tool executors");

    let mut script_exec = ScriptToolExecutor::new();
    let tools_dir = std::env::current_dir().unwrap_or_default().join("tools");
    let python_cmd = std::env::var("PYTHON_PATH").unwrap_or_else(|_| "python".to_string());
    if tools_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&tools_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() { continue; }
                let dir_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                if dir_name == "xxt" { continue; }
                // Check for packaged exe first
                let exe_name = format!("{}.exe", dir_name);
                let exe_path = path.join(&exe_name);
                if exe_path.exists() {
                    let description = std::fs::read_to_string(path.join("description.txt"))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|_| format!("执行工具: {}", dir_name));
                    let parameters_schema = std::fs::read_to_string(path.join("schema.json"))
                        .ok()
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
                    tracing::info!("Registering exe tool: {} ({})", dir_name, exe_path.display());
                    script_exec.register_script(ScriptDef {
                        name: dir_name,
                        description,
                        script_path: exe_path,
                        interpreter: None,
                        working_dir: None,
                        timeout_secs: 300,
                        parameters_schema,
                    });
                    continue;
                }
                for script_name in &["main.py", "run.py", "auto_answer.py", "index.js", "run.sh"] {
                    let script_path = path.join(script_name);
                    if script_path.exists() {
                        let interpreter = match script_path.extension().and_then(|e| e.to_str()) {
                            Some("py") => Some(python_cmd.clone()),
                            Some("js") => Some("node".to_string()),
                            Some("sh") => Some("bash".to_string()),
                            _ => None,
                        };
                        let description = std::fs::read_to_string(path.join("description.txt"))
                            .map(|s| s.trim().to_string())
                            .unwrap_or_else(|_| format!("执行脚本: {}", script_path.display()));
                        let parameters_schema = std::fs::read_to_string(path.join("schema.json"))
                            .ok()
                            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
                        tracing::info!("Registering script tool: {} ({})", dir_name, script_path.display());
                        script_exec.register_script(ScriptDef {
                            name: dir_name,
                            description,
                            script_path,
                            interpreter,
                            working_dir: None,
                            timeout_secs: 300,
                            parameters_schema,
                        });
                        break;
                    }
                }
            }
        }
    }
    tool_reg.register_executor(Arc::new(script_exec));
}

pub struct DomainModuleCS {
    tool_registry: Option<Arc<UnifiedToolRegistry>>,
    sub_agents_config: std::collections::HashMap<String, agent_teams_core::config::SubAgentConfig>,
}

impl DomainModuleCS {
    pub fn new(_provider: Arc<dyn LlmProvider>) -> Self {
        Self { tool_registry: None, sub_agents_config: std::collections::HashMap::new() }
    }

    pub fn with_tool_registry(mut self, registry: Arc<UnifiedToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    pub fn with_sub_agents_config(mut self, config: std::collections::HashMap<String, agent_teams_core::config::SubAgentConfig>) -> Self {
        self.sub_agents_config = config;
        self
    }

    /// Get thinking config for a specific agent from the sub_agents config
    fn thinking_config_for(&self, agent_id: &str) -> Option<agent_teams_core::provider::ThinkingConfig> {
        self.sub_agents_config
            .get(agent_id)
            .and_then(|c| c.thinking.as_ref())
            .map(|t| t.to_thinking_config())
    }

    /// Get max_tokens for a specific agent from the sub_agents config
    fn max_tokens_for(&self, agent_id: &str) -> Option<u32> {
        self.sub_agents_config
            .get(agent_id)
            .and_then(|c| c.config.as_ref())
            .and_then(|c| c.max_tokens)
    }

    pub fn sub_agent_descriptors() -> Vec<SubAgentDescriptor> {
        vec![
            SubAgentDescriptor {
                id: "sentiment".to_string(),
                capabilities: AgentCapabilities {
                    message_types: vec!["sentiment_analysis".to_string(), "user_input".to_string()],
                    requires_llm: true, supports_streaming: false, priority: 70,
                },
                expertise: "情感分析专家：深入分析用户情绪状态、语气特征、紧迫程度、情感变化趋势".to_string(),
                available_tools: Vec::new(), depends_on: Vec::new(), priority: 70,
                fallback_agent_id: None, optional: false, default_effects: Vec::new(), version: None,
            },
            SubAgentDescriptor {
                id: "task_planner".to_string(),
                capabilities: AgentCapabilities {
                    message_types: vec!["routing_decision".to_string(), "task_planning".to_string(), "user_input".to_string(), "tool_request".to_string(), "tool_planning".to_string(), "tool_execution".to_string(), "file_operation".to_string(), "web_request".to_string(), "browser_automation".to_string()],
                    requires_llm: true, supports_streaming: false, priority: 110,
                },
                expertise: "任务规划与工具执行专家：分析请求意图、规划工具调用、执行工具、决定路由".to_string(),
                available_tools: Vec::new(), depends_on: Vec::new(), priority: 110,
                fallback_agent_id: None, optional: false, default_effects: Vec::new(), version: None,
            },
            SubAgentDescriptor {
                id: "summary".to_string(),
                capabilities: AgentCapabilities {
                    message_types: vec!["conversation_summary".to_string()],
                    requires_llm: true, supports_streaming: false, priority: 50,
                },
                expertise: "对话摘要、关键信息提取、记忆压缩".to_string(),
                available_tools: Vec::new(), depends_on: Vec::new(), priority: 50,
                fallback_agent_id: None, optional: true, default_effects: Vec::new(), version: None,
            },
        ]
    }
}

#[async_trait]
impl DomainModule for DomainModuleCS {
    fn id(&self) -> &str { "customer_service" }
    fn name(&self) -> &str { "Customer Service Domain" }

    async fn create_agents(&self, registry: &AgentRegistry, provider: Arc<dyn LlmProvider>) -> Vec<String> {
        let mut agent_ids = Vec::new();

        registry.register(Arc::new(
            SentimentSubAgent::new(provider.clone())
                .with_thinking_config(self.thinking_config_for("sentiment"))
                .with_max_tokens(self.max_tokens_for("sentiment").unwrap_or(16384))
        )).await;
        agent_ids.push("sentiment".to_string());

        let mut task_planner = TaskPlannerAgent::new(provider.clone())
            .with_thinking_config(self.thinking_config_for("task_planner"))
            .with_max_tokens(self.max_tokens_for("task_planner").unwrap_or(16384));
        if let Some(ref tool_reg) = self.tool_registry {
            register_all_executors(tool_reg);
            let engine = Arc::new(ToolExecutionEngine::new(tool_reg.clone()));
            task_planner = task_planner.with_unified_registry(tool_reg.clone());
            task_planner = task_planner.with_tool_executor(Arc::new(HttpToolExecutor::new()));
            task_planner = task_planner.with_tool_engine(engine);
            task_planner = task_planner.with_param_inferrer(Arc::new(ParameterInferrer::new(provider.clone())));
        }
        registry.register(Arc::new(task_planner)).await;
        agent_ids.push("task_planner".to_string());

        registry.register(Arc::new(SummarySubAgent::new(provider.clone()).with_thinking_config(self.thinking_config_for("summary")))).await;
        agent_ids.push("summary".to_string());

        agent_ids
    }

    async fn create_agents_with_cache(&self, registry: &AgentRegistry, provider: Arc<dyn LlmProvider>, cache_factory: &(dyn Fn(&str) -> AgentMemoryCache + Sync)) -> Vec<String> {
        let mut agent_ids = Vec::new();

        registry.register(Arc::new(
            SentimentSubAgent::new(provider.clone())
                .with_agent_memory_cache(cache_factory("sentiment"))
                .with_thinking_config(self.thinking_config_for("sentiment"))
                .with_max_tokens(self.max_tokens_for("sentiment").unwrap_or(16384))
        )).await;
        agent_ids.push("sentiment".to_string());

        let mut task_planner = TaskPlannerAgent::new(provider.clone())
            .with_agent_memory_cache(cache_factory("task_planner"))
            .with_thinking_config(self.thinking_config_for("task_planner"))
            .with_max_tokens(self.max_tokens_for("task_planner").unwrap_or(16384));
        if let Some(ref tool_reg) = self.tool_registry {
            register_all_executors(tool_reg);
            let engine = Arc::new(ToolExecutionEngine::new(tool_reg.clone()));
            task_planner = task_planner.with_unified_registry(tool_reg.clone());
            task_planner = task_planner.with_tool_executor(Arc::new(HttpToolExecutor::new()));
            task_planner = task_planner.with_tool_engine(engine);
            task_planner = task_planner.with_param_inferrer(Arc::new(ParameterInferrer::new(provider.clone())));
        }
        registry.register(Arc::new(task_planner)).await;
        agent_ids.push("task_planner".to_string());

        registry.register(Arc::new(SummarySubAgent::new(provider.clone()).with_agent_memory_cache(cache_factory("summary")).with_thinking_config(self.thinking_config_for("summary")))).await;
        agent_ids.push("summary".to_string());

        agent_ids
    }

    fn routing_table(&self) -> Option<RoutingTable> {
        Some(RoutingTable::new().with_rules(vec![
            RoutingRule {
                name: "sentiment".to_string(),
                condition: RouteCondition::MessageType("sentiment_analysis".to_string()),
                target: RouteTarget { agent_id: "sentiment".to_string(), mode: RouteMode::Direct },
                priority: 100,
            },
            RoutingRule {
                name: "task_planner".to_string(),
                condition: RouteCondition::MessageType("routing_decision".to_string()),
                target: RouteTarget { agent_id: "task_planner".to_string(), mode: RouteMode::Direct },
                priority: 100,
            },
            RoutingRule {
                name: "task_planner_planning".to_string(),
                condition: RouteCondition::MessageType("task_planning".to_string()),
                target: RouteTarget { agent_id: "task_planner".to_string(), mode: RouteMode::Direct },
                priority: 100,
            },
            RoutingRule {
                name: "tool_request".to_string(),
                condition: RouteCondition::MessageType("tool_request".to_string()),
                target: RouteTarget { agent_id: "task_planner".to_string(), mode: RouteMode::Direct },
                priority: 95,
            },
            RoutingRule {
                name: "tool_execution".to_string(),
                condition: RouteCondition::MessageType("tool_execution".to_string()),
                target: RouteTarget { agent_id: "task_planner".to_string(), mode: RouteMode::Direct },
                priority: 95,
            },
            RoutingRule {
                name: "file_operation".to_string(),
                condition: RouteCondition::MessageType("file_operation".to_string()),
                target: RouteTarget { agent_id: "task_planner".to_string(), mode: RouteMode::Direct },
                priority: 95,
            },
            RoutingRule {
                name: "web_request".to_string(),
                condition: RouteCondition::MessageType("web_request".to_string()),
                target: RouteTarget { agent_id: "task_planner".to_string(), mode: RouteMode::Direct },
                priority: 95,
            },
            RoutingRule {
                name: "browser_automation".to_string(),
                condition: RouteCondition::MessageType("browser_automation".to_string()),
                target: RouteTarget { agent_id: "task_planner".to_string(), mode: RouteMode::Direct },
                priority: 95,
            },
            RoutingRule {
                name: "tool_planning".to_string(),
                condition: RouteCondition::MessageType("tool_planning".to_string()),
                target: RouteTarget { agent_id: "task_planner".to_string(), mode: RouteMode::Direct },
                priority: 85,
            },
        ]))
    }
}
