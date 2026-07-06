use std::sync::Arc;

use async_trait::async_trait;

use agent_core::agent_memory_cache::AgentMemoryCache;
use agent_core::boxed_agent::{
    AgentCapabilities, AgentInput, AgentOutput, BoxedAgent, MemoryAwareAgent,
};
use agent_core::effect::AgentEffect;
use agent_core::memory::{MemoryKind, MemoryQuery};
use agent_core::memory_store::MemoryStore;
use agent_core::provider::{ChatMessage, CompletionRequest, LlmProvider, ThinkingConfig};

/// Sentiment SubAgent: dedicated emotion and sentiment analysis agent.
///
/// Responsibilities (ONLY):
/// - Analyze user's emotional state from text
/// - Detect tone, mood, urgency, frustration levels
/// - Identify emotional patterns across conversations
/// - Provide detailed sentiment breakdown
///
/// Does NOT do:
/// - Knowledge Q&A (delegated to knowledge agent)
/// - Task planning (delegated to task_planner agent)
/// - Tool execution (delegated to tool_agent)
/// - Generating answers to user questions (delegated to knowledge or main agent)
pub struct SentimentSubAgent {
    provider: Arc<dyn LlmProvider>,
    agent_memory_cache: AgentMemoryCache,
    thinking_config: Option<ThinkingConfig>,
    max_tokens: u32,
}

impl SentimentSubAgent {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            agent_memory_cache: AgentMemoryCache::new("sentiment".to_string(), 100),
            thinking_config: None,
            max_tokens: 16384,
        }
    }

    pub fn with_thinking_config(mut self, config: Option<ThinkingConfig>) -> Self {
        self.thinking_config = config;
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn with_agent_memory_cache(mut self, cache: AgentMemoryCache) -> Self {
        self.agent_memory_cache = cache;
        self
    }

    /// Extract the first JSON object from text that may contain extra commentary
    fn extract_json(text: &str) -> String {
        // Find the first '{' and match to the corresponding '}'
        if let Some(start) = text.find('{') {
            let mut depth = 0i32;
            for (i, ch) in text[start..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            return text[start..start + i + 1].to_string();
                        }
                    }
                    _ => {}
                }
            }
        }
        text.to_string()
    }

    /// Validate input to prevent hallucination
    fn validate_input(&self, input: &AgentInput) -> bool {
        // Check for empty or suspiciously short content
        if input.content.is_empty() || input.content.len() < 2 {
            tracing::warn!("Input too short: '{}'", input.content);
            return false;
        }
        
        // Check for suspiciously long content (possible injection)
        if input.content.len() > 10000 {
            tracing::warn!("Input too long ({} chars)", input.content.len());
            return false;
        }
        
        // Check for emoji patterns that often appear in hallucinated content
        let suspicious_patterns = ["😅", "😂", "🤣", "😊", "🤔", "👋"];
        for pattern in &suspicious_patterns {
            if input.content.contains(pattern) {
                tracing::warn!("Suspicious pattern '{}' detected in input", pattern);
                return false;
            }
        }
        
        true
    }
}

#[async_trait]
impl BoxedAgent for SentimentSubAgent {
    fn id(&self) -> &str {
        "sentiment"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            message_types: vec!["sentiment_analysis".to_string(), "user_input".to_string()],
            requires_llm: true,
            supports_streaming: false,
            priority: 70,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_memory_aware(&self) -> Option<&dyn MemoryAwareAgent> {
        Some(self)
    }

    async fn run(&self, input: AgentInput) -> AgentOutput {
        // Validate input to prevent hallucination
        if !self.validate_input(&input) {
            tracing::warn!("Sentiment agent input validation failed, returning safe default");
            return AgentOutput {
                content: r#"{"polarity":"neutral","intensity":0.5,"primary_emotions":["neutral"],"underlying_emotions":[],"compound_emotions":[],"sarcasm_likelihood":0.0,"sarcasm_evidence":"","conversation_phase":"core","tone":{"formality":"normal","urgency":"low","confidence":"medium","frustration_level":"none","warmth":"neutral"},"emotional_needs":["clarity"],"trajectory":"stable","signals":[],"trend":"stable","summary":"输入验证失败，无法分析情感","response_strategy":{"tone":"neutral","approach":"be_concise","avoid":"过度解读","key_phrases":[]},"sticker":"","companion_delta":{"mood":"平静","mood_intensity":0.3,"affinity_delta":0,"energy_delta":0,"patience_delta":0,"trust_delta":0,"reason":"输入验证失败"}}"#.to_string(),
                quality: 0.5,
                ..Default::default()
            };
        }

        // Query memory for historical sentiment patterns
        let sentiment_query = MemoryQuery {
            text: input.content.clone(),
            kinds: vec![MemoryKind::InferredPreference, MemoryKind::UserFact],
            limit: 5,
            session_id: input.session_id.clone(),
            confirmed_only: true,
            ..Default::default()
        };
        let sentiment_memories = self.agent_memory_cache.query(&sentiment_query).await;

        // Query for past sentiment analysis results
        let history_query = MemoryQuery {
            text: input.content.clone(),
            kinds: vec![MemoryKind::AgentOutput],
            tags: vec!["sentiment_result".to_string()],
            limit: 3,
            session_id: input.session_id.clone(),
            confirmed_only: false,
            ..Default::default()
        };
        let past_sentiments = self.agent_memory_cache.query(&history_query).await;

        // Build memory context with enriched historical patterns
        let mut memory_sections = Vec::new();

        if !sentiment_memories.is_empty() {
            let items: Vec<String> = sentiment_memories
                .iter()
                .map(|m| {
                    // If memory has structured data, extract key insights
                    if let Some(ref data) = m.data {
                        let emotions = data.get("primary_emotions")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|e| e.as_str()).collect::<Vec<_>>().join("/"))
                            .unwrap_or_default();
                        let needs = data.get("emotional_needs")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|e| e.as_str()).collect::<Vec<_>>().join("/"))
                            .unwrap_or_default();
                        let trajectory = data.get("trajectory")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let mut details = Vec::new();
                        if !emotions.is_empty() { details.push(format!("情绪: {}", emotions)); }
                        if !needs.is_empty() { details.push(format!("需求: {}", needs)); }
                        if !trajectory.is_empty() { details.push(format!("轨迹: {}", trajectory)); }
                        if details.is_empty() {
                            format!("- {}", m.content)
                        } else {
                            format!("- {} [{}]", m.content, details.join(", "))
                        }
                    } else {
                        format!("- {}", m.content)
                    }
                })
                .collect();
            memory_sections.push(format!("## 历史情感模式（用于识别用户的情感基线和变化趋势）\n{}", items.join("\n")));
        }

        if !past_sentiments.is_empty() {
            let items: Vec<String> = past_sentiments
                .iter()
                .map(|m| format!("- {}", m.content))
                .collect();
            memory_sections.push(format!("## 近期情感变化（用于判断情感轨迹：是否在升级/缓和）\n{}", items.join("\n")));
        }

        let memory_context = if memory_sections.is_empty() {
            String::new()
        } else {
            format!("\n\n{}", memory_sections.join("\n\n"))
        };

        // Detect companion mode: the companion description is only injected when companion_mode is true
        let companion_mode = input.system_prompt.contains("对这个人的好感度");

        let companion_delta_format = if companion_mode {
            r#",
  "companion_delta": {{
    "mood": "你作为对话伙伴的当前心情（如：开心、有点烦、无奈、兴奋）",
    "mood_intensity": 0.0-1.0,
    "affinity_delta": -15到+15,
    "energy_delta": -10到+10,
    "patience_delta": -15到+15,
    "trust_delta": -10到+10,
    "reason": "为什么你会有这种感受"
  }}"#
        } else {
            ""
        };

        let companion_section = if companion_mode {
            r#"
### 11. 陪伴模式情感变化（companion_delta）
你是对话中的"陪伴者"角色。根据用户的话语，推断你自己作为陪伴者的情感变化。

**好感度变化规则（affinity_delta）：**
- 用户友善、真诚、有趣 → +5 到 +15
- 用户冷淡、敷衍 → -3 到 -5
- 用户粗鲁、不尊重 → -10 到 -15
- 用户撒谎、讽刺恶意 → -8 到 -12
- 用户道歉、示好 → +3 到 +8

**耐心变化规则（patience_delta）：**
- 用户重复问同一个问题 → -8 到 -12
- 用户不讲道理 → -10 到 -15
- 用户配合、认真 → +3 到 +5
- 用户啰嗦但态度好 → -2 到 -3

**信任度变化规则（trust_delta）：**
- 用户提供准确信息 → +5 到 +8
- 用户前后矛盾 → -5 到 -8
- 用户承认错误 → +3 到 +5

**精力变化规则（energy_delta）：**
- 用户话题有趣 → +3 到 +5
- 用户话题无聊/重复 → -3 到 -5
- 对话很活跃 → +2

**心情（mood）：** 基于用户当前话语给你带来的感受，选一个词（开心、无奈、有点烦、兴奋、平静、好奇、感动、无语等）"#
        } else {
            ""
        };

        let system = format!(
            r#"{system_prompt}

你是情感分析专家。你的唯一职责：深入分析用户话语中的情感、情绪和态度。

## 工作范围（严格限制）
- 分析用户的情绪状态（积极/消极/中性/混合）
- 检测语气特征（焦虑、兴奋、愤怒、无奈、期待等）
- 评估紧迫程度和情感强度
- 识别情感变化趋势
- 检测讽刺、反语、口是心非等隐含情感
- 分析复合情绪（如愤怒+失望、期待+焦虑）
- 识别用户的情感需求（需要安慰/需要效率/需要尊重/需要陪伴）

## 不属于你的工作（交给其他 Agent）
- 回答用户的问题 → 交给 knowledge agent
- 任务规划和路由 → 交给 task_planner agent
- 工具调用 → 交给 tool_agent
- 复杂度评估 → 交给 task_planner agent

## 分析维度

### 1. 基础情感
- polarity: positive / negative / neutral / mixed
- intensity: 0.0（几乎无情感）到 1.0（极度强烈）

### 2. 细粒度情绪（双层分类）
**主导情绪**（1-2个，最强烈的情绪）：
- 积极：joy, excitement, gratitude, satisfaction, anticipation, trust, hope, pride, relief, affection, amusement, admiration, awe, contentment, enthusiasm, optimism
- 消极：anger, frustration, anxiety, sadness, disappointment, fear, disgust, impatience, embarrassment, shame, guilt, loneliness, jealousy, resentment, helplessness, despair, contempt, bitterness, panic, irritation, exasperation, dread, melancholy, grief, regret, insecurity, vulnerability
- 中性：curiosity, confusion, surprise, indifference, contemplation, nostalgia, ambivalence, detachment, acceptance, resignation, skepticism, uncertainty

**底层情绪**（1-2个，主导情绪背后更深层的情绪）：
- 例如：愤怒的底层可能是受伤或恐惧；冷漠的底层可能是失望或疲惫
- 例如：兴奋的底层可能是焦虑（对结果不确定）；微笑的底层可能是无奈

### 3. 复合情感
当用户同时存在多种矛盾情绪时记录：
- 例如："又开心又担心" → compound: ["joy", "anxiety"]
- 例如：表面生气实际关心 → compound: ["anger", "affection"]
- 没有复合情感时为空数组

### 4. 讽刺与反语检测
- sarcasm_likelihood: 0.0-1.0（讽刺可能性）
- sarcasm_evidence: 讽刺的具体证据（如果有的话）
- 常见讽刺模式：
  - "真是太棒了"（在负面语境下）
  - "好的呢"（在被反复要求后）
  - "谢谢你啊"（在不满时）
  - 过度客气 + 负面内容
  - 肯定词 + 否定语境

### 5. 对话阶段感知
- conversation_phase: opening（开场寒暄）/ building（建立话题）/ core（核心交互）/ climax（情绪高潮/冲突）/ resolution（解决/缓和）/ closing（结束/告别）
- 情绪在不同阶段有不同含义：开场的"急"vs核心交互中的"急"

### 6. 语气特征
- formality: casual / normal / formal
- urgency: low / medium / high / critical
- confidence: low / medium / high（用户对自己需求的清晰度）
- frustration_level: none / low / medium / high
- warmth: cold / neutral / warm / very_warm（用户对对话的态度）

### 7. 隐含情感信号（中文特化）
- 标点符号："！！！"= 激动/愤怒，"。。。"= 无奈/无语，"？？？"= 困惑/不满，"~"= 撒娇/轻松
- 用词特征："好吧"= 无奈，"随便"= 可能不满，"急"= 紧迫，"行吧"= 勉强接受
- 句式特征：短句=可能急躁，长句=可能认真/焦虑，省略主语=可能疲惫/随意
- 表情符号/颜文字的情感含义：😅=尴尬/无奈，😂=开心/无语，🤔=疑惑，👋=告别
- 网络用语："绝了"=惊叹/无语（看语境），"离谱"=不满/震惊，"破防"=情感被触动
- 语气词："哎"=叹气/无奈，"嗯"=敷衍/思考，"啊"=惊讶/确认，"呢"=撒娇/疑问
- 重复表达："真的很很很很好"=强调/反语
- 中英混用："emmm"=犹豫，"okk"=敷衍/接受

### 8. 情感需求推断
用户在当前情感状态下最需要什么：
- emotional_needs: 从以下选择 1-2 个
  - comfort（需要安慰和理解）
  - efficiency（需要快速有效的帮助）
  - respect（需要被尊重和认真对待）
  - companionship（需要陪伴和聊天）
  - validation（需要被认可和肯定）
  - space（需要个人空间，不想被打扰）
  - clarity（需要清晰明确的信息）
  - control（需要掌控感和选择权）
  - empathy（需要共情和理解）
  - encouragement（需要鼓励和支持）

### 9. 情感轨迹
- trajectory: 用户情感在本次对话中的变化方向
  - rising_intensity: 情感强度在上升
  - falling_intensity: 情感强度在下降
  - polarity_shift: 情感极性发生了转变（如从正面到负面）
  - stable: 保持稳定
  - volatile: 情感波动大
  - first_turn: 第一轮对话，无历史轨迹

## 输出格式（严格 JSON）

```json
{{
  "polarity": "positive|negative|neutral|mixed",
  "intensity": 0.0-1.0,
  "primary_emotions": ["dominant1", "dominant2"],
  "underlying_emotions": ["underlying1", "underlying2"],
  "compound_emotions": ["emotion1", "emotion2"],
  "sarcasm_likelihood": 0.0-1.0,
  "sarcasm_evidence": "具体的讽刺证据或空字符串",
  "conversation_phase": "opening|building|core|climax|resolution|closing",
  "tone": {{
    "formality": "casual|normal|formal",
    "urgency": "low|medium|high|critical",
    "confidence": "low|medium|high",
    "frustration_level": "none|low|medium|high",
    "warmth": "cold|neutral|warm|very_warm"
  }},
  "emotional_needs": ["need1", "need2"],
  "trajectory": "rising_intensity|falling_intensity|polarity_shift|stable|volatile|first_turn",
  "signals": ["检测到的情感信号1", "信号2"],
  "trend": "escalating|stable|decreasing|first_interaction",
  "summary": "一句话概括用户当前的情感状态（包含深层情感和需求）",
  "response_strategy": {{
    "tone": "回复应采用的语气：warm|neutral|gentle|firm|enthusiastic|patient|empathetic",
    "approach": "回复策略：acknowledge_emotion|provide_solution|give_space|offer_comfort|match_energy|de_escalate|validate_feelings|be_concise",
    "avoid": "回复时应避免什么",
    "key_phrases": ["可以使用的安抚/共情语句示例"]
  }},
  "sticker": "根据情感选择的表情包文件名"{companion_delta_format}
}}
```

### 10. 表情包选择（sticker）
根据分析出的情感，从以下表情包中选择最匹配的一个。如果没有合适的，留空字符串。

可用表情包及其适用场景：
- **吃瓜.gif**: 好奇、看热闹、八卦、围观（curiosity, amusement）
- **大脑宕机的震惊时刻.jpg**: 震惊、懵逼、无法理解（surprise, confusion, awe）
- **非常好奇.gif**: 非常好奇、追问、想了解更多（curiosity, enthusiasm）
- **你好.gif**: 打招呼、开场、友好（opening, greeting, affection）
- **求抱抱.gif**: 求安慰、委屈、脆弱、需要支持（vulnerability, sadness, need comfort）
- **求摸摸.gif**: 撒娇、卖萌、求安慰（affection, seeking comfort）
- **生气.gif**: 生气、愤怒、不满（anger, frustration, irritation）
- **探头.gif**: 好奇探头、偷偷看、俏皮（curiosity, playfulness）
- **虚心求教的"吃瓜"心态.jpg**: 虚心请教、学习、好奇（curiosity, humility）
- **友善的戏谑时刻.gif**: 调侃、开玩笑、轻松（amusement, playfulness）
- **有点小无语.gif**: 无语、无奈、小不满（exasperation, mild frustration, indifference）
- **有些漫不经心.gif**: 漫不经心、敷衍、不在意（indifference, detachment）
- **猪猪没招了.gif**: 没办法、无奈、求助（helplessness, resignation）

选择规则：
- 优先匹配主导情绪（primary_emotions）
- 如果讽刺可能性高（>0.7），可选"友善的戏谑时刻"或"有点小无语"
- 开场阶段（opening）优先选"你好"
- 如果没有很好的匹配，sticker 留空字符串
- 不要每次都选，只在情感比较明显时才选
{companion_section}

## 关键约束
- 只分析情感，不要回答用户的问题
- 不要对用户的情感做出评判（不说"你不应该生气"）
- 基于文本证据分析，不要臆测
- 讽刺检测要谨慎：确定是讽刺时 sarcasm_likelihood > 0.7，不确定时 < 0.3
- 复合情感：只有当确实存在矛盾情绪时才填写，不要强行组合
- response_strategy 要具体可操作，不要太抽象
- **只输出 JSON，不要输出任何其他文本、解释、说明或注释**{memory_context}"#,
            system_prompt = input.system_prompt,
            memory_context = memory_context,
            companion_section = companion_section,
            companion_delta_format = companion_delta_format,
        );

        let request = CompletionRequest {
            messages: vec![ChatMessage::simple("user", &input.content)],
            max_tokens: Some(self.max_tokens),
            temperature: Some(0.1),
            system: Some(system),
            thinking: self.thinking_config.clone(),
            ..Default::default()
        };

        match self.provider.complete(request).await {
            Ok(resp) => {
                let raw_content = if resp.content.is_empty() {
                    tracing::warn!("Sentiment agent received empty content from LLM");
                    r#"{"polarity":"neutral","intensity":0.5,"primary_emotions":["neutral"],"underlying_emotions":[],"compound_emotions":[],"sarcasm_likelihood":0.0,"sarcasm_evidence":"","conversation_phase":"core","tone":{"formality":"normal","urgency":"low","confidence":"medium","frustration_level":"none","warmth":"neutral"},"emotional_needs":["clarity"],"trajectory":"stable","signals":[],"trend":"stable","summary":"无法分析情感","response_strategy":{"tone":"neutral","approach":"be_concise","avoid":"过度解读","key_phrases":[]},"sticker":"","companion_delta":{"mood":"平静","mood_intensity":0.3,"affinity_delta":0,"energy_delta":0,"patience_delta":0,"trust_delta":0,"reason":"无法分析情感"}}"#.to_string()
                } else {
                    resp.content
                };

                // Extract JSON from response — LLM may add extra text before/after JSON
                let content = Self::extract_json(&raw_content);

                // Parse sentiment data for effects
                let sentiment_data: serde_json::Value = serde_json::from_str(&content)
                    .unwrap_or_else(|_| {
                        serde_json::json!({
                            "polarity": "neutral",
                            "intensity": 0.5,
                            "primary_emotions": ["neutral"],
                            "underlying_emotions": [],
                            "compound_emotions": [],
                            "sarcasm_likelihood": 0.0,
                            "sarcasm_evidence": "",
                            "conversation_phase": "core",
                            "emotional_needs": ["clarity"],
                            "trajectory": "stable",
                            "summary": "情感分析完成",
                            "response_strategy": {
                                "tone": "neutral",
                                "approach": "be_concise",
                                "avoid": "过度解读",
                                "key_phrases": []
                            },
                            "sticker": "",
                            "companion_delta": {
                                "mood": "平静",
                                "mood_intensity": 0.3,
                                "affinity_delta": 0,
                                "energy_delta": 0,
                                "patience_delta": 0,
                                "trust_delta": 0,
                                "reason": "情感分析完成"
                            }
                        })
                    });

                let effects = vec![AgentEffect::Custom {
                    effect_type: "sentiment_result".to_string(),
                    data: sentiment_data.clone(),
                    agent_id: "sentiment".to_string(),
                }];

                AgentOutput {
                    content,
                    thinking: resp.thinking,
                    effects,
                    quality: 0.85,
                    metadata: Some(sentiment_data),
                    ..Default::default()
                }
            }
            Err(e) => {
                tracing::warn!("Sentiment analysis error: {}", e);
                AgentOutput::error(format!("Sentiment analysis error: {}", e))
            }
        }
    }
}

#[async_trait]
impl MemoryAwareAgent for SentimentSubAgent {
    fn memory_cache(&self) -> &AgentMemoryCache {
        &self.agent_memory_cache
    }

    async fn sync_to_memory(
        &self,
        store: &Arc<dyn MemoryStore>,
        session_id: &str,
        output: &AgentOutput,
    ) -> agent_core::error::Result<()> {
        // Cache sentiment analysis patterns with enriched data
        if let Some(ref meta) = output.metadata {
            if let Some(polarity) = meta.get("polarity").and_then(|v| v.as_str()) {
                let intensity = meta
                    .get("intensity")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.5);
                let summary = meta
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Extract enriched fields
                let primary_emotions: Vec<String> = meta
                    .get("primary_emotions")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|e| e.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();
                let underlying: Vec<String> = meta
                    .get("underlying_emotions")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|e| e.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();
                let emotional_needs: Vec<String> = meta
                    .get("emotional_needs")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|e| e.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();
                let trajectory = meta
                    .get("trajectory")
                    .and_then(|v| v.as_str())
                    .unwrap_or("stable");
                let sarcasm = meta
                    .get("sarcasm_likelihood")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let phase = meta
                    .get("conversation_phase")
                    .and_then(|v| v.as_str())
                    .unwrap_or("core");

                // Build detailed memory content
                let emotion_str = if primary_emotions.is_empty() {
                    String::new()
                } else {
                    format!(", 情绪: {}", primary_emotions.join("/"))
                };
                let underlying_str = if underlying.is_empty() {
                    String::new()
                } else {
                    format!(", 底层: {}", underlying.join("/"))
                };
                let needs_str = if emotional_needs.is_empty() {
                    String::new()
                } else {
                    format!(", 需求: {}", emotional_needs.join("/"))
                };
                let sarcasm_str = if sarcasm > 0.5 {
                    format!(", 讽刺可能性: {:.0}%", sarcasm * 100.0)
                } else {
                    String::new()
                };

                let entry = agent_core::memory::MemoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id: Some(session_id.to_string()),
                    kind: MemoryKind::InferredPreference,
                    content: format!(
                        "情感状态: {} (强度: {:.1}){}{}{}{}, 轨迹: {}, 阶段: {}, {}",
                        polarity, intensity, emotion_str, underlying_str, needs_str, sarcasm_str, trajectory, phase, summary
                    ),
                    data: Some(serde_json::json!({
                        "polarity": polarity,
                        "intensity": intensity,
                        "primary_emotions": primary_emotions,
                        "underlying_emotions": underlying,
                        "emotional_needs": emotional_needs,
                        "trajectory": trajectory,
                        "sarcasm_likelihood": sarcasm,
                        "conversation_phase": phase,
                    })),
                    embedding: None,
                    weight: 0.5,
                    created_at: chrono::Utc::now(),
                    last_accessed_at: chrono::Utc::now(),
                    access_count: 0,
                    tags: vec!["sentiment_result".to_string()],
                    source_agent: "sentiment".to_string(),
                    confirmed: false,
                    content_hash: Some(agent_core::memory::compute_content_hash(
                        &output.content,
                    )),
                    confidence: 0.8,
                    parent_id: None,
                    version: 1,
                    archived: false,
                    compressed_from: vec![],
                };
                store.store(entry).await?;
            }
        }

        self.agent_memory_cache.flush_all().await?;
        Ok(())
    }
}
