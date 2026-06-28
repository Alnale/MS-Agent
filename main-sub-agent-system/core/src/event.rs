use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// System events published on the event bus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemEvent {
    ContentGenerated {
        agent_id: String,
        session_id: String,
    },
    TurnEnded {
        session_id: String,
        turn_count: u32,
    },
    PipelineCompleted {
        session_id: String,
        duration_ms: u64,
    },
    AgentCreated {
        agent_id: String,
    },
    AgentRemoved {
        agent_id: String,
    },
    ErrorOccurred {
        agent_id: String,
        error: String,
    },
    MemoryUpdated {
        memory_id: String,
        source_agent: String,
    },
    Custom {
        event_type: String,
        data: serde_json::Value,
    },
}

/// Simple event bus using tokio broadcast channels
pub struct EventBus {
    sender: broadcast::Sender<SystemEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn publish(&self, event: SystemEvent) {
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SystemEvent> {
        self.sender.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(256)
    }
}
