use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::AgentContext;
use crate::message::AgentMessage;

/// Hook lifecycle points
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HookPoint {
    PreIntent,
    PreRun,
    PreAgent,
    PostAgent,
    PrePlan,
    PostPlan,
    PreAggregate,
    PostAggregate,
    PreApply,
    PostApply,
    PreCritic,
    PostCritic,
    PostRun,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HookPriority {
    Critical = 0,
    High = 10,
    Normal = 50,
    Low = 100,
}

/// Hook execution result
#[derive(Debug, Clone)]
pub enum HookResult {
    Continue,
    Modified,
    Halt(String),
}

/// Data passed to hooks
pub struct HookData<'a> {
    pub message: &'a mut Option<AgentMessage>,
    pub context: &'a mut AgentContext,
    pub response_content: &'a mut Option<String>,
    pub extra: &'a mut Value,
}

/// Hook context
pub struct HookContext<'a> {
    pub point: HookPoint,
    pub agent_id: Option<&'a str>,
    pub session_id: &'a str,
}

/// Hook trait
#[async_trait]
pub trait Hook: Send + Sync {
    fn name(&self) -> &str;
    fn interests(&self) -> Vec<HookPoint>;
    fn priority(&self) -> HookPriority {
        HookPriority::Normal
    }
    fn is_readonly(&self) -> bool {
        true
    }
    async fn execute(&self, ctx: HookContext<'_>, data: &mut HookData<'_>) -> HookResult;
}

/// Registry for hooks, organized by HookPoint
pub struct HookRegistry {
    hooks: DashMap<HookPoint, Vec<Arc<dyn Hook>>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self {
            hooks: DashMap::new(),
        }
    }

    pub fn register(&self, hook: Arc<dyn Hook>) {
        for point in hook.interests() {
            let mut entry = self.hooks.entry(point).or_default();
            entry.push(hook.clone());
            // Keep hooks sorted by priority on insert so get_hooks doesn't sort every call
            entry.sort_by_key(|h| h.priority() as i32);
        }
    }

    /// Get hooks for a specific point, already sorted by priority
    pub fn get_hooks(&self, point: &HookPoint) -> Vec<Arc<dyn Hook>> {
        self.hooks
            .get(point)
            .map(|hooks| hooks.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn list_all(&self) -> Vec<(String, Vec<HookPoint>)> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for entry in self.hooks.iter() {
            for hook in entry.value() {
                let name = hook.name().to_string();
                if seen.insert(name.clone()) {
                    result.push((name, hook.interests()));
                }
            }
        }
        result
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}
