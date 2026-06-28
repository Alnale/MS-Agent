use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::AgentContext;
use crate::message::AgentMessage;

/// Routing table for rule-based agent selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingTable {
    pub rules: Vec<RoutingRule>,
}

impl RoutingTable {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn with_rules(mut self, rules: Vec<RoutingRule>) -> Self {
        self.rules = rules;
        // Sort by priority descending (higher priority evaluated first)
        self.rules.sort_by(|a, b| b.priority.cmp(&a.priority));
        self
    }

    /// Evaluate rules in priority order, return first match
    pub fn evaluate(&self, ctx: &AgentContext, msg: &AgentMessage) -> Option<RouteTarget> {
        for rule in &self.rules {
            if let Some(target) = rule.evaluate(ctx, msg) {
                return Some(target);
            }
        }
        None
    }

    /// Merge another routing table (other's rules appended, then re-sorted by priority)
    pub fn merge(&mut self, other: RoutingTable) {
        self.rules.extend(other.rules);
        self.rules.sort_by(|a, b| b.priority.cmp(&a.priority));
    }
}

impl Default for RoutingTable {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRule {
    pub name: String,
    pub condition: RouteCondition,
    pub target: RouteTarget,
    pub priority: i32,
}

impl RoutingRule {
    pub fn evaluate(&self, ctx: &AgentContext, msg: &AgentMessage) -> Option<RouteTarget> {
        if self.condition.matches(ctx, msg) {
            Some(self.target.clone())
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RouteCondition {
    MessageType(String),
    ContentContains(String),
    Always,
    Custom {
        condition_type: String,
        params: Value,
    },
}

impl RouteCondition {
    pub fn matches(&self, _ctx: &AgentContext, msg: &AgentMessage) -> bool {
        match self {
            Self::MessageType(t) => msg.message_type == *t,
            Self::ContentContains(pattern) => msg.content.contains(pattern),
            Self::Always => true,
            Self::Custom { .. } => false, // Custom conditions need external evaluation
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteTarget {
    pub agent_id: String,
    pub mode: RouteMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RouteMode {
    Direct,
    WithFallback(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> AgentContext {
        AgentContext::default()
    }

    fn test_msg(msg_type: &str, content: &str) -> AgentMessage {
        AgentMessage::new(content).with_type(msg_type)
    }

    #[test]
    fn test_message_type_routing() {
        let table = RoutingTable::new().with_rules(vec![RoutingRule {
            name: "test".to_string(),
            condition: RouteCondition::MessageType("knowledge_query".to_string()),
            target: RouteTarget {
                agent_id: "knowledge".to_string(),
                mode: RouteMode::Direct,
            },
            priority: 100,
        }]);

        let ctx = test_ctx();
        let msg = test_msg("knowledge_query", "test");
        let result = table.evaluate(&ctx, &msg);
        assert!(result.is_some());
        assert_eq!(result.unwrap().agent_id, "knowledge");
    }

    #[test]
    fn test_content_contains_routing() {
        let table = RoutingTable::new().with_rules(vec![RoutingRule {
            name: "test".to_string(),
            condition: RouteCondition::ContentContains("价格".to_string()),
            target: RouteTarget {
                agent_id: "knowledge".to_string(),
                mode: RouteMode::Direct,
            },
            priority: 100,
        }]);

        let ctx = test_ctx();
        let msg = test_msg("user_input", "这个产品的价格是多少?");
        let result = table.evaluate(&ctx, &msg);
        assert!(result.is_some());
    }

    #[test]
    fn test_priority_ordering() {
        let table = RoutingTable::new().with_rules(vec![
            RoutingRule {
                name: "low".to_string(),
                condition: RouteCondition::Always,
                target: RouteTarget {
                    agent_id: "low_agent".to_string(),
                    mode: RouteMode::Direct,
                },
                priority: 10,
            },
            RoutingRule {
                name: "high".to_string(),
                condition: RouteCondition::Always,
                target: RouteTarget {
                    agent_id: "high_agent".to_string(),
                    mode: RouteMode::Direct,
                },
                priority: 100,
            },
        ]);

        let ctx = test_ctx();
        let msg = test_msg("test", "test");
        let result = table.evaluate(&ctx, &msg);
        assert!(result.is_some());
        assert_eq!(result.unwrap().agent_id, "high_agent");
    }

    #[test]
    fn test_no_match() {
        let table = RoutingTable::new().with_rules(vec![RoutingRule {
            name: "test".to_string(),
            condition: RouteCondition::MessageType("specific_type".to_string()),
            target: RouteTarget {
                agent_id: "agent".to_string(),
                mode: RouteMode::Direct,
            },
            priority: 100,
        }]);

        let ctx = test_ctx();
        let msg = test_msg("other_type", "test");
        let result = table.evaluate(&ctx, &msg);
        assert!(result.is_none());
    }
}
