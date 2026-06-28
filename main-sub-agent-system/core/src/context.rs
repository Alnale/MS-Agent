use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::context_provider::{ContextProvider, PromptFragment, PromptPriority};
use crate::effect::AgentEffect;
use crate::memory::MemoryEntry;
use crate::pipeline::PipelineDef;
use crate::plan::PlanExecutionState;
use crate::tool::{Tool, ToolCall, ToolResult};

/// Arc-wrapped Value for O(1) cloning with serde support.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SharedValue(#[serde(with = "arc_serde")] Arc<Value>);

mod arc_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::sync::Arc;

    pub fn serialize<T: Serialize, S: Serializer>(val: &Arc<T>, s: S) -> Result<S::Ok, S::Error> {
        val.as_ref().serialize(s)
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Arc<T>, D::Error> {
        T::deserialize(d).map(Arc::new)
    }
}

/// Context passed to an Agent during execution.
/// Large fields use Arc for O(1) cloning in `clone_for_agent`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    pub session_id: String,
    /// User identifier for memory system
    #[serde(default)]
    pub user_id: Option<String>,
    pub entity: Option<Value>,
    #[serde(with = "arc_serde")]
    pub domain_state: Arc<Value>,
    #[serde(with = "arc_serde")]
    pub memory: Arc<Value>,
    #[serde(with = "arc_serde")]
    pub recent_history: Arc<Vec<Value>>,
    #[serde(with = "arc_serde")]
    pub turn_effects: Arc<Vec<AgentEffect>>,
    #[serde(with = "arc_serde")]
    pub prompt_fragments: Arc<Vec<PromptFragment>>,
    #[serde(with = "arc_serde")]
    pub pipeline_def: Arc<PipelineDef>,
    #[serde(with = "arc_serde")]
    pub custom: Arc<Value>,
    pub agent_id: Option<String>,
    /// Working memory: current loaded relevant memory entries
    #[serde(with = "arc_serde", default)]
    pub working_memory: Arc<Vec<MemoryEntry>>,
    /// System instructions that persist across turns (e.g., role settings)
    #[serde(with = "arc_serde")]
    pub system_instructions: Arc<Vec<String>>,
    /// Plan execution state (for PlanNode-based orchestration)
    #[serde(skip)]
    pub plan_state: Arc<RwLock<PlanExecutionState>>,
    /// Available tools for LLM prompt construction
    #[serde(skip)]
    pub available_tools: Arc<Vec<Tool>>,
    /// Tool execution history (current request)
    #[serde(skip)]
    pub tool_history: Arc<Vec<(ToolCall, ToolResult)>>,
    /// Allowed tool names for this request (empty = all tools allowed)
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Optional tool event sender for real-time tool status streaming
    #[serde(skip)]
    pub tool_event_tx: Option<Arc<tokio::sync::mpsc::UnboundedSender<crate::tool::ToolStatusEvent>>>,
}

impl Default for AgentContext {
    fn default() -> Self {
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            user_id: None,
            entity: None,
            domain_state: Arc::new(Value::Null),
            memory: Arc::new(Value::Null),
            recent_history: Arc::new(Vec::new()),
            turn_effects: Arc::new(Vec::new()),
            prompt_fragments: Arc::new(Vec::new()),
            pipeline_def: Arc::new(PipelineDef::default()),
            custom: Arc::new(Value::Null),
            agent_id: None,
            working_memory: Arc::new(Vec::new()),
            system_instructions: Arc::new(Vec::new()),
            plan_state: Arc::new(RwLock::new(PlanExecutionState::default())),
            available_tools: Arc::new(Vec::new()),
            tool_history: Arc::new(Vec::new()),
            allowed_tools: Vec::new(),
            tool_event_tx: None,
        }
    }
}

/// Build memory-enhanced prompt from a slice of memory entries.
/// Shared implementation used by both AgentContext and MemoryManager.
pub fn build_memory_prompt_from_entries(working_memory: &[MemoryEntry]) -> String {
    build_memory_prompt_optimized(working_memory, 2000)  // Default 2000 token budget
}

/// Build memory-enhanced prompt with token budget and priority-based selection.
/// 
/// Priority order:
/// 1. UserProfile (weight > 0.8) - must include
/// 2. Confirmed UserFacts - must include
/// 3. Recent DialogueTurns - include if budget allows
/// 4. Other entries - include if budget allows
pub fn build_memory_prompt_optimized(working_memory: &[MemoryEntry], token_budget: usize) -> String {
    use crate::memory::MemoryKind;
    use std::collections::HashMap;

    if working_memory.is_empty() {
        return String::new();
    }

    // Estimate token count (Chinese chars ~1 token, English words ~1.3 tokens)
    let estimate_tokens = |text: &str| -> usize {
        let char_count = text.chars().count();
        let word_count = text.split_whitespace().count();
        char_count + word_count * 13 / 10
    };

    // Categorize entries by priority
    let mut critical: Vec<&MemoryEntry> = Vec::new();  // weight > 0.8
    let mut high: Vec<&MemoryEntry> = Vec::new();      // confirmed facts
    let mut normal: Vec<&MemoryEntry> = Vec::new();    // other useful entries
    let mut low: Vec<&MemoryEntry> = Vec::new();       // dialogue turns, agent output

    for entry in working_memory {
        match entry.kind {
            MemoryKind::UserProfile if entry.weight > 0.8 => critical.push(entry),
            MemoryKind::UserProfile => normal.push(entry),
            MemoryKind::UserFact if entry.confirmed => high.push(entry),
            MemoryKind::UserFact => normal.push(entry),
            MemoryKind::InferredPreference => normal.push(entry),
            MemoryKind::CrossSessionTopic => normal.push(entry),
            MemoryKind::Summary => normal.push(entry),
            MemoryKind::DialogueTurn => low.push(entry),
            MemoryKind::AgentOutput => low.push(entry),
        }
    }

    // Sort each category by weight (descending)
    critical.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    high.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    normal.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    low.sort_by(|a, b| b.weight.total_cmp(&a.weight));

    let mut sections = Vec::new();
    let mut used_tokens = 0;

    // Phase 1: Critical entries (must include)
    if !critical.is_empty() {
        let content: Vec<String> = critical.iter().map(|e| e.content.clone()).collect();
        let text = format!("## 用户画像\n{}", content.join("\n"));
        let tokens = estimate_tokens(&text);
        if used_tokens + tokens <= token_budget {
            sections.push(text);
            used_tokens += tokens;
        }
    }

    // Phase 2: High priority entries (confirmed facts)
    if !high.is_empty() {
        let content: Vec<String> = high.iter().map(|e| format!("- {}", e.content)).collect();
        let text = format!("## 已知事实（用户明确陈述的信息）\n{}", content.join("\n"));
        let tokens = estimate_tokens(&text);
        if used_tokens + tokens <= token_budget {
            sections.push(text);
            used_tokens += tokens;
        }
    }

    // Phase 3: Normal priority entries
    let mut normal_groups: HashMap<&str, Vec<&str>> = HashMap::new();
    for entry in normal {
        let label = match entry.kind {
            MemoryKind::UserProfile => "用户画像",
            MemoryKind::UserFact => "其他事实",
            MemoryKind::InferredPreference => "推断偏好",
            MemoryKind::CrossSessionTopic => "相关历史主题",
            MemoryKind::Summary => "对话摘要",
            _ => "其他",
        };
        normal_groups.entry(label).or_default().push(&entry.content);
    }

    for label in &["推断偏好", "相关历史主题", "对话摘要", "其他事实", "其他"] {
        if let Some(items) = normal_groups.get(label) {
            if !items.is_empty() {
                let formatted = if *label == "相关历史主题" || *label == "推断偏好" {
                    items.iter().map(|t| format!("- {}", t)).collect::<Vec<_>>().join("\n")
                } else {
                    items.join("\n")
                };
                let text = format!("## {}\n{}", label, formatted);
                let tokens = estimate_tokens(&text);
                if used_tokens + tokens <= token_budget {
                    sections.push(text);
                    used_tokens += tokens;
                }
            }
        }
    }

    // Phase 4: Low priority entries (dialogue turns) - only if budget allows
    if !low.is_empty() {
        let content: Vec<String> = low.iter().map(|e| e.content.clone()).collect();
        let text = format!("## 最近对话\n{}", content.join("\n"));
        let tokens = estimate_tokens(&text);
        if used_tokens + tokens <= token_budget {
            sections.push(text);
        }
    }

    sections.join("\n\n")
}

impl AgentContext {
    /// Clone context for a specific SubAgent. Most fields are O(1) Arc clone.
    pub fn clone_for_agent(&self, agent_id: &str) -> Self {
        Self {
            session_id: self.session_id.clone(),
            user_id: self.user_id.clone(),
            entity: self.entity.clone(),
            domain_state: Arc::clone(&self.domain_state),
            memory: Arc::clone(&self.memory),
            recent_history: Arc::clone(&self.recent_history),
            turn_effects: Arc::clone(&self.turn_effects),
            prompt_fragments: Arc::clone(&self.prompt_fragments),
            pipeline_def: Arc::clone(&self.pipeline_def),
            custom: Arc::clone(&self.custom),
            agent_id: Some(agent_id.to_string()),
            working_memory: Arc::clone(&self.working_memory),
            system_instructions: Arc::clone(&self.system_instructions),
            plan_state: Arc::clone(&self.plan_state),
            available_tools: Arc::clone(&self.available_tools),
            tool_history: Arc::clone(&self.tool_history),
            allowed_tools: self.allowed_tools.clone(),
            tool_event_tx: self.tool_event_tx.clone(),
        }
    }

    /// Collect all prompt fragments sorted by priority.
    /// Since fragments are behind Arc and immutable after construction,
    /// they could be pre-sorted. This method handles both cases efficiently.
    pub fn collect_prompt_fragments(&self) -> Vec<&PromptFragment> {
        let mut fragments: Vec<&PromptFragment> = self.prompt_fragments.iter().collect();
        // Only sort if not already sorted (check adjacent pairs)
        let needs_sort = fragments
            .windows(2)
            .any(|w| (w[0].priority as i32) > (w[1].priority as i32));
        if needs_sort {
            fragments.sort_by_key(|f| f.priority as i32);
        }
        fragments
    }

    /// Build memory-enhanced prompt from working memory
    pub fn build_memory_prompt(&self) -> String {
        build_memory_prompt_from_entries(&self.working_memory)
    }

    /// Build full system prompt from fragments + memory
    ///
    /// Memory is already included via the MemoryContextProvider fragment,
    /// so we don't add it again here.
    pub fn build_system_prompt(&self) -> String {
        let fragments = self.collect_prompt_fragments();
        let base_prompt = fragments
            .iter()
            .map(|f| f.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        // Prepend system instructions if present
        if self.system_instructions.is_empty() {
            base_prompt
        } else {
            let instructions = self.system_instructions.join("\n");
            if base_prompt.is_empty() {
                instructions
            } else {
                format!("{}\n\n{}", instructions, base_prompt)
            }
        }
    }

    /// Add a system instruction that persists across turns
    pub fn add_system_instruction(&mut self, instruction: String) {
        let instructions = Arc::make_mut(&mut self.system_instructions);
        if !instructions.contains(&instruction) {
            instructions.push(instruction);
        }
    }

    /// Set system instructions (replaces all existing)
    pub fn set_system_instructions(&mut self, instructions: Vec<String>) {
        self.system_instructions = Arc::new(instructions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_provider::PromptPriority;

    #[test]
    fn test_clone_for_agent_uses_arc() {
        let ctx = AgentContext::default();
        let cloned = ctx.clone_for_agent("test_agent");

        // Arc fields should be cloned (same underlying data)
        assert!(Arc::ptr_eq(&ctx.domain_state, &cloned.domain_state));
        assert!(Arc::ptr_eq(&ctx.memory, &cloned.memory));
        assert!(Arc::ptr_eq(&ctx.recent_history, &cloned.recent_history));
        assert!(Arc::ptr_eq(&ctx.prompt_fragments, &cloned.prompt_fragments));
        assert!(Arc::ptr_eq(&ctx.pipeline_def, &cloned.pipeline_def));
        assert!(Arc::ptr_eq(&ctx.custom, &cloned.custom));
        assert!(Arc::ptr_eq(&ctx.working_memory, &cloned.working_memory));
        assert!(Arc::ptr_eq(
            &ctx.system_instructions,
            &cloned.system_instructions
        ));

        // agent_id should be set
        assert_eq!(cloned.agent_id, Some("test_agent".to_string()));
    }

    #[test]
    fn test_build_system_prompt_empty() {
        let ctx = AgentContext::default();
        assert_eq!(ctx.build_system_prompt(), "");
    }

    #[test]
    fn test_build_system_prompt_with_fragments() {
        let mut ctx = AgentContext::default();
        let fragments = Arc::make_mut(&mut ctx.prompt_fragments);
        fragments.push(PromptFragment {
            source: "test".to_string(),
            content: "Hello".to_string(),
            priority: PromptPriority::Critical,
        });
        fragments.push(PromptFragment {
            source: "test2".to_string(),
            content: "World".to_string(),
            priority: PromptPriority::Mechanical,
        });

        let prompt = ctx.build_system_prompt();
        assert!(prompt.contains("Hello"));
        assert!(prompt.contains("World"));
    }

    #[test]
    fn test_collect_prompt_fragments_sorted() {
        let mut ctx = AgentContext::default();
        let fragments = Arc::make_mut(&mut ctx.prompt_fragments);
        fragments.push(PromptFragment {
            source: "low".to_string(),
            content: "low priority".to_string(),
            priority: PromptPriority::History,
        });
        fragments.push(PromptFragment {
            source: "high".to_string(),
            content: "high priority".to_string(),
            priority: PromptPriority::Critical,
        });

        let sorted = ctx.collect_prompt_fragments();
        assert_eq!(sorted[0].source, "high");
        assert_eq!(sorted[1].source, "low");
    }

    #[test]
    fn test_system_instructions_in_prompt() {
        let mut ctx = AgentContext::default();
        ctx.add_system_instruction("你是一只可可爱爱香香软软的小猫娘".to_string());

        let prompt = ctx.build_system_prompt();
        assert!(prompt.contains("你是一只可可爱爱香香软软的小猫娘"));
    }

    #[test]
    fn test_system_instructions_with_fragments() {
        let mut ctx = AgentContext::default();
        ctx.add_system_instruction("你是一只小猫娘".to_string());

        let fragments = Arc::make_mut(&mut ctx.prompt_fragments);
        fragments.push(PromptFragment {
            source: "test".to_string(),
            content: "Hello World".to_string(),
            priority: PromptPriority::Mechanical,
        });

        let prompt = ctx.build_system_prompt();
        // System instructions should come before other fragments
        let instruction_pos = prompt.find("你是一只小猫娘").unwrap();
        let hello_pos = prompt.find("Hello World").unwrap();
        assert!(instruction_pos < hello_pos);
    }

    #[test]
    fn test_set_system_instructions() {
        let mut ctx = AgentContext::default();
        ctx.set_system_instructions(vec![
            "instruction 1".to_string(),
            "instruction 2".to_string(),
        ]);

        assert_eq!(ctx.system_instructions.len(), 2);
        assert!(ctx
            .system_instructions
            .contains(&"instruction 1".to_string()));
        assert!(ctx
            .system_instructions
            .contains(&"instruction 2".to_string()));
    }

    #[test]
    fn test_add_system_instruction_no_duplicates() {
        let mut ctx = AgentContext::default();
        ctx.add_system_instruction("same instruction".to_string());
        ctx.add_system_instruction("same instruction".to_string());

        assert_eq!(ctx.system_instructions.len(), 1);
    }
}

/// Multi-turn history context provider
pub struct MultiTurnContextProvider;

#[async_trait]
impl ContextProvider for MultiTurnContextProvider {
    fn id(&self) -> &str {
        "multi_turn_history"
    }
    fn priority(&self) -> PromptPriority {
        PromptPriority::History
    }

    async fn provide(&self, ctx: &AgentContext) -> Option<PromptFragment> {
        if ctx.recent_history.is_empty() {
            return None;
        }

        let max_turns = 20;
        let max_chars_per_turn = 2000;
        let recent: Vec<String> = ctx
            .recent_history
            .iter()
            .take(max_turns)
            .map(|entry| {
                let text = entry.to_string();
                if text.len() > max_chars_per_turn {
                    let mut boundary = max_chars_per_turn;
                    while boundary > 0 && !text.is_char_boundary(boundary) {
                        boundary -= 1;
                    }
                    format!("{}...", &text[..boundary])
                } else {
                    text
                }
            })
            .collect();

        Some(PromptFragment {
            source: "multi_turn_history".to_string(),
            content: format!(
                "## 对话历史（最近 {} 轮）\n{}",
                recent.len(),
                recent.join("\n")
            ),
            priority: PromptPriority::History,
        })
    }
}

/// Domain state context provider
pub struct DomainStateContextProvider;

#[async_trait]
impl ContextProvider for DomainStateContextProvider {
    fn id(&self) -> &str {
        "domain_state"
    }
    fn priority(&self) -> PromptPriority {
        PromptPriority::World
    }

    async fn provide(&self, ctx: &AgentContext) -> Option<PromptFragment> {
        if ctx.domain_state.is_null() {
            return None;
        }
        Some(PromptFragment {
            source: "domain_state".to_string(),
            content: format!("## 领域状态\n{}", ctx.domain_state),
            priority: PromptPriority::World,
        })
    }
}

/// Entity context provider
pub struct EntityContextProvider;

#[async_trait]
impl ContextProvider for EntityContextProvider {
    fn id(&self) -> &str {
        "entity_profile"
    }
    fn priority(&self) -> PromptPriority {
        PromptPriority::Critical
    }

    async fn provide(&self, ctx: &AgentContext) -> Option<PromptFragment> {
        let entity = ctx.entity.as_ref()?;
        Some(PromptFragment {
            source: "entity_profile".to_string(),
            content: format!("## 用户信息\n{}", entity),
            priority: PromptPriority::Critical,
        })
    }
}

/// System instruction context provider
/// Provides persistent system instructions (e.g., role settings) across turns
pub struct SystemInstructionContextProvider;

#[async_trait]
impl ContextProvider for SystemInstructionContextProvider {
    fn id(&self) -> &str {
        "system_instructions"
    }
    fn priority(&self) -> PromptPriority {
        PromptPriority::Critical
    }

    async fn provide(&self, ctx: &AgentContext) -> Option<PromptFragment> {
        if ctx.system_instructions.is_empty() {
            return None;
        }

        let instructions = ctx.system_instructions.join("\n");
        Some(PromptFragment {
            source: "system_instructions".to_string(),
            content: format!("## 要记住的规则\n{}", instructions),
            priority: PromptPriority::Critical,
        })
    }
}

/// Memory context provider
/// Surfaces working memory into the prompt system
pub struct MemoryContextProvider;

#[async_trait]
impl ContextProvider for MemoryContextProvider {
    fn id(&self) -> &str {
        "working_memory"
    }
    fn priority(&self) -> PromptPriority {
        PromptPriority::World
    }

    async fn provide(&self, ctx: &AgentContext) -> Option<PromptFragment> {
        if ctx.working_memory.is_empty() {
            return None;
        }

        let memory_prompt = ctx.build_memory_prompt();
        if memory_prompt.is_empty() {
            return None;
        }

        Some(PromptFragment {
            source: "working_memory".to_string(),
            content: memory_prompt,
            priority: PromptPriority::World,
        })
    }
}

#[cfg(test)]
mod system_instruction_tests {
    use super::*;
    use crate::context_provider::ContextProvider;

    #[tokio::test]
    async fn test_system_instruction_provider_empty() {
        let provider = SystemInstructionContextProvider;
        let ctx = AgentContext::default();

        let result = provider.provide(&ctx).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_system_instruction_provider_with_instructions() {
        let provider = SystemInstructionContextProvider;
        let mut ctx = AgentContext::default();
        ctx.add_system_instruction("你是一只可可爱爱香香软软的小猫娘".to_string());

        let result = provider.provide(&ctx).await;
        assert!(result.is_some());

        let fragment = result.unwrap();
        assert_eq!(fragment.source, "system_instructions");
        assert_eq!(fragment.priority, PromptPriority::Critical);
        assert!(fragment
            .content
            .contains("你是一只可可爱爱香香软软的小猫娘"));
    }

    #[tokio::test]
    async fn test_system_instruction_provider_multiple_instructions() {
        let provider = SystemInstructionContextProvider;
        let mut ctx = AgentContext::default();
        ctx.add_system_instruction("你是一只小猫娘".to_string());
        ctx.add_system_instruction("你喜欢用可爱的语气说话".to_string());

        let result = provider.provide(&ctx).await;
        assert!(result.is_some());

        let fragment = result.unwrap();
        assert!(fragment.content.contains("你是一只小猫娘"));
        assert!(fragment.content.contains("你喜欢用可爱的语气说话"));
    }
}
