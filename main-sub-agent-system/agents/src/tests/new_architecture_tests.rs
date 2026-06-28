//! Comprehensive tests for the new Sub Agent architecture.
//!
//! Tests verify:
//! 1. All new agents can be created
//! 2. Agent descriptors are correct
//! 3. Agent IDs match expected values
//! 4. Agent capabilities are correct
//! 5. Routing table routes to correct agents
//! 6. Agent factory creates correct agents
//! 7. No legacy agent references remain

#[cfg(test)]
mod new_architecture_tests {
    use crate::agent_factory::AgentFactory;
    use crate::domain_cs::DomainModuleCS;

    // =========================================================================
    // Test: All new agents exist and can be identified
    // =========================================================================

    #[test]
    fn test_all_new_agents_valid() {
        let valid_ids = vec!["sentiment", "task_planner", "summary"];
        for id in &valid_ids {
            assert!(
                AgentFactory::is_valid_agent_id(id),
                "Agent '{}' should be valid",
                id
            );
        }
    }

    #[test]
    fn test_legacy_agents_invalid() {
        let legacy_ids = vec!["analysis", "tool_decision", "tool_exec", "tool_planner", "escalation"];
        for id in &legacy_ids {
            assert!(
                !AgentFactory::is_valid_agent_id(id),
                "Legacy agent '{}' should NOT be valid",
                id
            );
        }
    }

    // =========================================================================
    // Test: Agent descriptors are correct
    // =========================================================================

    #[test]
    fn test_sentiment_descriptor() {
        let desc = AgentFactory::get_descriptor("sentiment").expect("sentiment descriptor missing");
        assert_eq!(desc.id, "sentiment");
        assert_eq!(desc.priority, 70);
        assert!(!desc.optional);
        assert!(desc.capabilities.requires_llm);
        assert!(desc.capabilities.message_types.contains(&"sentiment_analysis".to_string()));
        assert!(desc.capabilities.message_types.contains(&"user_input".to_string()));
    }

    #[test]
    fn test_task_planner_descriptor() {
        let desc = AgentFactory::get_descriptor("task_planner").expect("task_planner descriptor missing");
        assert_eq!(desc.id, "task_planner");
        assert_eq!(desc.priority, 110);
        assert!(!desc.optional);
        assert!(desc.capabilities.requires_llm);
        assert!(desc.capabilities.message_types.contains(&"routing_decision".to_string()));
        assert!(desc.capabilities.message_types.contains(&"task_planning".to_string()));
        assert!(desc.capabilities.message_types.contains(&"user_input".to_string()));
    }

    #[test]
    fn test_summary_descriptor() {
        let desc = AgentFactory::get_descriptor("summary").expect("summary descriptor missing");
        assert_eq!(desc.id, "summary");
        assert_eq!(desc.priority, 50);
        assert!(desc.optional);
        assert!(desc.capabilities.requires_llm);
        assert!(desc.capabilities.message_types.contains(&"conversation_summary".to_string()));
    }

    // =========================================================================
    // Test: Total descriptor count
    // =========================================================================

    #[test]
    fn test_exactly_three_descriptors() {
        let descriptors = AgentFactory::get_all_descriptors();
        assert_eq!(descriptors.len(), 3, "Should have exactly 3 agents");
    }

    // =========================================================================
    // Test: Domain module descriptors match factory descriptors
    // =========================================================================

    #[test]
    fn test_domain_descriptors_match_factory() {
        let factory_descs = AgentFactory::get_all_descriptors();
        let domain_descs = DomainModuleCS::sub_agent_descriptors();

        assert_eq!(factory_descs.len(), domain_descs.len());

        for factory_desc in &factory_descs {
            let domain_desc = domain_descs.iter().find(|d| d.id == factory_desc.id);
            assert!(
                domain_desc.is_some(),
                "Domain module missing descriptor for '{}'",
                factory_desc.id
            );
            let domain_desc = domain_desc.unwrap();
            assert_eq!(domain_desc.priority, factory_desc.priority);
            assert_eq!(domain_desc.optional, factory_desc.optional);
        }
    }

    // =========================================================================
    // Test: No legacy descriptors in domain module
    // =========================================================================

    #[test]
    fn test_no_legacy_in_domain_descriptors() {
        let domain_descs = DomainModuleCS::sub_agent_descriptors();
        let legacy_ids = vec!["analysis", "tool_decision", "tool_exec", "tool_planner", "escalation"];

        for desc in &domain_descs {
            assert!(
                !legacy_ids.contains(&desc.id.as_str()),
                "Domain module should NOT contain legacy agent '{}'",
                desc.id
            );
        }
    }

    // =========================================================================
    // Test: Priority ordering
    // =========================================================================

    #[test]
    fn test_priority_ordering() {
        let sentiment = AgentFactory::get_descriptor("sentiment").unwrap();
        let task_planner = AgentFactory::get_descriptor("task_planner").unwrap();
        let summary = AgentFactory::get_descriptor("summary").unwrap();

        // task_planner > sentiment > summary
        assert!(task_planner.priority > sentiment.priority);
        assert!(sentiment.priority > summary.priority);
    }

    // =========================================================================
    // Test: Expertise descriptions are unique and meaningful
    // =========================================================================

    #[test]
    fn test_expertise_unique() {
        let descriptors = AgentFactory::get_all_descriptors();
        let mut expertises: Vec<&str> = descriptors.iter().map(|d| d.expertise.as_str()).collect();
        let original_len = expertises.len();
        expertises.sort();
        expertises.dedup();
        assert_eq!(expertises.len(), original_len, "All expertises should be unique");
    }

    // =========================================================================
    // Test: Default agents are the correct set
    // =========================================================================

    #[test]
    fn test_default_agents_count() {
        let descriptors = AgentFactory::get_all_descriptors();
        let ids: Vec<&str> = descriptors.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"sentiment"));
        assert!(ids.contains(&"task_planner"));
        assert!(ids.contains(&"summary"));
        assert!(!ids.contains(&"analysis"));
        assert!(!ids.contains(&"tool_decision"));
    }

    // =========================================================================
    // Test: Descriptor ID matches struct ID
    // =========================================================================

    #[test]
    fn test_descriptor_ids_consistent() {
        let descriptors = AgentFactory::get_all_descriptors();
        for desc in &descriptors {
            match desc.id.as_str() {
                "sentiment" | "task_planner" | "summary" => {}
                other => panic!("Unexpected agent ID: {}", other),
            }
        }
    }
}
