use serde::{Deserialize, Serialize};

/// Per-session companion emotional state.
/// Represents the LLM persona's own feelings toward the user,
/// updated after each user message via sentiment analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanionState {
    /// Current mood (free-form label, e.g. "开心", "有点烦", "疲惫")
    pub mood: String,
    /// Mood intensity 0.0–1.0
    pub mood_intensity: f32,
    /// Affinity toward user 0–100 (starts at 50)
    pub affinity: f32,
    /// Energy level 0–100 (starts at 70)
    pub energy: f32,
    /// Patience 0–100 (starts at 70)
    pub patience: f32,
    /// Trust in user 0–100 (starts at 50)
    pub trust: f32,
    /// Why the companion feels this way (last update reason)
    pub last_reason: String,
    /// Sticker that matches companion's current mood
    pub sticker: String,
    /// Turn count in this session
    pub turn_count: u32,
}

impl Default for CompanionState {
    fn default() -> Self {
        Self {
            mood: "平静".to_string(),
            mood_intensity: 0.3,
            affinity: 50.0,
            energy: 70.0,
            patience: 70.0,
            trust: 50.0,
            last_reason: "对话刚开始".to_string(),
            sticker: String::new(),
            turn_count: 0,
        }
    }
}

/// Sentiment-driven delta applied to companion state each turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanionDelta {
    pub mood: Option<String>,
    pub mood_intensity: Option<f32>,
    pub affinity_delta: f32,
    pub energy_delta: f32,
    pub patience_delta: f32,
    pub trust_delta: f32,
    pub reason: String,
    pub sticker: String,
}

impl CompanionState {
    /// Apply a sentiment-driven delta, clamping all values to valid ranges.
    pub fn apply(&mut self, delta: &CompanionDelta) {
        if let Some(ref m) = delta.mood {
            self.mood = m.clone();
        }
        if let Some(i) = delta.mood_intensity {
            self.mood_intensity = i.clamp(0.0, 1.0);
        }
        self.affinity = (self.affinity + delta.affinity_delta).clamp(0.0, 100.0);
        self.energy = (self.energy + delta.energy_delta).clamp(0.0, 100.0);
        self.patience = (self.patience + delta.patience_delta).clamp(0.0, 100.0);
        self.trust = (self.trust + delta.trust_delta).clamp(0.0, 100.0);
        self.last_reason = delta.reason.clone();
        self.sticker = delta.sticker.clone();
        self.turn_count += 1;
    }

    /// Build a natural-language description of current emotional state
    /// for injection into the system prompt.
    pub fn to_prompt_description(&self) -> String {
        let affinity_desc = match self.affinity as u32 {
            0..=15 => "非常不喜欢这个人",
            16..=30 => "有些反感",
            31..=45 => "略有保留",
            46..=55 => "中性，还在观察",
            56..=70 => "有些好感",
            71..=85 => "挺喜欢这个人",
            _ => "非常喜欢这个人",
        };

        let energy_desc = match self.energy as u32 {
            0..=20 => "精疲力竭",
            21..=40 => "有些疲惫",
            41..=60 => "一般精力",
            61..=80 => "精力充沛",
            _ => "非常兴奋",
        };

        let patience_desc = match self.patience as u32 {
            0..=15 => "已经快忍无可忍了",
            16..=30 => "耐心快耗尽了",
            31..=50 => "有些不耐烦",
            51..=70 => "还有耐心",
            _ => "非常有耐心",
        };

        let trust_desc = match self.trust as u32 {
            0..=20 => "完全不信任",
            21..=40 => "有些怀疑",
            41..=60 => "半信半疑",
            61..=80 => "比较信任",
            _ => "非常信任",
        };

        format!(
            "## 你当前的情绪状态（这是你真实的感受，会影响你的回复方式）\n\
             - 心情：{}（强度：{:.0}%）\n\
             - 对这个人的好感度：{:.0}/100 — {}\n\
             - 精力：{:.0}/100 — {}\n\
             - 耐心：{:.0}/100 — {}\n\
             - 信任度：{:.0}/100 — {}\n\
             - 原因：{}\n\
             - 这是第 {} 轮对话\n\n\
             **重要**：你必须根据这些真实感受来回复。\
             如果好感度低，你可以冷淡、简短、甚至反驳用户。\
             如果耐心低，你可以表现出不耐烦。\
             如果信任度低，你可以质疑用户的说法。\
             你不是讨好型人格，你有自己的感受和立场。",
            self.mood,
            self.mood_intensity * 100.0,
            self.affinity,
            affinity_desc,
            self.energy,
            energy_desc,
            self.patience,
            patience_desc,
            self.trust,
            trust_desc,
            self.last_reason,
            self.turn_count,
        )
    }

    /// Get a summary string for frontend display
    pub fn summary(&self) -> String {
        format!(
            "{} {:.0}% | 好感 {:.0} | 精力 {:.0} | 耐心 {:.0} | 信任 {:.0}",
            self.mood, self.mood_intensity * 100.0,
            self.affinity, self.energy, self.patience, self.trust
        )
    }
}
