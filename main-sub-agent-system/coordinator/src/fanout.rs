use std::sync::Arc;
use std::time::Duration;

use agent_teams_core::boxed_agent::AgentOutput;
use agent_teams_core::context::AgentContext;
use agent_teams_core::message::{AgentMessage, AgentStatus};
use agent_teams_core::registry::AgentRegistry;

use crate::plan_executor::build_input;

/// FanOut coordinator: runs multiple agents in parallel with timeout/panic isolation
pub struct FanOutCoordinator {
    default_timeout_ms: u64,
}

impl FanOutCoordinator {
    pub fn new(default_timeout_ms: u64) -> Self {
        Self { default_timeout_ms }
    }

    /// Run multiple agents in parallel
    pub async fn run_parallel(
        &self,
        ctx: &Arc<AgentContext>,
        msg: &AgentMessage,
        agent_ids: &[String],
        registry: &AgentRegistry,
        timeout_ms: Option<u64>,
    ) -> Vec<(String, AgentOutput)> {
        let timeout = Duration::from_millis(timeout_ms.unwrap_or(self.default_timeout_ms));

        // Build one shared input, clone for each agent
        let base_input = build_input(ctx, msg, ctx.turn_effects.clone(), "");

        let mut handles = Vec::new();
        let mut results = Vec::new();
        for agent_id in agent_ids {
            if let Some(agent) = registry.get(agent_id).await {
                let input = base_input.clone();
                let id = agent_id.clone();

                let handle = tokio::spawn(async move {
                    let agent_id = id.clone();
                    let result = tokio::time::timeout(timeout, agent.run(input)).await;
                    match result {
                        Ok(output) => (id, output),
                        Err(_) => (
                            id,
                            AgentOutput {
                                content: format!("Agent {} timed out", agent_id),
                                status: AgentStatus::Timeout,
                                ..Default::default()
                            },
                        ),
                    }
                });
                handles.push(handle);
            } else {
                tracing::warn!("Agent '{}' not found in registry", agent_id);
                results.push((
                    agent_id.clone(),
                    AgentOutput {
                        content: format!("Agent '{}' not found", agent_id),
                        status: AgentStatus::Error("Agent not registered".to_string()),
                        quality: 0.0,
                        ..Default::default()
                    },
                ));
            }
        }

        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => {
                    tracing::error!("Agent task panicked: {}", e);
                }
            }
        }

        results
    }
}
