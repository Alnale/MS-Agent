use std::sync::Arc;

use agent_core::agent_memory_cache::AgentMemoryCache;
use agent_core::boxed_agent::BoxedAgent;
use agent_core::provider::LlmProvider;
use agent_core::sub_agent::SubAgentDescriptor;
use agent_core::tool::UnifiedToolRegistry;

use crate::sub_agents::*;
use crate::sub_agents::sentiment::SentimentSubAgent;
use crate::sub_agents::task_planner::TaskPlannerAgent;
use crate::tool_engine::ToolExecutionEngine;

/// Trait for agent plugins that can be registered declaratively
pub trait AgentPlugin: Send + Sync {
    fn id(&self) -> &str;
    fn descriptor(&self) -> SubAgentDescriptor;
    fn create(
        &self,
        provider: Arc<dyn LlmProvider>,
        tool_registry: Option<Arc<UnifiedToolRegistry>>,
    ) -> Arc<dyn BoxedAgent>;

    fn create_with_cache(
        &self,
        provider: Arc<dyn LlmProvider>,
        tool_registry: Option<Arc<UnifiedToolRegistry>>,
        _cache: AgentMemoryCache,
    ) -> Arc<dyn BoxedAgent> {
        self.create(provider, tool_registry)
    }
}

/// Registry for agent plugins
pub struct PluginRegistry {
    plugins: Vec<Box<dyn AgentPlugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    pub fn register(&mut self, plugin: Box<dyn AgentPlugin>) {
        self.plugins.push(plugin);
    }

    pub fn plugin_ids(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.id()).collect()
    }

    pub fn get(&self, id: &str) -> Option<&dyn AgentPlugin> {
        self.plugins
            .iter()
            .find(|p| p.id() == id)
            .map(|p| p.as_ref())
    }

    pub fn descriptors(&self) -> Vec<SubAgentDescriptor> {
        self.plugins.iter().map(|p| p.descriptor()).collect()
    }

    pub fn create_all(
        &self,
        provider: Arc<dyn LlmProvider>,
        tool_registry: Option<Arc<UnifiedToolRegistry>>,
    ) -> Vec<Arc<dyn BoxedAgent>> {
        self.plugins
            .iter()
            .map(|p| p.create(provider.clone(), tool_registry.clone()))
            .collect()
    }

    pub fn create_by_ids(
        &self,
        ids: &[&str],
        provider: Arc<dyn LlmProvider>,
        tool_registry: Option<Arc<UnifiedToolRegistry>>,
    ) -> Vec<Arc<dyn BoxedAgent>> {
        self.plugins
            .iter()
            .filter(|p| ids.contains(&p.id()))
            .map(|p| p.create(provider.clone(), tool_registry.clone()))
            .collect()
    }

    pub fn create_all_with_cache(
        &self,
        provider: Arc<dyn LlmProvider>,
        tool_registry: Option<Arc<UnifiedToolRegistry>>,
        cache_factory: &dyn Fn(&str) -> AgentMemoryCache,
    ) -> Vec<Arc<dyn BoxedAgent>> {
        self.plugins
            .iter()
            .map(|p| {
                let cache = cache_factory(p.id());
                p.create_with_cache(provider.clone(), tool_registry.clone(), cache)
            })
            .collect()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Built-in agent plugins
// ============================================================================

pub struct SentimentPlugin;
pub struct TaskPlannerPlugin;
pub struct SummaryPlugin;

impl AgentPlugin for SentimentPlugin {
    fn id(&self) -> &str { "sentiment" }

    fn descriptor(&self) -> SubAgentDescriptor {
        SubAgentDescriptor {
            id: "sentiment".to_string(),
            capabilities: agent_core::boxed_agent::AgentCapabilities {
                message_types: vec!["sentiment_analysis".to_string(), "user_input".to_string()],
                requires_llm: true,
                supports_streaming: false,
                priority: 70,
            },
            expertise: "情感分析专家：多维度情绪分析（主导/底层/复合情绪）、讽刺反语检测、中文特化情感信号识别、情感需求推断、情感轨迹追踪、对话阶段感知".to_string(),
            available_tools: Vec::new(),
            depends_on: Vec::new(),
            priority: 70,
            fallback_agent_id: None,
            optional: false,
            default_effects: Vec::new(),
            version: None,
        }
    }

    fn create(&self, provider: Arc<dyn LlmProvider>, _: Option<Arc<UnifiedToolRegistry>>) -> Arc<dyn BoxedAgent> {
        Arc::new(SentimentSubAgent::new(provider))
    }

    fn create_with_cache(&self, provider: Arc<dyn LlmProvider>, _: Option<Arc<UnifiedToolRegistry>>, cache: AgentMemoryCache) -> Arc<dyn BoxedAgent> {
        Arc::new(SentimentSubAgent::new(provider).with_agent_memory_cache(cache))
    }
}

impl AgentPlugin for TaskPlannerPlugin {
    fn id(&self) -> &str { "task_planner" }

    fn descriptor(&self) -> SubAgentDescriptor {
        SubAgentDescriptor {
            id: "task_planner".to_string(),
            capabilities: agent_core::boxed_agent::AgentCapabilities {
                message_types: vec![
                    "routing_decision".to_string(),
                    "task_planning".to_string(),
                    "user_input".to_string(),
                    "tool_request".to_string(),
                    "tool_planning".to_string(),
                    "tool_execution".to_string(),
                    "file_operation".to_string(),
                    "web_request".to_string(),
                    "browser_automation".to_string(),
                ],
                requires_llm: true,
                supports_streaming: false,
                priority: 110,
            },
            expertise: "任务规划与工具执行专家：分析请求意图、规划工具调用、执行工具、决定路由".to_string(),
            available_tools: Vec::new(),
            depends_on: Vec::new(),
            priority: 110,
            fallback_agent_id: None,
            optional: false,
            default_effects: Vec::new(),
            version: None,
        }
    }

    fn create(&self, provider: Arc<dyn LlmProvider>, tool_registry: Option<Arc<UnifiedToolRegistry>>) -> Arc<dyn BoxedAgent> {
        let mut agent = TaskPlannerAgent::new(provider.clone());
        if let Some(ref tool_reg) = tool_registry {
            let engine = Arc::new(ToolExecutionEngine::new(tool_reg.clone()));
            agent = agent.with_unified_registry(tool_reg.clone());

            agent = agent.with_tool_engine(engine);
            agent = agent.with_param_inferrer(Arc::new(
                crate::tool_param_infer::ParameterInferrer::new(provider),
            ));
        }
        Arc::new(agent)
    }

    fn create_with_cache(&self, provider: Arc<dyn LlmProvider>, tool_registry: Option<Arc<UnifiedToolRegistry>>, cache: AgentMemoryCache) -> Arc<dyn BoxedAgent> {
        let mut agent = TaskPlannerAgent::new(provider.clone()).with_agent_memory_cache(cache);
        if let Some(ref tool_reg) = tool_registry {
            let engine = Arc::new(ToolExecutionEngine::new(tool_reg.clone()));
            agent = agent.with_unified_registry(tool_reg.clone());

            agent = agent.with_tool_engine(engine);
            agent = agent.with_param_inferrer(Arc::new(
                crate::tool_param_infer::ParameterInferrer::new(provider),
            ));
        }
        Arc::new(agent)
    }
}

impl AgentPlugin for SummaryPlugin {
    fn id(&self) -> &str { "summary" }

    fn descriptor(&self) -> SubAgentDescriptor {
        SubAgentDescriptor {
            id: "summary".to_string(),
            capabilities: agent_core::boxed_agent::AgentCapabilities {
                message_types: vec!["conversation_summary".to_string()],
                requires_llm: true,
                supports_streaming: false,
                priority: 50,
            },
            expertise: "对话摘要、关键信息提取、记忆压缩".to_string(),
            available_tools: Vec::new(),
            depends_on: Vec::new(),
            priority: 50,
            fallback_agent_id: None,
            optional: true,
            default_effects: Vec::new(),
            version: None,
        }
    }

    fn create(&self, provider: Arc<dyn LlmProvider>, _: Option<Arc<UnifiedToolRegistry>>) -> Arc<dyn BoxedAgent> {
        Arc::new(SummarySubAgent::new(provider))
    }

    fn create_with_cache(&self, provider: Arc<dyn LlmProvider>, _: Option<Arc<UnifiedToolRegistry>>, cache: AgentMemoryCache) -> Arc<dyn BoxedAgent> {
        Arc::new(SummarySubAgent::new(provider).with_agent_memory_cache(cache))
    }
}

// ============================================================================
// Registry and Factory
// ============================================================================

pub fn create_builtin_registry() -> PluginRegistry {
    let mut registry = PluginRegistry::new();
    registry.register(Box::new(SentimentPlugin));
    registry.register(Box::new(TaskPlannerPlugin));
    registry.register(Box::new(SummaryPlugin));
    registry
}

pub struct AgentFactory {
    provider: Arc<dyn LlmProvider>,
    tool_registry: Option<Arc<UnifiedToolRegistry>>,
}

impl AgentFactory {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            tool_registry: None,
        }
    }

    pub fn with_tool_registry(mut self, registry: Arc<UnifiedToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    pub fn create_agent(&self, agent_id: &str) -> Option<Arc<dyn BoxedAgent>> {
        match agent_id {
            "sentiment" => Some(Arc::new(SentimentSubAgent::new(self.provider.clone()))),
            "task_planner" => {
                let mut agent = TaskPlannerAgent::new(self.provider.clone());
                if let Some(ref tool_reg) = self.tool_registry {
                    let engine = Arc::new(ToolExecutionEngine::new(tool_reg.clone()));
                    agent = agent.with_unified_registry(tool_reg.clone());
        
                    agent = agent.with_tool_engine(engine);
                    agent = agent.with_param_inferrer(Arc::new(
                        crate::tool_param_infer::ParameterInferrer::new(self.provider.clone()),
                    ));
                }
                Some(Arc::new(agent))
            }
            "summary" => Some(Arc::new(SummarySubAgent::new(self.provider.clone()))),
            _ => None,
        }
    }

    pub fn create_default_agents(&self) -> Vec<Arc<dyn BoxedAgent>> {
        let agent_ids = vec!["sentiment", "task_planner", "summary"];
        agent_ids.into_iter().filter_map(|id| self.create_agent(id)).collect()
    }

    pub fn get_all_descriptors() -> Vec<SubAgentDescriptor> {
        vec![
            SubAgentDescriptor {
                id: "sentiment".to_string(),
                capabilities: agent_core::boxed_agent::AgentCapabilities {
                    message_types: vec!["sentiment_analysis".to_string(), "user_input".to_string()],
                    requires_llm: true, supports_streaming: false, priority: 70,
                },
                expertise: "情感分析专家：多维度情绪分析（主导/底层/复合情绪）、讽刺反语检测、中文特化情感信号识别、情感需求推断、情感轨迹追踪、对话阶段感知".to_string(),
                available_tools: Vec::new(), depends_on: Vec::new(), priority: 70,
                fallback_agent_id: None, optional: false, default_effects: Vec::new(), version: None,
            },
            SubAgentDescriptor {
                id: "task_planner".to_string(),
                capabilities: agent_core::boxed_agent::AgentCapabilities {
                    message_types: vec!["routing_decision".to_string(), "task_planning".to_string(), "user_input".to_string(), "tool_request".to_string(), "tool_planning".to_string(), "tool_execution".to_string(), "file_operation".to_string(), "web_request".to_string(), "browser_automation".to_string()],
                    requires_llm: true, supports_streaming: false, priority: 110,
                },
                expertise: "任务规划与工具执行专家：分析请求意图、规划工具调用、执行工具、决定路由".to_string(),
                available_tools: Vec::new(), depends_on: Vec::new(), priority: 110,
                fallback_agent_id: None, optional: false, default_effects: Vec::new(), version: None,
            },
            SubAgentDescriptor {
                id: "summary".to_string(),
                capabilities: agent_core::boxed_agent::AgentCapabilities {
                    message_types: vec!["conversation_summary".to_string()],
                    requires_llm: true, supports_streaming: false, priority: 50,
                },
                expertise: "对话摘要、关键信息提取、记忆压缩".to_string(),
                available_tools: Vec::new(), depends_on: Vec::new(), priority: 50,
                fallback_agent_id: None, optional: true, default_effects: Vec::new(), version: None,
            },
        ]
    }

    pub fn get_descriptor(agent_id: &str) -> Option<SubAgentDescriptor> {
        Self::get_all_descriptors().into_iter().find(|d| d.id == agent_id)
    }

    pub fn is_valid_agent_id(agent_id: &str) -> bool {
        matches!(agent_id, "sentiment" | "task_planner" | "summary")
    }
}

pub struct ConfigDrivenAgentRegistry {
    factory: AgentFactory,
    registered_agents: Vec<String>,
}

impl ConfigDrivenAgentRegistry {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            factory: AgentFactory::new(provider),
            registered_agents: Vec::new(),
        }
    }

    pub fn with_tool_registry(mut self, registry: Arc<UnifiedToolRegistry>) -> Self {
        self.factory = self.factory.with_tool_registry(registry);
        self
    }

    pub async fn register_from_config(
        &mut self,
        registry: &agent_core::registry::AgentRegistry,
        config: &serde_json::Value,
    ) -> Vec<String> {
        let mut registered = Vec::new();
        let agent_ids = if let Some(agents) = config.get("sub_agents").and_then(|v| v.as_array()) {
            agents.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect::<Vec<_>>()
        } else {
            vec!["sentiment".to_string(), "task_planner".to_string(), "summary".to_string()]
        };

        for agent_id in agent_ids {
            if let Some(agent) = self.factory.create_agent(&agent_id) {
                registry.register(agent).await;
                registered.push(agent_id.clone());
                self.registered_agents.push(agent_id);
            }
        }
        registered
    }

    pub fn registered_agents(&self) -> &[String] {
        &self.registered_agents
    }

    pub fn is_agent_registered(&self, agent_id: &str) -> bool {
        self.registered_agents.contains(&agent_id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_factory_create_agent() {
        assert!(AgentFactory::is_valid_agent_id("sentiment"));
        assert!(AgentFactory::is_valid_agent_id("task_planner"));
        assert!(AgentFactory::is_valid_agent_id("summary"));
        assert!(!AgentFactory::is_valid_agent_id("knowledge"));
        assert!(!AgentFactory::is_valid_agent_id("tool_agent"));
        assert!(!AgentFactory::is_valid_agent_id("analysis"));
        assert!(!AgentFactory::is_valid_agent_id("invalid"));
    }

    #[test]
    fn test_agent_factory_get_descriptor() {
        assert!(AgentFactory::get_descriptor("sentiment").is_some());
        assert!(AgentFactory::get_descriptor("task_planner").is_some());
        assert!(AgentFactory::get_descriptor("summary").is_some());
        assert!(AgentFactory::get_descriptor("knowledge").is_none());
        assert!(AgentFactory::get_descriptor("tool_agent").is_none());
        assert!(AgentFactory::get_descriptor("analysis").is_none());
        assert!(AgentFactory::get_descriptor("invalid").is_none());
    }

    #[test]
    fn test_agent_factory_get_all_descriptors() {
        let descriptors = AgentFactory::get_all_descriptors();
        assert_eq!(descriptors.len(), 3);
        let ids: Vec<&str> = descriptors.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"sentiment"));
        assert!(ids.contains(&"task_planner"));
        assert!(ids.contains(&"summary"));
    }
}
