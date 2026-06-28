use std::sync::Arc;

use agent_teams_core::context::AgentContext;
use agent_teams_core::effect::ReviewSeverity;
use agent_teams_core::provider::{ChatMessage, CompletionRequest, LlmProvider, ThinkingConfig};

/// Critic agent: reviews response quality and flags issues
pub struct CriticAgent {
    provider: Arc<dyn LlmProvider>,
    max_rounds: u8,
    default_model: String,
    thinking_enabled: bool,
    thinking_budget_tokens: u32,
}

impl CriticAgent {
    pub fn new(provider: Arc<dyn LlmProvider>, max_rounds: u8, default_model: &str) -> Self {
        Self {
            provider,
            max_rounds,
            default_model: default_model.to_string(),
            thinking_enabled: false,
            thinking_budget_tokens: 16384,
        }
    }

    pub fn with_thinking(mut self, enabled: bool, budget_tokens: u32) -> Self {
        self.thinking_enabled = enabled;
        self.thinking_budget_tokens = budget_tokens;
        self
    }

    fn thinking_config(&self) -> Option<ThinkingConfig> {
        if self.thinking_enabled {
            Some(ThinkingConfig {
                enabled: true,
                budget_tokens: self.thinking_budget_tokens,
                strategy: "Auto".to_string(),
            })
        } else {
            None
        }
    }

    pub fn max_rounds(&self) -> u8 {
        self.max_rounds
    }

    /// Multi-round critique: runs up to max_rounds, stopping early if no Critical issues found.
    pub async fn critique_multi_round(
        &self,
        ctx: &AgentContext,
        response: &str,
    ) -> Vec<(ReviewSeverity, String)> {
        let mut all_issues = Vec::new();
        let mut current_response = response.to_string();

        for _round in 0..self.max_rounds.max(1) {
            let issues = self.critique_single(ctx, &current_response).await;
            if issues.is_empty() {
                break;
            }

            let has_critical = issues
                .iter()
                .any(|(s, _)| matches!(s, ReviewSeverity::Critical));
            all_issues.extend(issues);

            if !has_critical {
                break;
            }

            // For critical issues, attempt to fix and re-review
            current_response = self.attempt_fix(ctx, &current_response, &all_issues).await;
        }

        all_issues
    }

    /// Single-round critique with optional SubAgent context
    pub async fn critique_with_context(
        &self,
        _ctx: &AgentContext,
        response: &str,
        sub_agent_results: &[(String, String)],
    ) -> Vec<(ReviewSeverity, String)> {
        let mut system = "你是一个质量审查员。请检查以下响应是否存在矛盾、事实错误或遗漏。\n\n\
             审查维度：\n\
             1. **事实准确性**：响应中的信息是否与各 Agent 的分析结果一致\n\
             2. **完整性**：是否遗漏了 Agent 分析中的重要信息\n\
             3. **一致性**：不同 Agent 的结果之间是否存在矛盾\n\
             4. **可操作性**：用户是否能根据响应采取下一步行动\n\n".to_string();

        if !sub_agent_results.is_empty() {
            system.push_str("## 各 Agent 原始分析结果（用于交叉验证）\n");
            for (id, content) in sub_agent_results {
                let preview: String = content.chars().take(500).collect();
                system.push_str(&format!("- {}: {}\n", id, preview));
            }
            system.push('\n');
        }

        system.push_str("如果发现问题，返回 JSON：{\"issues\": [{\"severity\": \"Warning|Critical\", \"message\": \"描述\"}]}。\n\
             如果没有问题，返回：{\"issues\": []}");

        self.do_critique(response, system).await
    }

    /// Single-round critique (legacy, without SubAgent context)
    async fn critique_single(
        &self,
        _ctx: &AgentContext,
        response: &str,
    ) -> Vec<(ReviewSeverity, String)> {
        let system = "你是一个质量审查员。请检查以下响应是否存在矛盾、事实错误或遗漏。\
             如果发现问题，返回 JSON：{\"issues\": [{\"severity\": \"Warning|Critical\", \"message\": \"描述\"}]}。\
             如果没有问题，返回：{\"issues\": []}".to_string();

        self.do_critique(response, system).await
    }

    /// Shared critique implementation
    async fn do_critique(
        &self,
        response: &str,
        system: String,
    ) -> Vec<(ReviewSeverity, String)> {
        // When thinking is enabled, max_tokens must exceed budget_tokens
        let thinking = self.thinking_config();
        let thinking_budget = thinking.as_ref().filter(|t| t.enabled).map(|t| t.budget_tokens).unwrap_or(0);
        let max_tokens = if thinking_budget > 0 {
            (thinking_budget + 8192).max(32768)
        } else {
            16384
        };

        let request = CompletionRequest {
            model: self.default_model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: response.to_string(),
                cache_control: None,
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: Some(max_tokens),
            temperature: Some(0.1),
            system: Some(system),
            stream: false,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking,
        };

        match self.provider.complete(request).await {
            Ok(resp) => {
                match serde_json::from_str::<serde_json::Value>(&resp.content) {
                    Ok(parsed) => parsed["issues"]
                        .as_array()
                        .unwrap_or(&vec![])
                        .iter()
                        .filter_map(|issue| {
                            let severity = match issue["severity"].as_str()? {
                                "Critical" => ReviewSeverity::Critical,
                                "Warning" => ReviewSeverity::Warning,
                                _ => ReviewSeverity::Info,
                            };
                            let message = issue["message"].as_str()?.to_string();
                            Some((severity, message))
                        })
                        .collect(),
                    Err(e) => {
                        tracing::warn!("Critic agent returned malformed JSON: {}. Raw response: {}", e, &resp.content[..resp.content.len().min(200)]);
                        vec![]
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Critic agent error: {}", e);
                vec![]
            }
        }
    }

    /// Attempt to fix critical issues in the response
    async fn attempt_fix(
        &self,
        _ctx: &AgentContext,
        response: &str,
        issues: &[(ReviewSeverity, String)],
    ) -> String {
        let issues_text: Vec<String> = issues
            .iter()
            .filter(|(s, _)| matches!(s, ReviewSeverity::Critical))
            .map(|(_, msg)| msg.clone())
            .collect();

        let prompt = format!(
            "请修复以下响应中的问题：\n\n原响应：\n{}\n\n发现的问题：\n{}\n\n请返回修复后的完整响应。",
            response,
            issues_text.join("\n- ")
        );

        let request = CompletionRequest {
            model: self.default_model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt,
                cache_control: None,
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: Some(16384),
            temperature: Some(0.3),
            system: Some(
                "你是一个响应修复专家。根据审查意见修复响应中的问题，保持原意。".to_string(),
            ),
            stream: false,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
        };

        match self.provider.complete(request).await {
            Ok(resp) => resp.content,
            Err(e) => {
                tracing::warn!("Critic fix attempt failed: {}", e);
                response.to_string()
            }
        }
    }
}
