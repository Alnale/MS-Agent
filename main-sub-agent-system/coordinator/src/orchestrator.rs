use std::pin::Pin;
use std::sync::Arc;

use agent_teams_core::boxed_agent::{AgentInput, AgentOutput, ToolInfo};
use agent_teams_core::context::AgentContext;
use agent_teams_core::error::{AgentTeamsError, Result};
use agent_teams_core::plan::{ArgumentSource, PlanExecutionState, PlanNode};
use agent_teams_core::registry::AgentRegistry;
use agent_teams_core::tool::{ToolCall, UnifiedToolRegistry};

use crate::memory_manager::MemoryManager;

/// Orchestrator: executes PlanNodes (Agent + Tool + Condition + Parallel + Sequential)
///
/// All tool execution is delegated to the `task_planner` SubAgent.
/// The orchestrator itself never directly calls tools.
pub struct Orchestrator {
    agent_registry: Arc<AgentRegistry>,
    memory_manager: Option<Arc<MemoryManager>>,
    tool_registry: Option<Arc<UnifiedToolRegistry>>,
}

impl Orchestrator {
    pub fn new(
        agent_registry: Arc<AgentRegistry>,
        memory_manager: Option<Arc<MemoryManager>>,
    ) -> Self {
        Self {
            agent_registry,
            memory_manager,
            tool_registry: None,
        }
    }

    /// Set the tool registry for providing tool info to agents
    pub fn with_tool_registry(mut self, registry: Arc<UnifiedToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    /// Get available tools info for agent input
    fn get_available_tools_info(&self) -> Vec<ToolInfo> {
        match &self.tool_registry {
            Some(registry) => registry.list_tools().iter().map(|t| {
                let params_hint = t.parameters.required.join(", ");
                ToolInfo {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters_hint: if params_hint.is_empty() {
                        "无必填参数".to_string()
                    } else {
                        format!("必填参数: {}", params_hint)
                    },
                }
            }).collect(),
            None => Vec::new(),
        }
    }

    /// Execute a single PlanNode (boxed for recursion)
    pub fn execute_node<'a>(
        &'a self,
        ctx: &'a Arc<AgentContext>,
        node: &'a PlanNode,
        state: &'a mut PlanExecutionState,
        current_msg: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value>> + Send + 'a>> {
        Box::pin(async move {
            match node {
                PlanNode::Agent {
                    agent_id,
                    input_transform,
                } => {
                    let agent = self.agent_registry.get(agent_id).await.ok_or_else(|| {
                        AgentTeamsError::NotFound(format!("Agent not found: {}", agent_id))
                    })?;

                    let input = self.build_agent_input(ctx, state, input_transform, agent_id, current_msg)?;
                    let output = agent.run(input).await;

                    // Sync to memory if available
                    if let Some(mm) = &self.memory_manager {
                        self.sync_output_to_memory(mm, ctx, agent_id, &output).await;
                    }

                    // If the agent returned tool_calls, execute them through task_planner SubAgent.
                    if !output.effects.is_empty() {
                        let tool_triggers: Vec<_> = output.effects.iter()
                            .filter_map(|e| match e {
                                agent_teams_core::effect::AgentEffect::ToolTrigger {
                                    tool_name, input, ..
                                } => Some((tool_name.clone(), input.clone())),
                                _ => None,
                            })
                            .collect();

                        if !tool_triggers.is_empty() {
                            tracing::info!(
                                "Agent '{}' returned {} tool_calls, delegating to task_planner (parallel)",
                                agent_id, tool_triggers.len()
                            );

                            let task_planner = self.agent_registry.get("task_planner").await
                                .ok_or_else(|| AgentTeamsError::NotFound(
                                    "task_planner SubAgent not registered".to_string()
                                ))?;

                            // Execute all independent tool calls in parallel
                            let tool_futures: Vec<_> = tool_triggers.into_iter().map(|(tool_name, arguments)| {
                                let call_id = uuid::Uuid::new_v4().to_string();
                                let tool_meta = serde_json::json!({
                                    "tool_name": tool_name,
                                    "arguments": arguments,
                                    "call_id": call_id,
                                });

                                let content = format!(
                                    "[TOOL_CALL]\n{}\n[/TOOL_CALL]\n\n调用 `{}` 这个工具。",
                                    tool_meta, tool_name
                                );

                                let tool_input = AgentInput {
                                    system_prompt: ctx.build_system_prompt(),
                                    content,
                                    recent_history: ctx.recent_history.as_ref().clone(),
                                    prior_effects: ctx.turn_effects.clone(),
                                    session_id: Some(ctx.session_id.clone()),
                                    user_id: ctx.user_id.clone(),
                                    available_tools: self.get_available_tools_info(),
                                    agent_context: Some(ctx.clone()),
                                };

                                // Clone the task_planner Arc for each concurrent call
                                let planner = task_planner.clone();
                                async move {
                                    let tool_output = planner.run(tool_input).await;
                                    serde_json::to_value(&tool_output).unwrap_or_default()
                                }
                            }).collect();

                            let tool_results = futures::future::join_all(tool_futures).await;

                            // Sync tool outputs to memory
                            if let Some(mm) = &self.memory_manager {
                                for result in &tool_results {
                                    if let Ok(output) = serde_json::from_value::<AgentOutput>(result.clone()) {
                                        self.sync_output_to_memory(mm, ctx, "task_planner", &output).await;
                                    }
                                }
                            }

                            // Return combined result: agent output + tool results
                            return Ok(serde_json::json!({
                                "agent": serde_json::to_value(&output).unwrap_or_default(),
                                "tool_results": tool_results,
                            }));
                        }
                    }

                    Ok(serde_json::to_value(&output).unwrap_or_default())
                }

                PlanNode::Tool {
                    tool_name,
                    arguments_source,
                } => {
                    let tool_call = self.build_tool_call(state, arguments_source, tool_name)?;

                    // All tool execution goes through task_planner SubAgent.
                    let task_planner = self.agent_registry.get("task_planner").await
                        .ok_or_else(|| AgentTeamsError::NotFound(
                            "task_planner SubAgent not registered. Cannot execute tool without it.".to_string()
                        ))?;

                    tracing::info!(
                        "Delegating tool '{}' to task_planner SubAgent",
                        tool_name
                    );

                    let tool_meta = serde_json::json!({
                        "tool_name": tool_call.name,
                        "arguments": tool_call.arguments,
                        "call_id": tool_call.id,
                    });

                    let prior_context = if !state.node_results.is_empty() {
                        let summaries: Vec<String> = state.node_results.iter().enumerate()
                            .map(|(i, v)| format!("上一步的结果[{}]: {}", i, v))
                            .collect();
                        format!("\n\n{}", summaries.join("\n"))
                    } else {
                        String::new()
                    };

                    let content = format!(
                        "[TOOL_CALL]\n{}\n[/TOOL_CALL]\n\n调用 `{}` 这个工具，执行完返回结果。{}",
                        tool_meta, tool_name, prior_context
                    );

                    let agent_input = AgentInput {
                        system_prompt: ctx.build_system_prompt(),
                        content,
                        recent_history: ctx.recent_history.as_ref().clone(),
                        prior_effects: ctx.turn_effects.clone(),
                        session_id: Some(ctx.session_id.clone()),
                        user_id: ctx.user_id.clone(),
                        available_tools: self.get_available_tools_info(),
                        agent_context: Some(ctx.clone()),
                    };

                    let output = task_planner.run(agent_input).await;

                    // Sync to memory if available
                    if let Some(mm) = &self.memory_manager {
                        self.sync_output_to_memory(mm, ctx, "task_planner", &output).await;
                    }

                    Ok(serde_json::to_value(&output).unwrap_or_default())
                }

                PlanNode::Condition {
                    expression,
                    then_branch,
                    else_branch,
                } => {
                    let condition_met = self.evaluate_condition(expression, state);
                    let branch = if condition_met {
                        then_branch
                    } else {
                        else_branch
                    };
                    let mut last_result = serde_json::Value::Null;
                    for node in branch {
                        last_result = self.execute_node(ctx, node, state, current_msg).await?;
                    }
                    Ok(last_result)
                }

                PlanNode::Parallel(nodes) => {
                    // For parallel execution, we collect results without mutating state
                    // until all nodes complete
                    let mut futures = Vec::new();
                    for node in nodes {
                        // We need to create a temporary state for each parallel branch
                        // that doesn't share the mutable reference
                        let temp_state = state.clone();
                        futures.push(self.execute_node_with_state(ctx, node, temp_state, current_msg));
                    }
                    let results = futures::future::join_all(futures).await;
                    let values: Result<Vec<_>> = results.into_iter().collect();
                    let values = values?;
                    // Push all results to state
                    for v in &values {
                        state.node_results.push(v.clone());
                    }
                    Ok(serde_json::json!(values))
                }

                PlanNode::Sequential(nodes) => {
                    let mut last_result = serde_json::Value::Null;
                    for node in nodes {
                        last_result = self.execute_node(ctx, node, state, current_msg).await?;
                        state.node_results.push(last_result.clone());
                    }
                    Ok(last_result)
                }
            }
        })
    }

    /// Execute a node with a cloned state (for parallel branches)
    fn execute_node_with_state<'a>(
        &'a self,
        ctx: &'a Arc<AgentContext>,
        node: &'a PlanNode,
        mut state: PlanExecutionState,
        current_msg: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value>> + Send + 'a>> {
        Box::pin(async move { self.execute_node(ctx, node, &mut state, current_msg).await })
    }

    fn build_agent_input(
        &self,
        ctx: &Arc<AgentContext>,
        state: &PlanExecutionState,
        input_transform: &Option<String>,
        agent_id: &str,
        current_msg: &str,
    ) -> Result<AgentInput> {
        let content = if let Some(transform) = input_transform {
            if let Some(last) = state.node_results.last() {
                extract_json_path(last, transform)
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            } else {
                // No prior node results — use current user message
                current_msg.to_string()
            }
        } else {
            current_msg.to_string()
        };

        let mut system_prompt = ctx.build_system_prompt();

        // Inject prior node results as context for cross-node collaboration
        if !state.node_results.is_empty() {
            let prior_summaries: Vec<String> = state
                .node_results
                .iter()
                .enumerate()
                .filter_map(|(i, v)| {
                    let s = v.to_string();
                    if s.len() > 10 && s != "null" {
                        let preview: String = s.chars().take(300).collect();
                        Some(format!("[节点{}] {}", i, preview))
                    } else {
                        None
                    }
                })
                .collect();

            if !prior_summaries.is_empty() {
                system_prompt.push_str(&format!(
                    "\n\n## 之前的执行结果（参考一下）\n{}",
                    prior_summaries.join("\n")
                ));
            }
        }

        // Only inject tool info for tool-capable agents.
        // Other agents (analysis, summary) should focus on their own domain.
        let is_tool_agent = agent_id == "task_planner";
        let available_tools = if is_tool_agent {
            self.get_available_tools_info()
        } else {
            Vec::new()
        };

        Ok(AgentInput {
            system_prompt,
            content,
            recent_history: ctx.recent_history.as_ref().clone(),
            prior_effects: ctx.turn_effects.clone(),
            session_id: Some(ctx.session_id.clone()),
            user_id: ctx.user_id.clone(),
            available_tools,
            agent_context: Some(ctx.clone()),
        })
    }

    fn build_tool_call(
        &self,
        state: &PlanExecutionState,
        arguments_source: &ArgumentSource,
        tool_name: &str,
    ) -> Result<ToolCall> {
        let arguments = match arguments_source {
            ArgumentSource::FromUserMessage { field } => {
                if let Some(last) = state.node_results.last() {
                    last.get(field).cloned().unwrap_or(serde_json::Value::Null)
                } else {
                    serde_json::Value::Null
                }
            }
            ArgumentSource::FromUpstream {
                node_index,
                json_path,
            } => {
                if let Some(result) = state.node_results.get(*node_index) {
                    extract_json_path(result, json_path).unwrap_or(serde_json::Value::Null)
                } else {
                    serde_json::Value::Null
                }
            }
            ArgumentSource::FromContext { key } => {
                serde_json::json!({"context_key": key})
            }
            ArgumentSource::Static(value) => {
                let mut args = value.clone();
                // Check if args contain upstream content references
                if let Some(source_node) = args.get("content_source_node").and_then(|v| v.as_u64()).map(|n| n as usize) {
                    let json_path = args.get("content_json_path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("content");
                    if let Some(upstream) = state.node_results.get(source_node) {
                        if let Some(content_val) = extract_json_path(upstream, json_path) {
                            if let Some(obj) = args.as_object_mut() {
                                obj.insert("content".to_string(), content_val);
                                obj.remove("content_source_node");
                                obj.remove("content_json_path");
                            }
                        }
                    }
                }
                args
            }
        };

        Ok(ToolCall {
            id: uuid::Uuid::new_v4().to_string(),
            name: tool_name.to_string(),
            arguments,
        })
    }

    fn evaluate_condition(&self, expression: &str, state: &PlanExecutionState) -> bool {
        if let Some(last) = state.node_results.last() {
            if let Some(field) = expression.strip_prefix("exists:") {
                last.get(field).is_some()
            } else if let Some(field) = expression.strip_prefix("truthy:") {
                last.get(field)
                    .map(|v| !v.is_null() && *v != serde_json::json!(false))
                    .unwrap_or(false)
            } else {
                !last.is_null()
            }
        } else {
            false
        }
    }

    async fn sync_output_to_memory(
        &self,
        mm: &Arc<MemoryManager>,
        ctx: &AgentContext,
        agent_id: &str,
        output: &AgentOutput,
    ) {
        if output.content.is_empty() || output.quality < 0.3 {
            return;
        }
        let entry = crate::memory_helpers::build_agent_output_memory_entry(
            agent_id,
            &output.content,
            &ctx.session_id,
            output.quality,
        );
        if let Err(e) = mm.long_term_store().store(entry).await {
            tracing::warn!("Failed to sync memory for agent {}: {}", agent_id, e);
        }
    }

}

/// Simple JSON path extraction (supports dot notation like "field.subfield")
fn extract_json_path(value: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;
    for part in parts {
        current = current.get(part)?;
    }
    Some(current.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_path() {
        let value = serde_json::json!({"a": {"b": 42}});
        assert_eq!(
            extract_json_path(&value, "a.b"),
            Some(serde_json::json!(42))
        );
        assert_eq!(extract_json_path(&value, "a.c"), None);
    }

    #[test]
    fn test_evaluate_condition() {
        let orchestrator = Orchestrator::new(
            Arc::new(AgentRegistry::new()),
            None,
        );

        let mut state = PlanExecutionState::default();
        assert!(!orchestrator.evaluate_condition("exists:field", &state));

        state
            .node_results
            .push(serde_json::json!({"field": "value"}));
        assert!(orchestrator.evaluate_condition("exists:field", &state));
        assert!(!orchestrator.evaluate_condition("exists:missing", &state));
        assert!(orchestrator.evaluate_condition("truthy:field", &state));
    }
}
