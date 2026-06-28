use std::collections::HashSet;

use agent_teams_core::effect::AgentEffect;

/// Effect aggregator: deduplicates and merges effects from multiple agents
pub struct EffectAggregator;

impl EffectAggregator {
    pub fn new() -> Self {
        Self
    }

    /// Aggregate effects from multiple agents.
    /// InfoFragments are deduplicated by hash_key and sorted by priority.
    /// TaskItems are deduplicated by task_id.
    /// All other effects are kept as-is.
    pub fn aggregate(&self, effects: Vec<Vec<AgentEffect>>) -> Vec<AgentEffect> {
        let all: Vec<AgentEffect> = effects.into_iter().flatten().collect();

        let mut result = Vec::new();
        let mut info_fragments = Vec::new();
        let mut seen_hashes = HashSet::new();
        let mut seen_tasks = HashSet::new();

        for effect in all {
            match &effect {
                AgentEffect::InfoFragment { hash_key, .. } => {
                    if let Some(key) = hash_key {
                        if seen_hashes.insert(key.clone()) {
                            info_fragments.push(effect);
                        }
                    } else {
                        info_fragments.push(effect);
                    }
                }
                AgentEffect::TaskItem { task_id, .. } => {
                    if seen_tasks.insert(task_id.clone()) {
                        result.push(effect);
                    }
                }
                _ => result.push(effect),
            }
        }

        // Sort InfoFragments by priority (lower number = higher priority)
        info_fragments.sort_by_key(|e| match e {
            AgentEffect::InfoFragment { priority, .. } => *priority,
            _ => i32::MAX,
        });

        result.extend(info_fragments);
        result
    }
}

impl Default for EffectAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dedup_info_fragments_by_hash_key() {
        let aggregator = EffectAggregator::new();
        let effects = vec![vec![
            AgentEffect::InfoFragment {
                content: "first".to_string(),
                agent_id: "a1".to_string(),
                priority: 1,
                hash_key: Some("key1".to_string()),
                category: None,
            },
            AgentEffect::InfoFragment {
                content: "duplicate".to_string(),
                agent_id: "a2".to_string(),
                priority: 2,
                hash_key: Some("key1".to_string()),
                category: None,
            },
        ]];
        let result = aggregator.aggregate(effects);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0],
            AgentEffect::InfoFragment {
                content: "first".to_string(),
                agent_id: "a1".to_string(),
                priority: 1,
                hash_key: Some("key1".to_string()),
                category: None,
            }
        );
    }

    #[test]
    fn test_sort_info_fragments_by_priority() {
        let aggregator = EffectAggregator::new();
        let effects = vec![vec![
            AgentEffect::InfoFragment {
                content: "low".to_string(),
                agent_id: "a1".to_string(),
                priority: 100,
                hash_key: None,
                category: None,
            },
            AgentEffect::InfoFragment {
                content: "high".to_string(),
                agent_id: "a2".to_string(),
                priority: 1,
                hash_key: None,
                category: None,
            },
        ]];
        let result = aggregator.aggregate(effects);
        assert_eq!(result.len(), 2);
        // Higher priority (lower number) should come first
        assert_eq!(
            result[0],
            AgentEffect::InfoFragment {
                content: "high".to_string(),
                agent_id: "a2".to_string(),
                priority: 1,
                hash_key: None,
                category: None,
            }
        );
    }

    #[test]
    fn test_dedup_task_items() {
        let aggregator = EffectAggregator::new();
        let effects = vec![vec![
            AgentEffect::TaskItem {
                task_id: "t1".to_string(),
                description: "first".to_string(),
                status: "pending".to_string(),
                agent_id: "a1".to_string(),
            },
            AgentEffect::TaskItem {
                task_id: "t1".to_string(),
                description: "duplicate".to_string(),
                status: "done".to_string(),
                agent_id: "a2".to_string(),
            },
        ]];
        let result = aggregator.aggregate(effects);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_other_effects_preserved() {
        let aggregator = EffectAggregator::new();
        let effects = vec![vec![
            AgentEffect::TextChange {
                field: "name".to_string(),
                value: "test".to_string(),
                agent_id: "a1".to_string(),
            },
            AgentEffect::NumericChange {
                field: "score".to_string(),
                delta: 1.5,
                agent_id: "a2".to_string(),
            },
        ]];
        let result = aggregator.aggregate(effects);
        assert_eq!(result.len(), 2);
    }
}
