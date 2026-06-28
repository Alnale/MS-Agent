use agent_teams_core::memory::{self, MemoryEntry, MemoryEntryBuilder, MemoryKind};

/// Maximum characters to keep when truncating content for memory storage
const MEMORY_CONTENT_MAX_LEN: usize = 500;

/// Build a MemoryEntry for an agent's output (used by both sync and streaming paths)
pub fn build_agent_output_memory_entry(
    agent_id: &str,
    content: &str,
    session_id: &str,
    quality: f32,
) -> MemoryEntry {
    MemoryEntryBuilder::new(
        MemoryKind::AgentOutput,
        content.chars().take(MEMORY_CONTENT_MAX_LEN).collect::<String>(),
        agent_id.to_string(),
    )
    .session_id(session_id)
    .data(serde_json::json!({
        "agent_id": agent_id,
        "quality": quality,
    }))
    .weight(quality * 0.5)
    .tags(vec![agent_id.to_string(), "agent_output".to_string()])
    .content_hash(memory::compute_content_hash(content))
    .confidence(quality)
    .build()
}

/// Build a MemoryEntry for the main agent's synthesis output
pub fn build_main_agent_memory_entry(
    narrative: &str,
    session_id: &str,
    effects_count: usize,
) -> MemoryEntry {
    MemoryEntryBuilder::new(
        MemoryKind::Summary,
        narrative.chars().take(MEMORY_CONTENT_MAX_LEN).collect::<String>(),
        "main_agent",
    )
    .session_id(session_id)
    .data(serde_json::json!({
        "agent_id": "main_agent",
        "effects_count": effects_count,
    }))
    .weight(0.7)
    .tags(vec!["main_agent".to_string(), "synthesis".to_string()])
    .content_hash(memory::compute_content_hash(narrative))
    .confidence(0.85)
    .build()
}
