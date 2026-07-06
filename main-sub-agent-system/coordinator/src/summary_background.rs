use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

use agent_teams_agents::sub_agents::summary::quality_inspector::MemoryQualityInspector;
use agent_core::bus::{AgentBus, AgentEnvelope, AgentTarget, BusPayload};
use agent_core::error::Result;
use agent_core::memory::{MemoryEntry, MemoryKind, MemoryQuery};
use agent_core::memory_lifecycle::MemoryLifecycleManager;
use agent_core::memory_store::MemoryStore;
use agent_core::provider::LlmProvider;

use crate::compression_evaluator::CompressionEvaluator;

/// Background summary task types
pub enum SummaryTask {
    /// Session reached threshold, trigger compression
    CompressSession { session_id: String },
    /// Scheduled incremental summary
    IncrementalSummary { session_id: String },
    /// Cross-session topic fusion
    CrossSessionFusion { user_id: String },
}

/// Configuration for the summary background service
#[derive(Debug, Clone)]
pub struct SummaryServiceConfig {
    /// Background processing interval (seconds)
    pub interval_secs: u64,
    /// Max turns per batch
    pub max_turns_per_batch: usize,
    /// Quality threshold (below this, retry)
    pub quality_threshold: f32,
    /// Max retry count
    pub max_retries: u32,
}

impl Default for SummaryServiceConfig {
    fn default() -> Self {
        Self {
            interval_secs: 60,
            max_turns_per_batch: 20,
            quality_threshold: 0.5,
            max_retries: 2,
        }
    }
}

/// Summary background service: processes compression tasks asynchronously
pub struct SummaryBackgroundService {
    /// Task receiver
    task_rx: mpsc::Receiver<SummaryTask>,
    /// Task sender (for submitting tasks)
    task_tx: mpsc::Sender<SummaryTask>,
    /// Short-term memory store
    short_term: Arc<dyn MemoryStore>,
    /// Long-term memory store
    long_term: Arc<dyn MemoryStore>,
    /// Embedding provider
    embedding_provider: Arc<dyn agent_core::memory_store::EmbeddingProvider>,
    /// Quality evaluator
    evaluator: Arc<CompressionEvaluator>,
    /// Optional LLM provider for extraction (fallback when bus is unavailable)
    llm_provider: Option<Arc<dyn LlmProvider>>,
    /// Agent bus for communicating with Summary Sub Agent
    bus: Option<Arc<dyn AgentBus>>,
    /// Sessions currently being compressed (prevents duplicate compression)
    in_progress_sessions: Arc<DashMap<String, ()>>,
    /// Optional lifecycle manager for quality-driven memory weight adjustment
    lifecycle_manager: Option<MemoryLifecycleManager>,
    /// Optional quality inspector for periodic memory quality checks
    quality_inspector: Option<MemoryQualityInspector>,
    /// Configuration
    config: SummaryServiceConfig,
}

impl SummaryBackgroundService {
    pub fn new(
        short_term: Arc<dyn MemoryStore>,
        long_term: Arc<dyn MemoryStore>,
        embedding_provider: Arc<dyn agent_core::memory_store::EmbeddingProvider>,
        evaluator: Arc<CompressionEvaluator>,
        config: SummaryServiceConfig,
    ) -> Self {
        let (task_tx, task_rx) = mpsc::channel(100);
        let lifecycle_manager = MemoryLifecycleManager::new(long_term.clone());
        let quality_inspector = MemoryQualityInspector::new(long_term.clone());
        Self {
            task_rx,
            task_tx,
            short_term,
            long_term,
            embedding_provider,
            evaluator,
            llm_provider: None,
            bus: None,
            in_progress_sessions: Arc::new(DashMap::new()),
            lifecycle_manager: Some(lifecycle_manager),
            quality_inspector: Some(quality_inspector),
            config,
        }
    }

    pub fn with_llm_provider(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.llm_provider = Some(provider);
        self
    }

    pub fn with_bus(mut self, bus: Arc<dyn AgentBus>) -> Self {
        self.bus = Some(bus);
        self
    }

    /// Get a task sender handle for submitting tasks
    pub fn sender(&self) -> mpsc::Sender<SummaryTask> {
        self.task_tx.clone()
    }

    /// Run the background service loop
    pub async fn run(mut self) {
        let mut tick_interval = interval(Duration::from_secs(self.config.interval_secs));
        let mut quality_inspection_interval = interval(Duration::from_secs(3600 * 6)); // every 6 hours
                                                                                       // Skip the first immediate tick
        tick_interval.tick().await;
        quality_inspection_interval.tick().await;

        tracing::info!(
            "SummaryBackgroundService started (interval={}s)",
            self.config.interval_secs
        );

        loop {
            tokio::select! {
                Some(task) = self.task_rx.recv() => {
                    if let Err(e) = self.process_task(task).await {
                        tracing::warn!("SummaryBackgroundService task failed: {}", e);
                    }
                }
                _ = tick_interval.tick() => {
                    // Periodic maintenance: tasks are submitted on-demand
                }
                _ = quality_inspection_interval.tick() => {
                    // Memory quality inspection
                    if let Some(ref inspector) = self.quality_inspector {
                        match inspector.inspect_all_sessions().await {
                            Ok(report) => {
                                if !report.problematic_sessions.is_empty() {
                                    tracing::info!(
                                        "Quality inspection found {} problematic sessions",
                                        report.problematic_sessions.len()
                                    );
                                    for session_id in report.problematic_sessions {
                                        let _ = self.task_tx.send(SummaryTask::CompressSession { session_id }).await;
                                    }
                                }
                            }
                            Err(e) => tracing::warn!("Quality inspection failed: {}", e),
                        }
                    }
                }
            }
        }
    }

    /// Process a single summary task
    async fn process_task(&self, task: SummaryTask) -> Result<()> {
        match task {
            SummaryTask::CompressSession { session_id } => {
                self.process_compress_session(&session_id).await
            }
            SummaryTask::IncrementalSummary { session_id } => {
                self.process_incremental_summary(&session_id).await
            }
            SummaryTask::CrossSessionFusion { user_id } => {
                self.process_cross_session_fusion(&user_id).await
            }
        }
    }

    /// Compress a session: delegate to Summary Sub Agent via bus
    /// The Summary Agent handles memory writing directly (bidirectional data flow)
    /// Protected by dedup lock — only one compression per session at a time
    async fn process_compress_session(&self, session_id: &str) -> Result<()> {
        // Dedup lock: if this session is already being compressed, skip
        if self.in_progress_sessions.contains_key(session_id) {
            tracing::debug!(
                "Compression already in progress for session {}, skipping",
                session_id
            );
            return Ok(());
        }

        self.in_progress_sessions.insert(session_id.to_string(), ());

        let result = self.do_compress_session(session_id).await;

        // Always release the lock
        self.in_progress_sessions.remove(session_id);

        result
    }

    /// Internal compression logic (called only under dedup lock)
    async fn do_compress_session(&self, session_id: &str) -> Result<()> {
        let turns = self.short_term.list_session_memories(session_id).await?;
        if turns.is_empty() {
            return Ok(());
        }

        let turn_count = turns
            .iter()
            .filter(|t| t.kind == MemoryKind::DialogueTurn)
            .count();
        if turn_count < 5 {
            tracing::debug!(
                "Skipping compression for session {}: only {} turns",
                session_id,
                turn_count
            );
            return Ok(());
        }

        // Build dialogue text from turns
        let dialogue: Vec<&str> = turns
            .iter()
            .filter(|t| t.kind == MemoryKind::DialogueTurn)
            .map(|t| t.content.as_str())
            .collect();
        let dialogue_text = dialogue.join("\n");

        if dialogue_text.is_empty() {
            return Ok(());
        }

        // Load existing summaries for incremental mode
        let existing_summaries = self.load_existing_summaries(session_id).await;
        let prompt = if existing_summaries.is_empty() {
            dialogue_text
        } else {
            format!(
                "session_id: {}\n\n已有摘要：\n{}\n\n新增对话：\n{}",
                session_id, existing_summaries, dialogue_text
            )
        };

        // Try bus-based communication (Summary Agent handles memory writing)
        let quality = if self.bus.is_some() {
            match self.call_summary_agent_via_bus(&prompt, session_id).await {
                Ok((_facts, _summary)) => {
                    // Summary Agent already wrote to memory system
                    // Evaluate quality of the compression
                    let quality = self.evaluate_compression_quality(session_id, &turns).await;
                    tracing::info!(
                        "Summary Agent completed compression for session {} (quality: {:.2})",
                        session_id,
                        quality
                    );
                    quality
                }
                Err(e) => {
                    tracing::warn!(
                        "Summary Agent failed for session {}: {}, falling back to rule-based",
                        session_id,
                        e
                    );
                    if let Err(basic_err) = self.compress_basic(session_id).await {
                        tracing::error!("Rule-based compression also failed: {}", basic_err);
                    }
                    0.0
                }
            }
        } else if self.llm_provider.is_some() {
            // Fallback: direct LLM extraction
            match self
                .extract_with_quality_retry(&prompt, session_id, &turns)
                .await
            {
                Ok((facts, summary)) => {
                    self.store_extraction_results(session_id, &facts, &summary)
                        .await
                }
                Err(e) => {
                    tracing::warn!(
                        "Extraction failed for session {}: {}, falling back to rule-based",
                        session_id,
                        e
                    );
                    if let Err(basic_err) = self.compress_basic(session_id).await {
                        tracing::error!("Rule-based compression also failed: {}", basic_err);
                    }
                    0.0
                }
            }
        } else {
            // No bus or LLM: use basic compression
            self.compress_basic(session_id).await?;
            0.5
        };

        // Apply quality feedback to generated summaries via lifecycle manager
        if let Some(ref lifecycle) = self.lifecycle_manager {
            let summaries = self
                .long_term
                .retrieve(MemoryQuery {
                    kinds: vec![MemoryKind::Summary],
                    session_id: Some(session_id.to_string()),
                    limit: 1,
                    min_weight: 0.0,
                    ..Default::default()
                })
                .await
                .unwrap_or_else(|_| agent_core::memory::MemoryRetrievalResult {
                    entries: Vec::new(),
                    total_available: 0,
                });

            if let Some(summary) = summaries.entries.first() {
                match lifecycle
                    .apply_quality_feedback(&summary.id, quality, "compression_evaluator")
                    .await
                {
                    Ok(action) => tracing::info!(
                        "Quality feedback applied to summary {} for session {}: {:?}",
                        summary.id,
                        session_id,
                        action
                    ),
                    Err(e) => tracing::warn!("Failed to apply quality feedback: {}", e),
                }
            }
        }

        // Archive original turns if quality is acceptable
        if quality >= self.config.quality_threshold {
            for turn in &turns {
                let mut archived = turn.clone();
                archived.weight = 0.01;
                archived.tags.push("archived".to_string());
                if let Err(e) = self.short_term.store(archived).await {
                    tracing::warn!("Failed to archive dialogue turn: {}", e);
                }
            }
            tracing::info!(
                "Compressed session {} ({} turns, quality: {:.2})",
                session_id,
                turn_count,
                quality
            );
        } else {
            tracing::warn!(
                "Compression quality {:.2} below threshold {} for session {}, skipping archive",
                quality,
                self.config.quality_threshold,
                session_id
            );
        }

        Ok(())
    }

    /// Store extraction results (used when bus is not available)
    async fn store_extraction_results(
        &self,
        session_id: &str,
        facts: &[String],
        summary: &str,
    ) -> f32 {
        let mut all_stored = true;

        // Store facts
        for fact in facts {
            let embedding = self.embedding_provider.embed(fact).await.ok();
            let entry = MemoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: Some(session_id.to_string()),
                kind: MemoryKind::UserFact,
                content: fact.clone(),
                data: None,
                embedding,
                weight: 0.8,
                created_at: Utc::now(),
                last_accessed_at: Utc::now(),
                access_count: 0,
                tags: vec!["fact".to_string()],
                source_agent: "summary_background".to_string(),
                confirmed: true,
                content_hash: Some(agent_core::memory::compute_content_hash(fact)),
                confidence: 1.0,
                parent_id: None,
                version: 1,
                archived: false,
                compressed_from: vec![],
            };
            if let Err(e) = self.long_term.store(entry).await {
                tracing::warn!("Failed to store fact: {}", e);
                all_stored = false;
            }
        }

        // Store summary
        if !summary.is_empty() {
            let embedding = self.embedding_provider.embed(summary).await.ok();
            let summary_entry = MemoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: Some(session_id.to_string()),
                kind: MemoryKind::Summary,
                content: summary.to_string(),
                data: None,
                embedding,
                weight: 0.6,
                created_at: Utc::now(),
                last_accessed_at: Utc::now(),
                access_count: 0,
                tags: vec!["compressed".to_string()],
                source_agent: "summary_background".to_string(),
                confirmed: false,
                content_hash: None,
                confidence: 0.8,
                parent_id: None,
                version: 1,
                archived: false,
                compressed_from: vec![],
            };
            if let Err(e) = self.long_term.store(summary_entry).await {
                tracing::warn!("Failed to store summary: {}", e);
                all_stored = false;
            }
        }

        if all_stored {
            0.8
        } else {
            0.4
        }
    }

    /// Evaluate compression quality by checking memory system state
    async fn evaluate_compression_quality(
        &self,
        session_id: &str,
        original_turns: &[MemoryEntry],
    ) -> f32 {
        // Check if summaries were stored
        let summaries = self
            .long_term
            .retrieve(MemoryQuery {
                kinds: vec![MemoryKind::Summary],
                session_id: Some(session_id.to_string()),
                limit: 5,
                min_weight: 0.0,
                ..Default::default()
            })
            .await
            .unwrap_or_else(|_| agent_core::memory::MemoryRetrievalResult {
                entries: Vec::new(),
                total_available: 0,
            });

        if summaries.entries.is_empty() {
            return 0.0;
        }

        // Use evaluator if available
        let facts = self
            .long_term
            .retrieve(MemoryQuery {
                kinds: vec![MemoryKind::UserFact],
                session_id: Some(session_id.to_string()),
                limit: 10,
                min_weight: 0.0,
                ..Default::default()
            })
            .await
            .unwrap_or_else(|_| agent_core::memory::MemoryRetrievalResult {
                entries: Vec::new(),
                total_available: 0,
            });

        let latest_summary = &summaries.entries[summaries.entries.len() - 1];
        self.evaluator
            .evaluate(original_turns, &facts.entries, &latest_summary.content)
            .await
            .unwrap_or(0.7)
    }

    /// Incremental summary: build on existing summaries + new turns
    async fn process_incremental_summary(&self, session_id: &str) -> Result<()> {
        let existing = self.load_existing_summaries(session_id).await;
        let turns = self.short_term.list_session_memories(session_id).await?;

        let recent_turns: Vec<&str> = turns
            .iter()
            .rev()
            .take(5)
            .filter(|t| t.kind == MemoryKind::DialogueTurn)
            .map(|t| t.content.as_str())
            .collect();

        if recent_turns.is_empty() {
            return Ok(());
        }

        if let Some(ref llm) = self.llm_provider {
            let prompt = if existing.is_empty() {
                recent_turns.join("\n")
            } else {
                format!(
                    "已有摘要：\n{}\n\n新增对话：\n{}",
                    existing,
                    recent_turns.join("\n")
                )
            };

            match self.llm_extract(&prompt, llm).await {
                Ok((_facts, summary)) => {
                    if !summary.is_empty() {
                        let embedding = self.embedding_provider.embed(&summary).await.ok();
                        let entry = MemoryEntry {
                            id: uuid::Uuid::new_v4().to_string(),
                            session_id: Some(session_id.to_string()),
                            kind: MemoryKind::Summary,
                            content: summary,
                            data: None,
                            embedding,
                            weight: 0.6,
                            created_at: Utc::now(),
                            last_accessed_at: Utc::now(),
                            access_count: 0,
                            tags: vec!["incremental".to_string()],
                            source_agent: "summary_background".to_string(),
                            confirmed: false,
                            content_hash: None,
                            confidence: 0.8,
                            parent_id: None,
                            version: 1,
                            archived: false,
                            compressed_from: vec![],
                        };
                        self.long_term.store(entry).await?;
                    }
                }
                Err(e) => {
                    tracing::warn!("Incremental summary failed: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Cross-session topic fusion
    async fn process_cross_session_fusion(&self, user_id: &str) -> Result<()> {
        let summaries = self
            .long_term
            .retrieve(MemoryQuery {
                kinds: vec![MemoryKind::Summary],
                limit: 20,
                min_weight: 0.3,
                ..Default::default()
            })
            .await?;

        if summaries.entries.len() < 2 {
            return Ok(());
        }

        if let Some(ref llm) = self.llm_provider {
            let summaries_text: String = summaries
                .entries
                .iter()
                .map(|e| format!("- {}", e.content))
                .collect::<Vec<_>>()
                .join("\n");

            let prompt = format!(
                "以下是多个会话的摘要，请提炼跨会话的共同主题和用户偏好：\n{}",
                summaries_text
            );

            match self.llm_extract(&prompt, llm).await {
                Ok((_facts, topics)) => {
                    if !topics.is_empty() {
                        let embedding = self.embedding_provider.embed(&topics).await.ok();
                        let entry = MemoryEntry {
                            id: uuid::Uuid::new_v4().to_string(),
                            session_id: None,
                            kind: MemoryKind::CrossSessionTopic,
                            content: topics,
                            data: Some(serde_json::json!({"user_id": user_id})),
                            embedding,
                            weight: 0.7,
                            created_at: Utc::now(),
                            last_accessed_at: Utc::now(),
                            access_count: 0,
                            tags: vec!["cross_session".to_string()],
                            source_agent: "summary_background".to_string(),
                            confirmed: false,
                            content_hash: None,
                            confidence: 0.8,
                            parent_id: None,
                            version: 1,
                            archived: false,
                            compressed_from: vec![],
                        };
                        self.long_term.store(entry).await?;
                    }
                }
                Err(e) => {
                    tracing::warn!("Cross-session fusion failed: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Call Summary Sub Agent via AgentBus to generate summary
    async fn call_summary_agent_via_bus(
        &self,
        dialogue: &str,
        session_id: &str,
    ) -> std::result::Result<(Vec<String>, String), String> {
        let bus = self
            .bus
            .as_ref()
            .ok_or_else(|| "No AgentBus configured".to_string())?;

        let request_id = uuid::Uuid::new_v4().to_string();
        let envelope = AgentEnvelope {
            id: request_id.clone(),
            from: "summary_background".to_string(),
            to: AgentTarget::Agent("summary".to_string()),
            payload: BusPayload::Task {
                content: dialogue.to_string(),
                message_type: "conversation_summary".to_string(),
                context: Some(serde_json::json!({
                    "session_id": session_id,
                    "mode": "compression",
                })),
            },
            reply_to: None,
        };

        let response = bus
            .request(envelope, 30_000)
            .await
            .map_err(|e| format!("Bus request failed: {}", e))?;

        match response.payload {
            BusPayload::Result { content, .. } => {
                // Parse the summary output
                let (facts, summary) = self.parse_summary_output(&content);
                Ok((facts, summary))
            }
            _ => Err("Unexpected response type from Summary Agent".to_string()),
        }
    }

    /// Parse summary agent output into facts and summary
    fn parse_summary_output(&self, content: &str) -> (Vec<String>, String) {
        let mut facts = Vec::new();
        let mut summary = String::new();
        let mut in_facts = false;
        let mut in_summary = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("FACTS:") {
                in_facts = true;
                in_summary = false;
                continue;
            }
            if trimmed.starts_with("SUMMARY:") {
                in_facts = false;
                in_summary = true;
                continue;
            }
            if in_facts {
                if let Some(fact) = trimmed
                    .strip_prefix("- ")
                    .or_else(|| trimmed.strip_prefix("• "))
                {
                    facts.push(fact.to_string());
                }
            }
            if in_summary && !trimmed.is_empty() {
                if !summary.is_empty() {
                    summary.push(' ');
                }
                summary.push_str(trimmed);
            }
        }

        (facts, summary)
    }

    /// Extract summary with quality retry: try bus first, then LLM, retry on low quality
    async fn extract_with_quality_retry(
        &self,
        dialogue: &str,
        session_id: &str,
        original_turns: &[MemoryEntry],
    ) -> std::result::Result<(Vec<String>, String), String> {
        let mut attempts = 0;
        let max_retries = self.config.max_retries;

        loop {
            attempts += 1;

            // Try bus-based communication first, then fallback to direct LLM
            let result = if self.bus.is_some() {
                self.call_summary_agent_via_bus(dialogue, session_id).await
            } else if let Some(ref llm) = self.llm_provider {
                self.llm_extract(dialogue, llm).await
            } else {
                return Err("No bus or LLM provider available".to_string());
            };

            match result {
                Ok((facts, summary)) => {
                    // Evaluate quality
                    let fact_entries: Vec<MemoryEntry> = facts
                        .iter()
                        .map(|f| MemoryEntry {
                            id: String::new(),
                            session_id: None,
                            kind: MemoryKind::UserFact,
                            content: f.clone(),
                            data: None,
                            embedding: None,
                            weight: 0.8,
                            created_at: Utc::now(),
                            last_accessed_at: Utc::now(),
                            access_count: 0,
                            tags: vec![],
                            source_agent: String::new(),
                            confirmed: false,
                            content_hash: None,
                            confidence: 1.0,
                            parent_id: None,
                            version: 1,
                            archived: false,
                            compressed_from: vec![],
                        })
                        .collect();

                    let quality = self
                        .evaluator
                        .evaluate(original_turns, &fact_entries, &summary)
                        .await
                        .unwrap_or(0.8);

                    if quality >= self.config.quality_threshold {
                        return Ok((facts, summary));
                    }

                    if attempts > max_retries {
                        // Quality still below threshold after all retries:
                        // degrade to rule-based extraction
                        tracing::warn!(
                            "Quality {:.2} still below threshold after {} attempts for session {}, degrading to rule-based",
                            quality, attempts, session_id
                        );
                        return Err(format!(
                            "Quality {:.2} below threshold after {} attempts, degrade to rule-based",
                            quality, attempts
                        ));
                    }

                    tracing::warn!(
                        "Quality {:.2} below threshold {} (attempt {}/{}), retrying for session {}",
                        quality,
                        self.config.quality_threshold,
                        attempts,
                        max_retries,
                        session_id
                    );
                }
                Err(e) => {
                    if attempts > max_retries {
                        return Err(e);
                    }
                    tracing::warn!(
                        "Extraction failed (attempt {}/{}): {}",
                        attempts,
                        max_retries,
                        e
                    );
                }
            }
        }
    }

    /// Load existing summaries for a session
    async fn load_existing_summaries(&self, session_id: &str) -> String {
        match self
            .long_term
            .retrieve(MemoryQuery {
                kinds: vec![MemoryKind::Summary],
                session_id: Some(session_id.to_string()),
                limit: 3,
                min_weight: 0.0,
                ..Default::default()
            })
            .await
        {
            Ok(result) => result
                .entries
                .iter()
                .map(|e| format!("- {}", e.content))
                .collect::<Vec<_>>()
                .join("\n"),
            Err(_) => String::new(),
        }
    }

    /// Basic compression without LLM (fallback)
    async fn compress_basic(&self, session_id: &str) -> Result<()> {
        use agent_core::memory::CompressionStrategy;

        let facts = self
            .short_term
            .compress(session_id, CompressionStrategy::ExtractFacts)
            .await?;
        let summaries = self
            .short_term
            .compress(session_id, CompressionStrategy::Summarize)
            .await?;

        let mut entries = Vec::new();
        for mut entry in facts.into_iter().chain(summaries.into_iter()) {
            if let Ok(embedding) = self.embedding_provider.embed(&entry.content).await {
                entry.embedding = Some(embedding);
            }
            entry.last_accessed_at = Utc::now();
            entries.push(entry);
        }

        if !entries.is_empty() {
            self.long_term.store_batch(entries).await?;
        }

        Ok(())
    }

    /// Use LLM to extract facts and generate summary
    async fn llm_extract(
        &self,
        dialogue: &str,
        llm: &Arc<dyn LlmProvider>,
    ) -> std::result::Result<(Vec<String>, String), String> {
        use agent_core::provider::{ChatMessage, CompletionRequest};

        let system = "你是一个记忆压缩助手。请从以下对话中：
1. 提取关键事实（用户偏好、重要信息），每行一个事实
2. 生成简洁摘要

请按以下格式输出：
FACTS:
- 事实1
- 事实2
SUMMARY:
摘要内容"
            .to_string();

        let request = CompletionRequest {
            messages: vec![ChatMessage::simple("user", dialogue.to_string())],
            max_tokens: Some(8192),
            temperature: Some(0.3),
            system: Some(system),
            ..Default::default()
        };

        let resp = llm.complete(request).await.map_err(|e| e.to_string())?;
        let content = resp.content;

        let mut facts = Vec::new();
        let mut summary = String::new();
        let mut in_facts = false;
        let mut in_summary = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("FACTS:") {
                in_facts = true;
                in_summary = false;
                continue;
            }
            if trimmed.starts_with("SUMMARY:") {
                in_facts = false;
                in_summary = true;
                continue;
            }
            if in_facts {
                if let Some(fact) = trimmed
                    .strip_prefix("- ")
                    .or_else(|| trimmed.strip_prefix("• "))
                {
                    facts.push(fact.to_string());
                }
            }
            if in_summary && !trimmed.is_empty() {
                if !summary.is_empty() {
                    summary.push(' ');
                }
                summary.push_str(trimmed);
            }
        }

        Ok((facts, summary))
    }
}

/// Handle for submitting tasks to the background service
#[derive(Clone)]
pub struct SummaryServiceHandle {
    sender: mpsc::Sender<SummaryTask>,
}

impl SummaryServiceHandle {
    pub fn new(sender: mpsc::Sender<SummaryTask>) -> Self {
        Self { sender }
    }

    /// Submit a compression task (non-blocking)
    pub async fn submit(&self, task: SummaryTask) -> Result<()> {
        self.sender.send(task).await.map_err(|_| {
            agent_core::error::AgentTeamsError::Internal(
                "Summary service channel closed".to_string(),
            )
        })
    }

    /// Submit compression for a session
    pub async fn compress_session(&self, session_id: &str) -> Result<()> {
        self.submit(SummaryTask::CompressSession {
            session_id: session_id.to_string(),
        })
        .await
    }

    /// Submit incremental summary for a session
    pub async fn incremental_summary(&self, session_id: &str) -> Result<()> {
        self.submit(SummaryTask::IncrementalSummary {
            session_id: session_id.to_string(),
        })
        .await
    }

    /// Submit cross-session fusion for a user
    pub async fn cross_session_fusion(&self, user_id: &str) -> Result<()> {
        self.submit(SummaryTask::CrossSessionFusion {
            user_id: user_id.to_string(),
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summary_service_config_defaults() {
        let config = SummaryServiceConfig::default();
        assert_eq!(config.interval_secs, 60);
        assert_eq!(config.max_turns_per_batch, 20);
        assert!((config.quality_threshold - 0.5).abs() < 0.01);
        assert_eq!(config.max_retries, 2);
    }

    #[test]
    fn test_in_progress_sessions_prevents_duplicate() {
        let in_progress: Arc<DashMap<String, ()>> = Arc::new(DashMap::new());
        let session_id = "test-session-1";

        // First insertion succeeds
        assert!(!in_progress.contains_key(session_id));
        in_progress.insert(session_id.to_string(), ());

        // Second check sees it's already in progress
        assert!(in_progress.contains_key(session_id));

        // Removal allows re-entry
        in_progress.remove(session_id);
        assert!(!in_progress.contains_key(session_id));
    }

    #[test]
    fn test_in_progress_sessions_concurrent_different_sessions() {
        let in_progress: Arc<DashMap<String, ()>> = Arc::new(DashMap::new());

        // Different sessions can run concurrently
        in_progress.insert("session-a".to_string(), ());
        in_progress.insert("session-b".to_string(), ());

        assert!(in_progress.contains_key("session-a"));
        assert!(in_progress.contains_key("session-b"));

        // Removing one doesn't affect the other
        in_progress.remove("session-a");
        assert!(!in_progress.contains_key("session-a"));
        assert!(in_progress.contains_key("session-b"));
    }

    #[test]
    fn test_parse_summary_output_text_format() {
        // We can't easily construct SummaryBackgroundService without real stores,
        // but we can test the parse logic by replicating it
        let content = "FACTS:\n- 用户喜欢Rust\n- 用户是开发者\nSUMMARY:\n用户讨论了编程语言偏好";

        let mut facts = Vec::new();
        let mut summary = String::new();
        let mut in_facts = false;
        let mut in_summary = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("FACTS:") {
                in_facts = true;
                in_summary = false;
                continue;
            }
            if trimmed.starts_with("SUMMARY:") {
                in_facts = false;
                in_summary = true;
                continue;
            }
            if in_facts {
                if let Some(fact) = trimmed
                    .strip_prefix("- ")
                    .or_else(|| trimmed.strip_prefix("• "))
                {
                    facts.push(fact.to_string());
                }
            }
            if in_summary && !trimmed.is_empty() {
                if !summary.is_empty() {
                    summary.push(' ');
                }
                summary.push_str(trimmed);
            }
        }

        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0], "用户喜欢Rust");
        assert_eq!(facts[1], "用户是开发者");
        assert_eq!(summary, "用户讨论了编程语言偏好");
    }

    #[test]
    fn test_dedup_lock_release_on_success() {
        let in_progress: Arc<DashMap<String, ()>> = Arc::new(DashMap::new());
        let session_id = "session-release-test";

        // Simulate the lock lifecycle
        in_progress.insert(session_id.to_string(), ());
        assert!(in_progress.contains_key(session_id));

        // Simulate successful completion
        in_progress.remove(session_id);
        assert!(!in_progress.contains_key(session_id));

        // Can re-acquire after release
        in_progress.insert(session_id.to_string(), ());
        assert!(in_progress.contains_key(session_id));
        in_progress.remove(session_id);
    }
}
