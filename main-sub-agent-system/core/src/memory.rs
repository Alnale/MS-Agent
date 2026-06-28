use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hash::fnv1a_hash_str;

/// Compute content hash for deduplication
pub fn compute_content_hash(content: &str) -> String {
    fnv1a_hash_str(&[content])
}

/// Memory type classification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MemoryKind {
    /// Raw dialogue turn
    DialogueTurn,
    /// User-stated fact
    UserFact,
    /// System-inferred preference
    InferredPreference,
    /// Intermediate summary
    Summary,
    /// User profile attribute
    UserProfile,
    /// Cross-session topic
    CrossSessionTopic,
    /// Agent output (should NOT be retrieved as user knowledge)
    AgentOutput,
}

impl MemoryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DialogueTurn => "DialogueTurn",
            Self::UserFact => "UserFact",
            Self::InferredPreference => "InferredPreference",
            Self::Summary => "Summary",
            Self::UserProfile => "UserProfile",
            Self::CrossSessionTopic => "CrossSessionTopic",
            Self::AgentOutput => "AgentOutput",
        }
    }

}

impl FromStr for MemoryKind {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "DialogueTurn" => Ok(Self::DialogueTurn),
            "UserFact" => Ok(Self::UserFact),
            "InferredPreference" => Ok(Self::InferredPreference),
            "Summary" => Ok(Self::Summary),
            "UserProfile" => Ok(Self::UserProfile),
            "CrossSessionTopic" => Ok(Self::CrossSessionTopic),
            "AgentOutput" => Ok(Self::AgentOutput),
            _ => Err(format!("Unknown MemoryKind: {}", s)),
        }
    }
}

impl std::fmt::Display for MemoryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Unified memory entry for both short-term and long-term memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub session_id: Option<String>,
    pub kind: MemoryKind,
    /// Text content
    pub content: String,
    /// Structured data (optional)
    pub data: Option<Value>,
    /// Vector embedding for semantic retrieval (optional, required for long-term)
    pub embedding: Option<Vec<f32>>,
    /// Importance weight (0.0 - 1.0)
    pub weight: f32,
    pub created_at: DateTime<Utc>,
    /// Last access time (for LRU decay)
    pub last_accessed_at: DateTime<Utc>,
    /// Access count (popularity)
    pub access_count: u32,
    /// Associated entity/topic tags
    pub tags: Vec<String>,
    /// Source agent that produced this memory
    pub source_agent: String,
    /// Whether the user has explicitly confirmed this fact
    pub confirmed: bool,
    /// Content hash for idempotent deduplication (FNV-1a)
    pub content_hash: Option<String>,
    /// Confidence level (0.0-1.0), affected by contradictions and negative feedback
    pub confidence: f32,
    /// Parent memory ID (the compressed summary this was archived into)
    pub parent_id: Option<String>,
    /// Version number (incremented on updates)
    pub version: u32,
    /// Whether this memory has been archived (soft-deleted after compression)
    pub archived: bool,
    /// IDs of original memories this was compressed from
    pub compressed_from: Vec<String>,
}

/// Builder for MemoryEntry with sensible defaults.
///
/// Defaults: id=UUID, session_id=None, embedding=None, access_count=0,
/// confirmed=false, content_hash=None, confidence=1.0, parent_id=None,
/// version=1, archived=false, compressed_from=[], timestamps=now.
pub struct MemoryEntryBuilder {
    id: String,
    session_id: Option<String>,
    kind: MemoryKind,
    content: String,
    data: Option<Value>,
    embedding: Option<Vec<f32>>,
    weight: f32,
    tags: Vec<String>,
    source_agent: String,
    confirmed: bool,
    content_hash: Option<String>,
    confidence: f32,
    parent_id: Option<String>,
    version: u32,
    archived: bool,
    compressed_from: Vec<String>,
}

impl MemoryEntryBuilder {
    pub fn new(kind: MemoryKind, content: impl Into<String>, source_agent: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: None,
            kind,
            content: content.into(),
            data: None,
            embedding: None,
            weight: 0.5,
            tags: vec![],
            source_agent: source_agent.into(),
            confirmed: false,
            content_hash: None,
            confidence: 1.0,
            parent_id: None,
            version: 1,
            archived: false,
            compressed_from: vec![],
        }
    }

    pub fn id(mut self, id: impl Into<String>) -> Self { self.id = id.into(); self }
    pub fn session_id(mut self, session_id: impl Into<String>) -> Self { self.session_id = Some(session_id.into()); self }
    pub fn data(mut self, data: Value) -> Self { self.data = Some(data); self }
    pub fn embedding(mut self, embedding: Vec<f32>) -> Self { self.embedding = Some(embedding); self }
    pub fn weight(mut self, weight: f32) -> Self { self.weight = weight; self }
    pub fn tags(mut self, tags: Vec<String>) -> Self { self.tags = tags; self }
    pub fn confirmed(mut self, confirmed: bool) -> Self { self.confirmed = confirmed; self }
    pub fn content_hash(mut self, hash: impl Into<String>) -> Self { self.content_hash = Some(hash.into()); self }
    pub fn confidence(mut self, confidence: f32) -> Self { self.confidence = confidence; self }

    pub fn build(self) -> MemoryEntry {
        MemoryEntry {
            id: self.id,
            session_id: self.session_id,
            kind: self.kind,
            content: self.content,
            data: self.data,
            embedding: self.embedding,
            weight: self.weight,
            created_at: Utc::now(),
            last_accessed_at: Utc::now(),
            access_count: 0,
            tags: self.tags,
            source_agent: self.source_agent,
            confirmed: self.confirmed,
            content_hash: self.content_hash,
            confidence: self.confidence,
            parent_id: self.parent_id,
            version: self.version,
            archived: self.archived,
            compressed_from: self.compressed_from,
        }
    }
}

/// Memory retrieval request
#[derive(Debug, Clone)]
pub struct MemoryQuery {
    /// Text query (for semantic similarity)
    pub text: String,
    /// Pre-computed vector embedding (optional)
    pub embedding: Option<Vec<f32>>,
    /// Memory type filter
    pub kinds: Vec<MemoryKind>,
    /// Tag filter
    pub tags: Vec<String>,
    /// Session filter (None = cross-session)
    pub session_id: Option<String>,
    /// Time range filter
    pub since: Option<DateTime<Utc>>,
    /// Max results
    pub limit: usize,
    /// Minimum weight threshold
    pub min_weight: f32,
    /// Minimum cosine similarity for vector search (0.0-1.0)
    pub similarity_threshold: f32,
    /// User ID filter for data isolation (None = no filter)
    pub user_id: Option<String>,
    /// If true, only return entries where confirmed == true.
    /// Use this to prevent unverified agent outputs from being treated as facts.
    pub confirmed_only: bool,
}

impl Default for MemoryQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            embedding: None,
            kinds: Vec::new(),
            tags: Vec::new(),
            session_id: None,
            since: None,
            limit: 20,
            min_weight: 0.0,
            similarity_threshold: 0.7,
            user_id: None,
            confirmed_only: false,
        }
    }
}

/// Memory retrieval result
#[derive(Debug, Clone)]
pub struct MemoryRetrievalResult {
    pub entries: Vec<MemoryEntry>,
    pub total_available: usize,
}

/// Compression strategy
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CompressionStrategy {
    /// Extract key facts
    ExtractFacts,
    /// Generate summary
    Summarize,
    /// Build user profile update
    UpdateProfile,
    /// Topic clustering
    ClusterTopics,
}

/// Memory importance level for tiered storage and decay
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MemoryImportance {
    /// Core identity/security (never delete)
    Critical,
    /// User-confirmed important preferences
    High,
    /// General facts and preferences
    Medium,
    /// Temporary context
    Low,
    /// Disposable chatter
    Ephemeral,
}

impl MemoryImportance {
    pub fn prune_threshold(&self) -> f32 {
        match self {
            Self::Critical => 0.0, // never delete
            Self::High => 0.05,
            Self::Medium => 0.15,
            Self::Low => 0.3,
            Self::Ephemeral => 0.5,
        }
    }

    pub fn decay_factor(&self) -> f32 {
        match self {
            Self::Critical => 0.999, // almost no decay
            Self::High => 0.99,
            Self::Medium => 0.95,
            Self::Low => 0.90,
            Self::Ephemeral => 0.80,
        }
    }
}

impl MemoryKind {
    /// Default importance level for each memory kind
    pub fn default_importance(&self) -> MemoryImportance {
        match self {
            Self::UserProfile => MemoryImportance::Critical,
            Self::UserFact => MemoryImportance::High,
            Self::InferredPreference => MemoryImportance::Medium,
            Self::CrossSessionTopic => MemoryImportance::Medium,
            Self::Summary => MemoryImportance::Low,
            Self::DialogueTurn => MemoryImportance::Ephemeral,
            Self::AgentOutput => MemoryImportance::Low,
        }
    }
}

/// Memory relation type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MemoryRelationType {
    /// Contradiction (new fact conflicts with old)
    Contradicts,
    /// Support (multiple sources confirm same fact)
    Supports,
    /// Supersedes (new fact replaces old)
    Supersedes,
    /// Related (same topic, different facts)
    Related,
}

impl MemoryRelationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Contradicts => "contradicts",
            Self::Supports => "supports",
            Self::Supersedes => "supersedes",
            Self::Related => "related",
        }
    }

}

impl FromStr for MemoryRelationType {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "contradicts" => Ok(Self::Contradicts),
            "supports" => Ok(Self::Supports),
            "supersedes" => Ok(Self::Supersedes),
            "related" => Ok(Self::Related),
            _ => Err(format!("Unknown MemoryRelationType: {}", s)),
        }
    }
}

/// Memory relation between two entries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRelation {
    pub source_id: String,
    pub target_id: String,
    pub relation_type: MemoryRelationType,
    pub strength: f32,
    pub created_at: DateTime<Utc>,
}

/// Memory system events for event-driven updates
#[derive(Debug, Clone)]
pub enum MemoryEvent {
    /// New dialogue turn recorded
    TurnRecorded {
        session_id: String,
        turn_count: usize,
    },
    /// User fact confirmed
    FactConfirmed { memory_id: String },
    /// Summary generation completed
    SummaryProduced { session_id: String, quality: f32 },
    /// User feedback received
    UserFeedback { memory_id: String, positive: bool },
    /// Contradiction detected
    ContradictionDetected {
        source_id: String,
        target_id: String,
    },
}

/// Memory event handler trait
#[async_trait::async_trait]
pub trait MemoryEventHandler: Send + Sync {
    async fn handle(&self, event: MemoryEvent) -> crate::error::Result<()>;
}

/// Memory system configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Max entries in working memory
    pub working_memory_limit: usize,
    /// Short-term memory TTL (seconds)
    pub short_term_ttl_secs: u64,
    /// Compression threshold (turn count)
    pub compression_threshold: usize,
    /// Max entries returned by retrieval
    pub retrieval_limit: usize,
    /// Semantic similarity threshold
    pub similarity_threshold: f32,
    /// Daily weight decay factor
    pub daily_decay_factor: f32,
    /// Whether memory system is enabled
    pub enabled: bool,
    /// Cosine similarity threshold for deduplication (0.0-1.0)
    pub duplicate_similarity_threshold: f32,
    /// Max tokens for working memory (Token budget)
    pub working_memory_max_tokens: usize,
    /// Half-life days for weight decay
    pub decay_halflife_days: f32,
    /// Enable contradiction detection when recording facts
    pub enable_contradiction_detection: bool,
    /// Embedding cache size (number of entries)
    pub embedding_cache_size: usize,
    /// Session cache TTL (seconds) — how long working memory is cached per session
    pub session_cache_ttl_secs: u64,
    /// L1 hot cache max size per agent (0 = unlimited)
    pub hot_cache_max_size: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            working_memory_limit: 50,
            short_term_ttl_secs: 86400, // 24h
            compression_threshold: 20,
            retrieval_limit: 20,
            similarity_threshold: 0.7,
            daily_decay_factor: 0.95,
            enabled: true,
            duplicate_similarity_threshold: 0.92,
            working_memory_max_tokens: 2000,
            decay_halflife_days: 7.0,
            enable_contradiction_detection: true,
            embedding_cache_size: 10000,
            session_cache_ttl_secs: 30,
            hot_cache_max_size: 500,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_kind_roundtrip() {
        let kinds = vec![
            MemoryKind::DialogueTurn,
            MemoryKind::UserFact,
            MemoryKind::InferredPreference,
            MemoryKind::Summary,
            MemoryKind::UserProfile,
            MemoryKind::CrossSessionTopic,
        ];
        for kind in kinds {
            let s = kind.as_str();
            assert_eq!(s.parse::<MemoryKind>().ok(), Some(kind.clone()));
        }
    }

    #[test]
    fn test_memory_config_defaults() {
        let config = MemoryConfig::default();
        assert_eq!(config.working_memory_limit, 50);
        assert_eq!(config.short_term_ttl_secs, 86400);
        assert_eq!(config.compression_threshold, 20);
        assert!(config.enabled);
        assert_eq!(config.duplicate_similarity_threshold, 0.92);
        assert_eq!(config.working_memory_max_tokens, 2000);
        assert_eq!(config.decay_halflife_days, 7.0);
        assert!(config.enable_contradiction_detection);
        assert_eq!(config.embedding_cache_size, 10000);
    }

    #[test]
    fn test_memory_entry_serialization() {
        let entry = MemoryEntry {
            id: "test-id".to_string(),
            session_id: Some("session-1".to_string()),
            kind: MemoryKind::UserFact,
            content: "User prefers dark mode".to_string(),
            data: None,
            embedding: None,
            weight: 0.8,
            created_at: Utc::now(),
            last_accessed_at: Utc::now(),
            access_count: 1,
            tags: vec!["preference".to_string()],
            source_agent: "knowledge".to_string(),
            confirmed: true,
            content_hash: Some(compute_content_hash("User prefers dark mode")),
            confidence: 1.0,
            parent_id: None,
            version: 1,
            archived: false,
            compressed_from: vec![],
        };

        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: MemoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "test-id");
        assert_eq!(deserialized.kind, MemoryKind::UserFact);
        assert!(deserialized.confirmed);
    }
}
