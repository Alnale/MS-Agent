use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::error::Result;

/// Envelope for bus messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEnvelope {
    pub id: String,
    pub from: String,
    pub to: AgentTarget,
    pub payload: BusPayload,
    pub reply_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentTarget {
    Agent(String),
    Topic(String),
    Broadcast,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BusPayload {
    Task {
        content: String,
        message_type: String,
        context: Option<Value>,
    },
    Result {
        content: String,
        effects: Vec<Value>,
        quality: f32,
    },
    Event {
        event_type: String,
        data: Value,
    },
    Heartbeat,
    Custom {
        payload_type: String,
        data: Value,
    },
}

/// Agent communication bus trait
#[async_trait]
pub trait AgentBus: Send + Sync {
    /// Publish a message to the bus
    async fn publish(&self, envelope: AgentEnvelope) -> Result<()>;

    /// Subscribe an agent to receive messages
    async fn subscribe(&self, agent_id: String) -> mpsc::Receiver<AgentEnvelope>;

    /// Subscribe to a topic
    async fn subscribe_topic(&self, agent_id: String, topic: String) -> Result<()>;

    /// Send a request and wait for a response
    async fn request(&self, envelope: AgentEnvelope, timeout_ms: u64) -> Result<AgentEnvelope>;

    /// Unsubscribe an agent
    async fn unsubscribe(&self, agent_id: &str) -> Result<()>;
}
