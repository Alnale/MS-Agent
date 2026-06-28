use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use serde_json::json;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

use agent_teams_core::dedup_engine::{DedupAction, HierarchicalDedupEngine};
use agent_teams_core::effect::AgentEffect;
use agent_teams_core::error::Result;
use agent_teams_core::memory::{
    compute_content_hash, MemoryConfig, MemoryEntry, MemoryEntryBuilder, MemoryEvent,
    MemoryEventHandler, MemoryKind, MemoryQuery,
};
use agent_teams_core::memory_intent::{IntentRecognizer, RankingConfig};
use agent_teams_core::memory_reranker::MemoryReranker;
use agent_teams_core::memory_store::{EmbeddingProvider, MemoryStore};
use agent_teams_core::provider::{ChatMessage, CompletionRequest, LlmProvider};
use agent_teams_core::tag_extractor::TagExtractor;

use crate::summary_background::SummaryServiceHandle;

use agent_teams_core::cosine_similarity;

/// Estimate token count for text (Chinese chars ~1 token, English words ~1.3 tokens)
fn estimate_tokens(text: &str) -> usize {
    let char_count = text.chars().count();
    let word_count = text.split_whitespace().count();
    char_count + word_count * 13 / 10
}

/// Cached working memory for a session
struct SessionCache {
    working_memory: Vec<MemoryEntry>,
    initialized_at: Instant,
}

/// Maximum age for session cache entries before eviction (5 minutes)
const SESSION_CACHE_MAX_AGE_SECS: u64 = 300;
/// Maximum number of cached sessions
const SESSION_CACHE_MAX_SIZE: usize = 1000;

/// Memory manager: coordinates three-level memory flow
pub struct MemoryManager {
    /// Short-term memory store (session-scoped, TTL-based)
    short_term: Arc<dyn MemoryStore>,
    /// Long-term memory store (persistent, with vector search)
    long_term: Arc<dyn MemoryStore>,
    /// Embedding provider for generating vectors
    embedding_provider: Arc<dyn EmbeddingProvider>,
    /// Optional LLM provider for compression and profile updates
    llm_provider: Option<Arc<dyn LlmProvider>>,
    /// Intent recognizer for query classification
    intent_recognizer: IntentRecognizer,
    /// Tag extractor for enhanced tag matching
    tag_extractor: TagExtractor,
    /// Session-level cache to avoid re-initializing on every request
    session_cache: RwLock<HashMap<String, SessionCache>>,
    /// Optional background summary service handle for async compression
    summary_service: Option<SummaryServiceHandle>,
    /// Optional reranker for refining retrieval results
    reranker: Option<MemoryReranker>,
    /// Optional hierarchical dedup engine for record_fact
    dedup_engine: Option<HierarchicalDedupEngine>,
    /// Event handlers for event-driven memory updates
    event_handlers: Vec<Arc<dyn MemoryEventHandler>>,
    /// Configuration
    config: MemoryConfig,
}


impl MemoryManager {
    pub fn new(
        short_term: Arc<dyn MemoryStore>,
        long_term: Arc<dyn MemoryStore>,
        embedding_provider: Arc<dyn EmbeddingProvider>,
        config: MemoryConfig,
    ) -> Self {
        let intent_recognizer = IntentRecognizer::new(None);
        let reranker = MemoryReranker::new(embedding_provider.clone());
        let dedup_engine = HierarchicalDedupEngine::new(embedding_provider.clone());
        Self {
            short_term,
            long_term,
            embedding_provider,
            llm_provider: None,
            intent_recognizer,
            tag_extractor: TagExtractor::new(),
            session_cache: RwLock::new(HashMap::new()),
            summary_service: None,
            reranker: Some(reranker),
            dedup_engine: Some(dedup_engine),
            event_handlers: Vec::new(),
            config,
        }
    }

    /// Register an event handler for memory events
    pub fn with_event_handler(mut self, handler: Arc<dyn MemoryEventHandler>) -> Self {
        self.event_handlers.push(handler);
        self
    }

    /// Emit a memory event to all registered handlers
    async fn emit_event(&self, event: MemoryEvent) {
        for handler in &self.event_handlers {
            if let Err(e) = handler.handle(event.clone()).await {
                tracing::warn!("Memory event handler failed: {}", e);
            }
        }
    }

    /// Set the LLM provider for compression and profile updates
    pub fn with_llm_provider(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.intent_recognizer = IntentRecognizer::new(Some(provider.clone()));
        self.llm_provider = Some(provider);
        self
    }

    /// Set the background summary service handle for async compression
    pub fn with_summary_service(mut self, handle: SummaryServiceHandle) -> Self {
        self.summary_service = Some(handle);
        self
    }

    /// Trigger async compression via background service (non-blocking)
    /// Only uses SummaryBackgroundService — no synchronous fallback (Single-Writer principle)
    pub async fn trigger_compression(&self, session_id: &str) -> Result<()> {
        if let Some(ref service) = self.summary_service {
            service.compress_session(session_id).await?;
        } else {
            tracing::warn!(
                "No summary service configured, skipping compression for session {}",
                session_id
            );
        }
        Ok(())
    }

    /// Sliding window compression: compress only older dialogue turns while keeping recent ones intact.
    /// This provides more granular compression than the current session-level approach.
    /// 
    /// - `keep_recent`: Number of recent dialogue turns to preserve (default: 5)
    /// - Returns: Number of entries compressed
    pub async fn sliding_window_compress(
        &self,
        session_id: &str,
        keep_recent: usize,
    ) -> Result<usize> {
        if !self.config.enabled {
            return Ok(0);
        }

        let memories = self.short_term.list_session_memories(session_id).await?;
        let mut turns: Vec<_> = memories
            .iter()
            .filter(|m| m.kind == MemoryKind::DialogueTurn)
            .collect();

        // Sort by creation time (oldest first)
        turns.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        if turns.len() <= keep_recent {
            tracing::debug!(
                "Session {} has {} turns, less than keep_recent={}, skipping compression",
                session_id, turns.len(), keep_recent
            );
            return Ok(0);
        }

        // Compress turns before the keep_recent window
        let to_compress = &turns[..turns.len() - keep_recent];
        let compress_count = to_compress.len();

        tracing::info!(
            "Sliding window compress: session={}, total_turns={}, compressing={}, keeping={}",
            session_id, turns.len(), compress_count, keep_recent
        );

        // Extract key facts from older turns before compressing
        if let Some(ref llm) = self.llm_provider {
            let older_content: Vec<String> = to_compress
                .iter()
                .map(|t| t.content.clone())
                .collect();
            
            // Extract facts from older turns
            let facts = self.extract_facts_from_batch(&older_content, llm).await;
            
            // Store extracted facts in long-term memory
            for fact in facts {
                if let Err(e) = self.record_fact(
                    &self.get_user_id_from_session(session_id).await.unwrap_or_default(),
                    session_id,
                    &fact,
                    "sliding_window_extractor",
                ).await {
                    tracing::warn!("Failed to store extracted fact: {}", e);
                }
            }
        }

        // Archive compressed turns (mark as archived, don't delete)
        for turn in to_compress {
            // In a real implementation, you'd mark these as archived
            // For now, we'll just log them
            tracing::debug!("Archiving turn {} from session {}", turn.id, session_id);
        }

        Ok(compress_count)
    }

    /// Extract facts from a batch of dialogue turns
    async fn extract_facts_from_batch(&self, contents: &[String], llm: &Arc<dyn LlmProvider>) -> Vec<String> {
        if contents.is_empty() {
            return Vec::new();
        }

        let batch_text = contents.join("\n---\n");
        let system = "你是一个信息提取器。从以下对话中提取用户的持久性信息（职业、偏好、习惯等）。
        
返回JSON数组格式：[{\"fact\": \"提取的事实\", \"kind\": \"fact/preference\"}]
如果没有值得提取的持久性信息，返回空数组 []。只输出JSON，不要其他内容。";

        let request = CompletionRequest {
            model: String::new(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: format!("对话内容：\n{}", batch_text),
                cache_control: None,
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: Some(8192),
            temperature: Some(0.1),
            system: Some(system.to_string()),
            stream: false,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
        };

        match llm.complete(request).await {
            Ok(resp) => {
                let content = resp.content.trim();
                if content.is_empty() || content == "[]" {
                    return Vec::new();
                }
                
                match serde_json::from_str::<Vec<serde_json::Value>>(content) {
                    Ok(facts) => facts
                        .iter()
                        .filter_map(|f| f["fact"].as_str().map(|s| s.to_string()))
                        .filter(|s| !s.trim().is_empty() && s.trim().len() >= 10)
                        .collect(),
                    Err(e) => {
                        tracing::warn!("Failed to parse batch fact extraction: {}", e);
                        Vec::new()
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Batch fact extraction failed: {}", e);
                Vec::new()
            }
        }
    }

    /// Helper to get user_id from session (placeholder implementation)
    async fn get_user_id_from_session(&self, _session_id: &str) -> Option<String> {
        // In a real implementation, you'd look up the user_id from the session
        // For now, return None
        None
    }

    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }

    /// Initialize session: load relevant memories into working memory using multi-channel retrieval
    /// Uses session-level cache to avoid expensive re-initialization on every request.
    pub async fn initialize_session(
        &self,
        user_id: &str,
        session_id: &str,
        current_query: &str,
    ) -> Result<Vec<MemoryEntry>> {
        if !self.config.enabled {
            tracing::debug!("Memory system disabled");
            return Ok(Vec::new());
        }

        // Check session cache (TTL from config)
        {
            let cache = self.session_cache.read().await;
            if let Some(cached) = cache.get(session_id) {
                if cached.initialized_at.elapsed().as_secs() < self.config.session_cache_ttl_secs {
                    tracing::info!(
                        "Session cache hit for session={} (age={}ms)",
                        session_id,
                        cached.initialized_at.elapsed().as_millis()
                    );
                    return Ok(cached.working_memory.clone());
                }
            }
        }
        // Note: concurrent requests for the same session may both miss the cache
        // and run initialization in parallel. This is benign — the second write
        // simply overwrites the first with equivalent data.

        tracing::info!(
            "Initializing memory session for user={}, session={}",
            user_id,
            session_id
        );

        let query_embedding = self
            .embedding_provider
            .embed(current_query)
            .await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::Internal(e.to_string()))?;

        // Channel 1: Semantic retrieval from long-term memory (scoped to session)
        let semantic = self
            .long_term
            .retrieve(MemoryQuery {
                text: current_query.to_string(),
                embedding: Some(query_embedding.clone()),
                kinds: vec![
                    MemoryKind::UserFact,
                    MemoryKind::InferredPreference,
                    MemoryKind::CrossSessionTopic,
                ],
                limit: self.config.retrieval_limit,
                min_weight: 0.3,
                similarity_threshold: self.config.similarity_threshold,
                session_id: Some(session_id.to_string()),
                user_id: Some(user_id.to_string()),
                ..Default::default()
            })
            .await?;

        // Channel 2: Recent short-term memories (timeline)
        let recent = self.short_term.list_session_memories(session_id).await?;
        tracing::info!(
            "Channel 2: Found {} short-term memories for session={}",
            recent.len(),
            session_id
        );

        // Channel 3: User profile — DISABLED for session isolation
        // User profile is global (cross-session) and causes context leakage between sessions.
        // Each session should only see its own memories.
        let profile: Option<serde_json::Value> = None;

        // Channel 4: Tag-based recall for known tags in the query (scoped to session)
        let query_tags: Vec<String> = self.extract_tags(current_query);
        let tag_results = if !query_tags.is_empty() {
            self.long_term
                .retrieve(MemoryQuery {
                    tags: query_tags,
                    limit: 5,
                    min_weight: 0.2,
                    session_id: Some(session_id.to_string()),
                    user_id: Some(user_id.to_string()),
                    ..Default::default()
                })
                .await?
                .entries
        } else {
            Vec::new()
        };

        // Merge all candidates with deduplication
        let mut candidates: Vec<MemoryEntry> = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        // Add user profile first (highest priority)
        if let Some(p) = profile {
            let profile_entry = MemoryEntryBuilder::new(
                MemoryKind::UserProfile,
                p.to_string(),
                "system",
            )
            .id(format!("profile:{}", user_id))
            .session_id(session_id)
            .data(p)
            .weight(1.0)
            .tags(vec!["profile".to_string()])
            .confirmed(true)
            .build();
            seen_ids.insert(profile_entry.id.clone());
            candidates.push(profile_entry);
        }

        // Add semantic results
        for entry in semantic.entries {
            if seen_ids.insert(entry.id.clone()) {
                candidates.push(entry);
            }
        }

        // Add recent short-term (reverse to get most recent first)
        for entry in recent.into_iter().rev().take(10) {
            if seen_ids.insert(entry.id.clone()) {
                candidates.push(entry);
            }
        }

        // Add tag-based results
        for entry in tag_results {
            if seen_ids.insert(entry.id.clone()) {
                candidates.push(entry);
            }
        }

        // Channel 5: User evolution summary — DISABLED for session isolation
        // Evolution graph is global (cross-session) and causes context leakage between sessions.

        // Content-hash dedup: remove entries with identical content
        {
            let mut seen_hashes = std::collections::HashSet::new();
            candidates.retain(|e| {
                if let Some(ref hash) = e.content_hash {
                    seen_hashes.insert(hash.clone())
                } else {
                    true
                }
            });
        }

        // Reranker: filter low-quality candidates via embedding-based scoring
        tracing::info!("Before reranker: {} candidates", candidates.len());
        let candidates = if let Some(ref reranker) = self.reranker {
            match reranker.rerank_with_embedding(&query_embedding, candidates.clone()) {
                Ok(scored) => {
                    tracing::info!("After reranker: {} scored entries", scored.len());
                    scored.into_iter().map(|s| s.entry).collect()
                }
                Err(e) => {
                    tracing::warn!("Reranker failed, using unfiltered candidates: {}", e);
                    candidates
                }
            }
        } else {
            candidates
        };

        // Recognize query intent for dynamic reranking
        let intent = self.intent_recognizer.recognize(current_query).await;
        let ranking_config = RankingConfig::for_intent(&intent);

        // Rerank with intent-aware composite scoring
        let query_emb_ref = &query_embedding;
        let mut scored: Vec<(f32, MemoryEntry)> = candidates
            .into_iter()
            .map(|e| {
                let score = self.compute_rerank_score_with_config(
                    &e,
                    query_emb_ref,
                    Utc::now(),
                    &ranking_config,
                );
                (score, e)
            })
            .collect();

        scored.sort_by(|a, b| b.0.total_cmp(&a.0));

        // Token budget-aware truncation
        tracing::info!("Before truncation: {} scored entries", scored.len());
        let working = self.truncate_by_token_budget(scored);
        tracing::info!("After truncation: {} working memories", working.len());

        // Update access stats
        for entry in &working {
            let _ = self.long_term.touch(&entry.id).await;
        }

        // Store in session cache with eviction
        {
            let mut cache = self.session_cache.write().await;
            // Evict stale entries if cache is large
            if cache.len() >= SESSION_CACHE_MAX_SIZE {
                cache.retain(|_, v| v.initialized_at.elapsed().as_secs() < SESSION_CACHE_MAX_AGE_SECS);
            }
            cache.insert(
                session_id.to_string(),
                SessionCache {
                    working_memory: working.clone(),
                    initialized_at: Instant::now(),
                },
            );
        }

        Ok(working)
    }

    /// Extract potential tags from query text using enhanced tag extraction
    fn extract_tags(&self, text: &str) -> Vec<String> {
        self.tag_extractor.extract(text)
    }

    /// Compute rerank score with intent-specific ranking config
    fn compute_rerank_score_with_config(
        &self,
        entry: &MemoryEntry,
        query_embedding: &[f32],
        now: DateTime<Utc>,
        config: &RankingConfig,
    ) -> f32 {
        // Semantic similarity (0.0 if no embedding)
        let semantic_sim = entry
            .embedding
            .as_ref()
            .map(|emb| cosine_similarity(emb, query_embedding))
            .unwrap_or(0.0);

        // Time decay: exponential decay based on age in days
        let age_hours = (now - entry.created_at).num_hours().max(0) as f32;
        let half_life_hours = self.config.decay_halflife_days * 24.0;
        let time_decay = if half_life_hours > 0.0 {
            2.0_f32.powf(-age_hours / half_life_hours)
        } else {
            1.0
        };

        // Normalized access count (log scale, capped at 1.0)
        let access_norm = if entry.access_count > 0 {
            ((entry.access_count as f32).ln() + 1.0).min(1.0) / 10.0
        } else {
            0.0
        };

        // Confirmed bonus
        let confirmed_bonus = if entry.confirmed { 1.0 } else { 0.0 };

        // Intent-specific boost for matching memory kinds
        let intent_boost = config
            .intent_boosts
            .get(&entry.kind)
            .copied()
            .unwrap_or(1.0);

        let base_score = config.semantic_weight * semantic_sim
            + config.recency_weight * time_decay
            + config.access_weight * access_norm
            + config.weight_factor * entry.weight
            + config.confirmed_bonus * confirmed_bonus;

        base_score * intent_boost
    }

    /// Truncate working memory by token budget
    fn truncate_by_token_budget(
        &self,
        sorted_entries: Vec<(f32, MemoryEntry)>,
    ) -> Vec<MemoryEntry> {
        let mut total_tokens = 0;
        let mut final_entries = Vec::new();
        let max_tokens = self.config.working_memory_max_tokens;
        let max_count = self.config.working_memory_limit;

        for (_, entry) in sorted_entries {
            if final_entries.len() >= max_count {
                break;
            }
            let tokens = estimate_tokens(&entry.content);
            if total_tokens + tokens > max_tokens {
                break;
            }
            total_tokens += tokens;
            final_entries.push(entry);
        }

        final_entries
    }

    /// Record a dialogue turn to short-term memory
    pub async fn record_turn(
        &self,
        session_id: &str,
        user_message: &str,
        assistant_response: &str,
        effects: &[AgentEffect],
    ) -> Result<()> {
        if !self.config.enabled {
            tracing::debug!("Memory system disabled, skipping record_turn");
            return Ok(());
        }

        tracing::info!("Recording turn to memory for session={}", session_id);

        let content = format!("User: {}\nAssistant: {}", user_message, assistant_response);
        let entry = MemoryEntryBuilder::new(
            MemoryKind::DialogueTurn,
            content.clone(),
            "coordinator",
        )
        .session_id(session_id)
        .data(json!({
            "user": user_message,
            "assistant": assistant_response,
            "effects_count": effects.len(),
        }))
        .content_hash(compute_content_hash(&content))
        .build();

        self.short_term.store(entry).await?;
        tracing::info!(
            "Stored dialogue turn in short-term memory for session={}",
            session_id
        );

        // Check compression trigger: hard limit OR density-based
        let session_memories = self.short_term.list_session_memories(session_id).await?;
        let turn_count = session_memories
            .iter()
            .filter(|m| m.kind == MemoryKind::DialogueTurn)
            .count();
        let fact_count = session_memories
            .iter()
            .filter(|m| m.kind == MemoryKind::UserFact)
            .count();

        let should_compress = if turn_count >= self.config.compression_threshold {
            // Hard limit reached
            true
        } else if turn_count > 0 {
            // Density check: high information density triggers early compression
            let density = fact_count as f32 / turn_count as f32;
            density >= 0.5 && turn_count >= 10
        } else {
            false
        };

        // Emit event for new turn
        self.emit_event(MemoryEvent::TurnRecorded {
            session_id: session_id.to_string(),
            turn_count,
        })
        .await;

        if should_compress {
            if let Err(e) = self.trigger_compression(session_id).await {
                tracing::warn!(
                    "Compression trigger failed for session {}: {}",
                    session_id,
                    e
                );
            }
        }

        Ok(())
    }

    /// Extract key facts from a user message using LLM and store them.
    /// This runs after each turn to ensure important information is immediately available.
    pub async fn extract_and_store_facts(
        &self,
        user_id: &str,
        session_id: &str,
        user_message: &str,
        _assistant_response: &str,
    ) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        let llm = match &self.llm_provider {
            Some(p) => p.clone(),
            None => {
                tracing::debug!("No LLM provider for fact extraction");
                return Ok(());
            }
        };

        // Lightweight extraction prompt — asks LLM to extract key facts in JSON
        let system = "你是一个信息提取器。只从用户消息中提取用户的持久性信息。

**只提取以下类型的信息：**
- 用户明确陈述的个人事实（如职业、偏好、习惯）
- 用户明确表达的长期偏好（如\"我喜欢深色主题\"）
- 用户给出的持久性指令（如\"以后用中文回答\"）

**严格不要提取以下内容：**
- 任何不是用户直接陈述的信息
- 用户的请求或命令（如\"帮我写个快排\"、\"翻译这段话\"）
- 用户的问题（如\"什么是快排？\"）
- 一次性的对话内容
- 角色扮演中虚构的内容（如\"小猫娘喜欢吃鱼\"不是用户事实）
- 任何推断或猜测的内容

返回JSON数组格式：[{\"fact\": \"提取的事实\", \"kind\": \"fact/preference/instruction\"}]
如果没有值得提取的持久性信息，返回空数组 []。只输出JSON，不要其他内容。";

        // Only send the USER message for fact extraction.
        // Sending assistant responses causes the LLM to extract its own hallucinated
        // content as "facts", which then pollutes future sessions via memory retrieval.
        let user_prompt = format!(
            "用户消息：{}",
            user_message
        );

        let request = CompletionRequest {
            model: String::new(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: user_prompt,
                cache_control: None,
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: Some(8192),
            temperature: Some(0.1),
            system: Some(system.to_string()),
            stream: false,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
        };

        // Run extraction asynchronously to not block the response
        let response = match llm.complete(request).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Fact extraction LLM call failed: {}", e);
                return Ok(());
            }
        };

        let content = response.content.trim();
        tracing::debug!("Fact extraction LLM response: '{}'", content);
        if content.is_empty() || content == "[]" {
            tracing::debug!(
                "Fact extraction returned empty result for session={}",
                session_id
            );
            return Ok(());
        }

        // Parse the JSON array of extracted facts
        let facts: Vec<serde_json::Value> = match serde_json::from_str(content) {
            Ok(f) => f,
            Err(e) => {
                tracing::debug!(
                    "Failed to parse fact extraction result: {}, content: {}",
                    e,
                    content
                );
                return Ok(());
            }
        };

        for fact in &facts {
            let fact_text = match fact["fact"].as_str() {
                Some(t) if !t.trim().is_empty() && t.trim().len() >= 10 => t.trim(),
                _ => continue,
            };
            let kind_str = fact["kind"].as_str().unwrap_or("fact");

            let memory_kind = match kind_str {
                "preference" => MemoryKind::InferredPreference,
                "instruction" => MemoryKind::UserFact, // Instructions stored as high-weight facts
                _ => MemoryKind::UserFact,
            };

            let weight = match kind_str {
                "instruction" => 0.95, // Instructions are very important
                "preference" => 0.8,
                _ => 0.7,
            };

            // Use record_fact which handles dedup
            let content_hash = compute_content_hash(fact_text);

            // Use record_fact() which has proper dedup (hash + semantic + hierarchical)
            // For non-UserFact kinds, store directly after semantic dedup
            if memory_kind == MemoryKind::UserFact {
                if let Err(e) = self
                    .record_fact(user_id, session_id, fact_text, "memory_extractor")
                    .await
                {
                    tracing::warn!("Failed to record extracted fact '{}': {}", fact_text, e);
                } else {
                    tracing::info!(
                        "Extracted and stored fact: {} (kind={})",
                        fact_text,
                        kind_str
                    );
                }
            } else {
                // For InferredPreference, do semantic dedup then store
                let embedding = self.embedding_provider.embed(fact_text).await.ok();

                // Semantic dedup check (scoped to session)
                let is_dup = if let Some(ref emb) = embedding {
                    let similar = match self
                        .long_term
                        .retrieve(MemoryQuery {
                            text: fact_text.to_string(),
                            embedding: Some(emb.clone()),
                            kinds: vec![memory_kind.clone()],
                            limit: 3,
                            min_weight: 0.0,
                            similarity_threshold: self.config.duplicate_similarity_threshold,
                            session_id: Some(session_id.to_string()),
                            user_id: Some(user_id.to_string()),
                            ..Default::default()
                        })
                        .await
                    {
                        Ok(r) => r,
                        Err(_) => agent_teams_core::memory::MemoryRetrievalResult {
                            entries: Vec::new(),
                            total_available: 0,
                        },
                    };

                    similar.entries.iter().any(|e| {
                        e.content_hash.as_ref() == Some(&content_hash) || e.content == fact_text
                    })
                } else {
                    false
                };

                if is_dup {
                    tracing::debug!("Skipping duplicate preference: {}", fact_text);
                    continue;
                }

                let mut builder = MemoryEntryBuilder::new(
                    memory_kind,
                    fact_text,
                    "memory_extractor",
                )
                .session_id(session_id)
                .data(json!({
                    "user_id": user_id,
                    "extracted_from": "turn_extraction",
                    "original_kind": kind_str,
                }))
                .weight(weight)
                .tags(vec![kind_str.to_string(), "extracted".to_string()])
                .confirmed(true)
                .content_hash(content_hash)
                .confidence(0.9);
                if let Some(emb) = embedding {
                    builder = builder.embedding(emb);
                }
                let entry = builder.build();

                if let Err(e) = self.long_term.store(entry).await {
                    tracing::warn!(
                        "Failed to store extracted preference '{}': {}",
                        fact_text,
                        e
                    );
                } else {
                    tracing::info!(
                        "Extracted and stored preference: {} (kind={})",
                        fact_text,
                        kind_str
                    );
                }
            }
        }

        Ok(())
    }

    /// Batch extract facts from multiple messages in a single LLM call.
    /// This is more efficient than calling extract_and_store_facts multiple times.
    /// 
    /// Use this when you have multiple messages to process at once (e.g., after a batch of turns).
    pub async fn batch_extract_and_store_facts(
        &self,
        user_id: &str,
        session_id: &str,
        messages: &[(&str, &str)],  // (user_message, assistant_response) pairs
    ) -> Result<()> {
        if !self.config.enabled || messages.is_empty() {
            return Ok(());
        }

        let llm = match &self.llm_provider {
            Some(p) => p.clone(),
            None => {
                tracing::debug!("No LLM provider for batch fact extraction");
                return Ok(());
            }
        };

        // Build batch prompt with all user messages
        let user_messages: Vec<String> = messages
            .iter()
            .enumerate()
            .map(|(i, (user_msg, _))| format!("{}. {}", i + 1, user_msg))
            .collect();
        
        let batch_prompt = format!(
            "用户消息列表：\n{}",
            user_messages.join("\n")
        );

        let system = "你是一个信息提取器。从以下多条用户消息中提取用户的持久性信息。

**只提取以下类型的信息：**
- 用户明确陈述的个人事实（如职业、偏好、习惯）
- 用户明确表达的长期偏好（如\"我喜欢深色主题\"）
- 用户给出的持久性指令（如\"以后用中文回答\"）

**严格不要提取以下内容：**
- 任何不是用户直接陈述的信息
- 用户的请求或命令
- 用户的问题
- 一次性的对话内容
- 角色扮演中虚构的内容
- 任何推断或猜测的内容

返回JSON数组格式：[{\"fact\": \"提取的事实\", \"kind\": \"fact/preference/instruction\", \"source_index\": 1}]
其中source_index是该事实来自第几条用户消息（从1开始）。
如果没有值得提取的持久性信息，返回空数组 []。只输出JSON，不要其他内容。";

        let request = CompletionRequest {
            model: String::new(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: batch_prompt,
                cache_control: None,
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: Some(8192),
            temperature: Some(0.1),
            system: Some(system.to_string()),
            stream: false,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
        };

        // Run extraction
        let response = match llm.complete(request).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Batch fact extraction LLM call failed: {}", e);
                return Ok(());
            }
        };

        let content = response.content.trim();
        tracing::debug!("Batch fact extraction LLM response: '{}'", content);
        if content.is_empty() || content == "[]" {
            tracing::debug!(
                "Batch fact extraction returned empty result for session={}",
                session_id
            );
            return Ok(());
        }

        // Parse the JSON array of extracted facts
        let facts: Vec<serde_json::Value> = match serde_json::from_str(content) {
            Ok(f) => f,
            Err(e) => {
                tracing::debug!(
                    "Failed to parse batch fact extraction result: {}, content: {}",
                    e,
                    content
                );
                return Ok(());
            }
        };

        // Store extracted facts
        let mut stored_count = 0;
        for fact in &facts {
            let fact_text = match fact["fact"].as_str() {
                Some(t) if !t.trim().is_empty() && t.trim().len() >= 10 => t.trim(),
                _ => continue,
            };
            let kind_str = fact["kind"].as_str().unwrap_or("fact");

            let memory_kind = match kind_str {
                "preference" => MemoryKind::InferredPreference,
                "instruction" => MemoryKind::UserFact,
                _ => MemoryKind::UserFact,
            };

            let weight = match kind_str {
                "instruction" => 0.95,
                "preference" => 0.8,
                _ => 0.7,
            };

            // Use record_fact which handles dedup
            if memory_kind == MemoryKind::UserFact {
                if let Err(e) = self
                    .record_fact(user_id, session_id, fact_text, "batch_extractor")
                    .await
                {
                    tracing::warn!("Failed to store batch extracted fact '{}': {}", fact_text, e);
                } else {
                    stored_count += 1;
                }
            } else {
                // For InferredPreference, store directly after semantic dedup
                let embedding = self.embedding_provider.embed(fact_text).await.ok();
                let content_hash = compute_content_hash(fact_text);

                // Semantic dedup check (scoped to session)
                let is_dup = if let Some(ref emb) = embedding {
                    let similar = match self
                        .long_term
                        .retrieve(MemoryQuery {
                            text: fact_text.to_string(),
                            embedding: Some(emb.clone()),
                            kinds: vec![memory_kind.clone()],
                            limit: 3,
                            min_weight: 0.0,
                            similarity_threshold: self.config.duplicate_similarity_threshold,
                            session_id: Some(session_id.to_string()),
                            user_id: Some(user_id.to_string()),
                            ..Default::default()
                        })
                        .await
                    {
                        Ok(r) => r,
                        Err(_) => agent_teams_core::memory::MemoryRetrievalResult {
                            entries: Vec::new(),
                            total_available: 0,
                        },
                    };

                    similar.entries.iter().any(|e| {
                        e.content_hash.as_ref() == Some(&content_hash) || e.content == fact_text
                    })
                } else {
                    false
                };

                if is_dup {
                    tracing::debug!("Skipping duplicate batch preference: {}", fact_text);
                    continue;
                }

                let mut builder = MemoryEntryBuilder::new(
                    memory_kind,
                    fact_text,
                    "batch_extractor",
                )
                .session_id(session_id)
                .data(json!({
                    "user_id": user_id,
                    "extracted_from": "batch_extraction",
                    "original_kind": kind_str,
                }))
                .weight(weight)
                .tags(vec![kind_str.to_string(), "extracted".to_string()])
                .confirmed(true)
                .content_hash(content_hash)
                .confidence(0.9);
                if let Some(emb) = embedding {
                    builder = builder.embedding(emb);
                }
                let entry = builder.build();

                if let Err(e) = self.long_term.store(entry).await {
                    tracing::warn!(
                        "Failed to store batch extracted preference '{}': {}",
                        fact_text,
                        e
                    );
                } else {
                    stored_count += 1;
                }
            }
        }

        tracing::info!(
            "Batch extracted {} facts from {} messages for session={}",
            stored_count,
            messages.len(),
            session_id
        );

        Ok(())
    }

    /// Record a user fact to long-term memory (with idempotent deduplication)
    pub async fn record_fact(
        &self,
        user_id: &str,
        session_id: &str,
        fact: &str,
        source_agent: &str,
    ) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        let content_hash = compute_content_hash(fact);

        // Step 1: Exact hash match (fast path) — check if this exact content already exists
        if let Some(existing) = self
            .long_term
            .find_by_content_hash(&content_hash, MemoryKind::UserFact, Some(user_id), Some(session_id))
            .await?
        {
            // Exact duplicate — promote and return
            self.long_term.promote(&existing.id, 0.02).await?;
            self.long_term.touch(&existing.id).await?;
            tracing::debug!(
                "Idempotent dedup: fact '{}' already exists as {}",
                fact,
                existing.id
            );
            return Ok(());
        }

        // Step 2 & 3.5 (parallel): Generate embedding AND retrieve hierarchical dedup candidates
        let (embedding_result, hierarchical_candidates) = tokio::join!(
            self.embedding_provider.embed(fact),
            async {
                if self.dedup_engine.is_some() {
                    self.long_term
                        .retrieve(MemoryQuery {
                            kinds: vec![MemoryKind::UserFact],
                            limit: 10,
                            min_weight: 0.0,
                            similarity_threshold: 0.70,
                            session_id: Some(session_id.to_string()),
                            user_id: Some(user_id.to_string()),
                            ..Default::default()
                        })
                        .await
                        .ok()
                        .map(|r| r.entries)
                } else {
                    None
                }
            }
        );

        let embedding = embedding_result
            .map_err(|e| agent_teams_core::error::AgentTeamsError::Internal(e.to_string()))?;

        // Step 3: Semantic similarity check (scoped to session)
        let similar = self
            .long_term
            .retrieve(MemoryQuery {
                text: fact.to_string(),
                embedding: Some(embedding.clone()),
                kinds: vec![MemoryKind::UserFact],
                limit: 3,
                min_weight: 0.0,
                similarity_threshold: self.config.duplicate_similarity_threshold,
                session_id: Some(session_id.to_string()),
                user_id: Some(user_id.to_string()),
                ..Default::default()
            })
            .await?;

        if let Some(existing) = similar.entries.first() {
            if let Some(ref existing_emb) = existing.embedding {
                let sim = cosine_similarity(existing_emb, &embedding);
                if sim > self.config.duplicate_similarity_threshold {
                    // Semantic duplicate — merge content and promote
                    self.long_term.promote(&existing.id, 0.05).await?;
                    tracing::debug!(
                        "Semantic dedup: fact '{}' ~ existing {} (sim={:.3})",
                        fact,
                        existing.id,
                        sim
                    );
                    return Ok(());
                }
            }
        }

        // Step 3.5: Hierarchical dedup (synonym, containment, related detection)
        if let (Some(ref dedup_engine), Some(candidates)) = (&self.dedup_engine, hierarchical_candidates) {

            match dedup_engine
                .check_duplicate_with_embedding(fact, &embedding, &candidates)
            {
                Ok(dedup_result) => {
                    match dedup_result.recommended_action() {
                        DedupAction::MergeAndPromote { existing_id } => {
                            self.long_term.promote(&existing_id, 0.05).await?;
                            tracing::debug!(
                                "Hierarchical dedup: merged fact '{}' into {}",
                                fact,
                                existing_id
                            );
                            return Ok(());
                        }
                        DedupAction::SkipAsRedundant => {
                            tracing::debug!(
                                "Hierarchical dedup: skipped redundant fact '{}'",
                                fact
                            );
                            return Ok(());
                        }
                        DedupAction::ReplaceExisting { existing_id } => {
                            let _ = self.long_term.delete(&existing_id).await;
                            tracing::debug!(
                                "Hierarchical dedup: replacing {} with new fact",
                                existing_id
                            );
                            // Continue to store the new fact
                        }
                        DedupAction::StoreWithRelation { existing_id } => {
                            // Store and link — continue to store, then add relation
                            tracing::debug!(
                                "Hierarchical dedup: storing related fact alongside {}",
                                existing_id
                            );
                            // Will add relation after storage
                        }
                        DedupAction::StoreAsNew => {
                            // Normal path — continue to store
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Hierarchical dedup failed, proceeding with storage: {}", e);
                }
            }
        }

        // Step 4: New fact — store it
        let entry = MemoryEntryBuilder::new(
            MemoryKind::UserFact,
            fact,
            source_agent,
        )
        .session_id(session_id)
        .data(json!({"user_id": user_id}))
        .embedding(embedding)
        .weight(0.8)
        .tags(vec!["fact".to_string()])
        .confirmed(true)
        .content_hash(content_hash)
        .build();

        self.long_term.store(entry.clone()).await?;

        // Step 5: Detect contradictions (with optional LLM-based resolution)
        if self.config.enable_contradiction_detection {
            if let Some(ref llm) = self.llm_provider {
                // LLM-based contradiction resolution
                match self.resolve_contradictions_with_llm(&entry, llm).await {
                    Ok(resolutions) if !resolutions.is_empty() => {
                        tracing::info!(
                            "LLM detected {} contradictions for fact '{}'",
                            resolutions.len(),
                            fact
                        );
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(
                            "LLM contradiction resolution failed, falling back to rules: {}",
                            e
                        );
                        let _ = self
                            .long_term
                            .resolve_contradictions(&entry, self.config.similarity_threshold)
                            .await;
                    }
                }
            } else {
                // Rule-based contradiction resolution
                match self
                    .long_term
                    .resolve_contradictions(&entry, self.config.similarity_threshold)
                    .await
                {
                    Ok(contradicted) if !contradicted.is_empty() => {
                        tracing::info!(
                            "Detected {} contradictions for fact '{}'",
                            contradicted.len(),
                            fact
                        );
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("Contradiction detection failed: {}", e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Update user profile from accumulated facts and preferences
    pub async fn update_user_profile_from_memories(&self, user_id: &str) -> Result<()> {
        let facts = self
            .long_term
            .retrieve(MemoryQuery {
                kinds: vec![MemoryKind::UserFact, MemoryKind::InferredPreference],
                limit: 50,
                min_weight: 0.3,
                user_id: Some(user_id.to_string()),
                ..Default::default()
            })
            .await?;

        let current_profile = self
            .long_term
            .get_user_profile(user_id)
            .await?
            .unwrap_or_else(|| serde_json::json!({}));

        if let Some(ref llm) = self.llm_provider {
            match self
                .llm_merge_profile(user_id, &current_profile, &facts.entries, llm)
                .await
            {
                Ok(updated) => {
                    self.long_term.update_user_profile(user_id, updated).await?;
                }
                Err(e) => {
                    tracing::warn!("LLM profile merge failed: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Use LLM to merge facts into user profile
    async fn llm_merge_profile(
        &self,
        _user_id: &str,
        current: &serde_json::Value,
        facts: &[MemoryEntry],
        llm: &Arc<dyn LlmProvider>,
    ) -> std::result::Result<serde_json::Value, String> {
        let facts_text: String = facts
            .iter()
            .map(|f| format!("- {}", f.content))
            .collect::<Vec<_>>()
            .join("\n");

        let system = format!(
            "你是一个用户画像更新助手。请根据新的事实更新用户画像。
当前用户画像：
{}
新发现的事实：
{}

请输出更新后的完整JSON格式用户画像。只输出JSON，不要其他内容。",
            current, facts_text
        );

        let request = CompletionRequest {
            model: String::new(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "请更新用户画像".to_string(),
                cache_control: None,
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: Some(8192),
            temperature: Some(0.2),
            system: Some(system),
            stream: false,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
        };

        let resp = llm.complete(request).await.map_err(|e| e.to_string())?;
        let content = resp.content.trim();

        // Try to parse JSON from response
        serde_json::from_str(content).map_err(|e| format!("Failed to parse profile JSON: {}", e))
    }

    /// Build memory-enhanced prompt from working memory
    pub fn build_memory_prompt(working_memory: &[MemoryEntry]) -> String {
        agent_teams_core::context::build_memory_prompt_from_entries(working_memory)
    }

    /// Get reference to long-term store (for maintenance tasks)
    pub fn long_term_store(&self) -> &Arc<dyn MemoryStore> {
        &self.long_term
    }

    /// Get reference to short-term store (for maintenance tasks)
    pub fn short_term_store(&self) -> &Arc<dyn MemoryStore> {
        &self.short_term
    }

    /// Clear all memories from both stores
    pub async fn clear_all(&self) -> Result<(usize, usize)> {
        let short_count = self.short_term.clear().await.unwrap_or(0);
        let long_count = self.long_term.clear().await.unwrap_or(0);
        tracing::info!("Cleared all memories: short_term={}, long_term={}", short_count, long_count);
        Ok((short_count, long_count))
    }

    /// Get reference to embedding provider (for agent cache warming)
    pub fn embedding_provider(&self) -> &Arc<dyn EmbeddingProvider> {
        &self.embedding_provider
    }

    /// Sync an agent's output to the memory system
    /// 
    /// IMPORTANT: Agent outputs are stored in SHORT-TERM memory only.
    /// This prevents cross-session contamination where one session's agent
    /// output pollutes another session's retrieval results.
    pub async fn sync_agent_output(
        &self,
        session_id: &str,
        agent_id: &str,
        content: &str,
        quality: f32,
    ) -> Result<()> {
        if !self.config.enabled || content.is_empty() || quality < 0.3 {
            return Ok(());
        }

        // Validate input: check for suspicious content that might indicate hallucination
        if !self.validate_content(content) {
            tracing::warn!(
                "Agent {} output failed validation for session {}, skipping sync",
                agent_id, session_id
            );
            return Ok(());
        }

        let entry = MemoryEntryBuilder::new(
            MemoryKind::AgentOutput,
            content.chars().take(500).collect::<String>(),
            agent_id,
        )
        .session_id(session_id)
        .data(serde_json::json!({
            "agent_id": agent_id,
            "quality": quality,
        }))
        .weight(quality * 0.3)  // Lower weight to reduce influence
        .tags(vec![agent_id.to_string(), "agent_output".to_string()])
        .content_hash(compute_content_hash(content))
        .confidence(quality)
        .build();

        // Store in SHORT-TERM memory only (not long-term)
        // This ensures agent outputs don't persist across sessions
        self.short_term.store(entry).await?;
        tracing::debug!(
            "Synced agent {} output to SHORT-TERM memory for session {}",
            agent_id,
            session_id
        );
        Ok(())
    }

    /// Validate content for suspicious patterns that might indicate hallucination
    fn validate_content(&self, content: &str) -> bool {
        // Check for emoji patterns that often appear in hallucinated content
        let suspicious_patterns = ["😅", "😂", "🤣", "😊", "🤔", "👋"];
        for pattern in &suspicious_patterns {
            if content.contains(pattern) {
                tracing::warn!("Suspicious pattern '{}' detected in content", pattern);
                return false;
            }
        }
        
        // Check for unreasonable length
        if content.len() > 10000 {
            tracing::warn!("Content too long ({} chars), possible hallucination", content.len());
            return false;
        }
        
        true
    }

    /// Use LLM to detect and resolve contradictions between new entry and existing memories
    async fn resolve_contradictions_with_llm(
        &self,
        entry: &MemoryEntry,
        llm: &Arc<dyn LlmProvider>,
    ) -> std::result::Result<Vec<String>, String> {
        // Find similar memories (scoped to session)
        let similar = self
            .long_term
            .retrieve(MemoryQuery {
                text: entry.content.clone(),
                embedding: entry.embedding.clone(),
                kinds: vec![entry.kind.clone()],
                limit: 5,
                min_weight: 0.0,
                similarity_threshold: 0.7,
                session_id: entry.session_id.clone(),
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;

        let candidates: Vec<_> = similar
            .entries
            .iter()
            .filter(|e| e.id != entry.id && e.content != entry.content)
            .collect();

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        // Batch all candidates into a single LLM call
        let system = "你是一个记忆矛盾检测器。判断新信息与每条旧信息的关系。\n\
            对每条旧信息返回：Supersedes（新信息取代旧信息）、Contradicts（真正矛盾）、Related（相关但不矛盾）\n\
            按顺序返回，每行一个关系类型名称。".to_string();

        let mut prompt = format!("新信息：{}\n\n旧信息列表：\n", entry.content);
        for (i, existing) in candidates.iter().enumerate() {
            prompt.push_str(&format!("{}. {}\n", i + 1, existing.content));
        }

        let request = CompletionRequest {
            model: String::new(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt,
                cache_control: None,
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: Some(8192),
            temperature: Some(0.0),
            system: Some(system),
            stream: false,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
        };

        let mut resolutions = Vec::new();

        if let Ok(resp) = llm.complete(request).await {
            let lines: Vec<&str> = resp.content.trim().lines().collect();
            for (i, existing) in candidates.iter().enumerate() {
                let relation_type = match lines.get(i).map(|l| l.trim()) {
                    Some("Supersedes") => {
                        let _ = self.long_term.promote(&existing.id, -0.3).await;
                        agent_teams_core::memory::MemoryRelationType::Supersedes
                    }
                    Some("Contradicts") => {
                        let _ = self.long_term.promote(&existing.id, -0.2).await;
                        agent_teams_core::memory::MemoryRelationType::Contradicts
                    }
                    _ => agent_teams_core::memory::MemoryRelationType::Related,
                };

                let _ = self
                    .long_term
                    .add_relation(agent_teams_core::memory::MemoryRelation {
                        source_id: entry.id.clone(),
                        target_id: existing.id.clone(),
                        relation_type,
                        strength: 0.8,
                        created_at: Utc::now(),
                    })
                    .await;

                resolutions.push(existing.id.clone());
            }
        }

        Ok(resolutions)
    }

    /// Record negative feedback for a memory (user corrected or denied it)
    pub async fn record_negative_feedback(&self, memory_id: &str, reason: &str) -> Result<()> {
        if let Some(mut entry) = self.long_term.get_by_id(memory_id).await? {
            // Confidence decay: each negative feedback reduces by 30%
            entry.confidence *= 0.7;
            entry.confidence = entry.confidence.max(0.1);

            // Weight reduction
            entry.weight *= 0.5;
            entry.weight = entry.weight.max(0.05);

            self.long_term.store(entry).await?;

            tracing::info!(
                memory_id = %memory_id,
                reason = %reason,
                "Recorded negative feedback for memory"
            );
        }
        Ok(())
    }

    /// Generate a memory quality report
    pub async fn generate_quality_report(&self) -> Result<serde_json::Value> {
        // Retrieve all long-term memories for analysis
        let all_memories = self
            .long_term
            .retrieve(MemoryQuery {
                limit: 10000,
                min_weight: 0.0,
                ..Default::default()
            })
            .await?;

        let total = all_memories.entries.len();

        // Calculate average confidence
        let avg_confidence = if total > 0 {
            all_memories
                .entries
                .iter()
                .map(|e| e.confidence)
                .sum::<f32>()
                / total as f32
        } else {
            0.0
        };

        // Kind distribution
        let mut kind_distribution = std::collections::HashMap::new();
        for entry in &all_memories.entries {
            *kind_distribution
                .entry(entry.kind.to_string())
                .or_insert(0usize) += 1;
        }

        // Average weight
        let avg_weight = if total > 0 {
            all_memories.entries.iter().map(|e| e.weight).sum::<f32>() / total as f32
        } else {
            0.0
        };

        Ok(serde_json::json!({
            "total_memories": total,
            "avg_confidence": avg_confidence,
            "avg_weight": avg_weight,
            "kind_distribution": kind_distribution,
        }))
    }

    /// Smart pruning: remove memories based on relevance, access patterns, and age.
    /// 
    /// This is more intelligent than simple weight-based pruning:
    /// 1. Preserves frequently accessed memories
    /// 2. Preserves recently accessed memories
    /// 3. Preserves high-confidence memories
    /// 4. Preserves memories with many relations
    /// 
    /// Returns: (pruned_count, preserved_count)
    pub async fn smart_prune(
        &self,
        max_memories: usize,
        min_access_count: u32,
        max_age_days: i64,
    ) -> Result<(usize, usize)> {
        if !self.config.enabled {
            return Ok((0, 0));
        }

        // Retrieve all long-term memories
        let all_memories = self
            .long_term
            .retrieve(MemoryQuery {
                limit: 10000,
                min_weight: 0.0,
                ..Default::default()
            })
            .await?;

        let total = all_memories.entries.len();
        if total <= max_memories {
            tracing::debug!(
                "Smart prune: total={}, max={}, no pruning needed",
                total, max_memories
            );
            return Ok((0, total));
        }

        // Calculate pruning score for each memory
        // Lower score = more likely to be pruned
        let now = Utc::now();
        let cutoff = now - chrono::Duration::days(max_age_days);

        let mut scored_memories: Vec<(f32, &MemoryEntry)> = all_memories
            .entries
            .iter()
            .map(|entry| {
                let mut score = 0.0;

                // Factor 1: Weight (0-1 points)
                score += entry.weight;

                // Factor 2: Access count (0-0.5 points)
                let access_score = (entry.access_count as f32).ln().max(0.0) / 10.0;
                score += access_score.min(0.5);

                // Factor 3: Recency (0-0.5 points)
                let age_hours = (now - entry.last_accessed_at).num_hours().max(0) as f32;
                let recency_score = 1.0 / (1.0 + age_hours / 168.0);  // 168 hours = 1 week
                score += recency_score * 0.5;

                // Factor 4: Confidence (0-0.3 points)
                score += entry.confidence * 0.3;

                // Factor 5: Has relations (0-0.2 points)
                if entry.parent_id.is_some() || !entry.compressed_from.is_empty() {
                    score += 0.2;
                }

                // Factor 6: Is confirmed (0-0.2 points)
                if entry.confirmed {
                    score += 0.2;
                }

                (score, entry)
            })
            .collect();

        // Sort by score (highest first)
        scored_memories.sort_by(|a, b| b.0.total_cmp(&a.0));

        // Determine which memories to prune
        let to_preserve = &scored_memories[..max_memories];
        let to_prune = &scored_memories[max_memories..];

        let mut pruned_count = 0;
        let mut preserved_count = 0;

        // Prune low-score memories
        for (score, entry) in to_prune {
            // Skip memories that are too recent
            if entry.created_at > cutoff {
                preserved_count += 1;
                continue;
            }

            // Skip memories with high access count
            if entry.access_count >= min_access_count {
                preserved_count += 1;
                continue;
            }

            // Archive the memory (soft delete)
            if let Err(e) = self.long_term.archive(&entry.id).await {
                tracing::warn!("Failed to archive memory {}: {}", entry.id, e);
            } else {
                pruned_count += 1;
                tracing::debug!(
                    "Smart prune: archived memory {} (score={:.2}, weight={:.2}, access={})",
                    entry.id, score, entry.weight, entry.access_count
                );
            }
        }

        // Count preserved memories
        preserved_count += to_preserve.len();

        tracing::info!(
            "Smart prune completed: pruned={}, preserved={}, total={}",
            pruned_count,
            preserved_count,
            total
        );

        Ok((pruned_count, preserved_count))
    }
}

/// Start memory maintenance background tasks
/// - Daily: decay old memory weights
/// - Hourly: prune low-weight memories + cleanup expired short-term memories
pub fn start_memory_maintenance(memory_manager: Arc<MemoryManager>) {
    tokio::spawn(async move {
        let mut decay_interval = interval(Duration::from_secs(86400)); // daily
        let mut prune_interval = interval(Duration::from_secs(3600)); // hourly

        // Skip the first immediate tick
        decay_interval.tick().await;
        prune_interval.tick().await;

        loop {
            tokio::select! {
                _ = decay_interval.tick() => {
                    let before = Utc::now() - chrono::Duration::days(7);
                    let factor = memory_manager.config().daily_decay_factor;

                    match memory_manager.long_term_store().decay(before, factor).await {
                        Ok(count) => tracing::info!("Decayed {} memory entries", count),
                        Err(e) => tracing::error!("Memory decay failed: {}", e),
                    }
                }

                _ = prune_interval.tick() => {
                    let before = Utc::now() - chrono::Duration::days(30);

                    // Prune low-weight long-term memories
                    match memory_manager.long_term_store().prune(0.1, before).await {
                        Ok(count) => tracing::info!("Pruned {} low-weight memories", count),
                        Err(e) => tracing::error!("Memory prune failed: {}", e),
                    }

                    // Cleanup expired short-term memories
                    // The InMemoryMemoryStore handles this via TTL in retrieve(),
                    // but for Postgres we call the cleanup function
                    let short_term_before = Utc::now() - chrono::Duration::seconds(
                        memory_manager.config().short_term_ttl_secs as i64
                    );
                    match memory_manager.short_term_store().prune(0.0, short_term_before).await {
                        Ok(count) if count > 0 => tracing::info!("Cleaned up {} expired short-term memories", count),
                        Ok(_) => {}
                        Err(e) => tracing::error!("Short-term cleanup failed: {}", e),
                    }
                }
            }
        }
    });
}
