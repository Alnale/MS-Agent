use tokio::sync::broadcast;

use crate::memory::MemoryKind;

/// Fine-grained memory change events for cross-cache coordination
#[derive(Debug, Clone)]
pub enum MemoryChangeEvent {
    /// New memory stored
    Stored {
        agent_id: String,
        session_id: Option<String>,
        memory_kind: MemoryKind,
        tags: Vec<String>,
        content_hash: String,
    },
    /// Existing memory updated
    Updated {
        agent_id: String,
        memory_id: String,
        tags: Vec<String>,
    },
    /// Memory invalidated (e.g. negative user feedback)
    Invalidated { agent_id: String, memory_id: String },
    /// Session ended — all caches for this session should be cleaned
    SessionEnded { session_id: String },
}

/// Global memory event bus for cross-agent cache coordination
///
/// Uses tokio broadcast channels so multiple listeners can subscribe
/// independently. Events are fire-and-forget — slow consumers will
/// miss events (lagged), which is acceptable for cache invalidation.
pub struct MemoryEventBus {
    sender: broadcast::Sender<MemoryChangeEvent>,
}

impl MemoryEventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Subscribe to memory change events
    pub fn subscribe(&self) -> broadcast::Receiver<MemoryChangeEvent> {
        self.sender.subscribe()
    }

    /// Publish a memory change event (fire-and-forget)
    pub fn publish(&self, event: MemoryChangeEvent) {
        let _ = self.sender.send(event);
    }
}

impl Default for MemoryEventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_event_bus_publish_subscribe() {
        let bus = MemoryEventBus::new(64);
        let mut rx = bus.subscribe();

        bus.publish(MemoryChangeEvent::Stored {
            agent_id: "knowledge".to_string(),
            session_id: Some("s1".to_string()),
            memory_kind: MemoryKind::UserFact,
            tags: vec!["test".to_string()],
            content_hash: "abc123".to_string(),
        });

        let event = rx.recv().await.unwrap();
        match event {
            MemoryChangeEvent::Stored { agent_id, .. } => {
                assert_eq!(agent_id, "knowledge");
            }
            _ => panic!("Expected Stored event"),
        }
    }

    #[tokio::test]
    async fn test_memory_event_bus_multiple_subscribers() {
        let bus = MemoryEventBus::new(64);
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        bus.publish(MemoryChangeEvent::Invalidated {
            agent_id: "sentiment".to_string(),
            memory_id: "m1".to_string(),
        });

        let event1 = rx1.recv().await.unwrap();
        let event2 = rx2.recv().await.unwrap();

        match (event1, event2) {
            (
                MemoryChangeEvent::Invalidated { agent_id: a1, .. },
                MemoryChangeEvent::Invalidated { agent_id: a2, .. },
            ) => {
                assert_eq!(a1, "sentiment");
                assert_eq!(a2, "sentiment");
            }
            _ => panic!("Expected Invalidated events"),
        }
    }

    #[tokio::test]
    async fn test_memory_event_bus_default_capacity() {
        let bus = MemoryEventBus::default();
        let mut rx = bus.subscribe();

        bus.publish(MemoryChangeEvent::Updated {
            agent_id: "knowledge".to_string(),
            memory_id: "m2".to_string(),
            tags: vec!["tag1".to_string()],
        });

        let event = rx.recv().await.unwrap();
        match event {
            MemoryChangeEvent::Updated { memory_id, .. } => {
                assert_eq!(memory_id, "m2");
            }
            _ => panic!("Expected Updated event"),
        }
    }
}
