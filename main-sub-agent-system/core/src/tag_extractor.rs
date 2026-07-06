use regex::Regex;
use std::collections::HashMap;

/// Tag extractor with dictionary-based matching and entity patterns
pub struct TagExtractor {
    /// Predefined tag dictionary (canonical -> synonyms)
    tag_dictionary: HashMap<String, Vec<String>>,
    /// Entity extraction patterns
    entity_patterns: Vec<Regex>,
}

impl TagExtractor {
    pub fn new() -> Self {
        let mut tag_dictionary = HashMap::new();

        // Common Chinese topic tags with synonyms
        tag_dictionary.insert(
            "产品".to_string(),
            vec![
                "商品".to_string(),
                "物品".to_string(),
                "product".to_string(),
            ],
        );
        tag_dictionary.insert(
            "价格".to_string(),
            vec![
                "多少钱".to_string(),
                "费用".to_string(),
                "cost".to_string(),
                "price".to_string(),
            ],
        );
        tag_dictionary.insert(
            "订单".to_string(),
            vec!["购买".to_string(), "下单".to_string(), "order".to_string()],
        );
        tag_dictionary.insert(
            "退款".to_string(),
            vec!["退货".to_string(), "退钱".to_string(), "refund".to_string()],
        );
        tag_dictionary.insert(
            "投诉".to_string(),
            vec![
                "不满".to_string(),
                "差评".to_string(),
                "complaint".to_string(),
            ],
        );
        tag_dictionary.insert(
            "偏好".to_string(),
            vec![
                "喜欢".to_string(),
                "习惯".to_string(),
                "preference".to_string(),
            ],
        );
        tag_dictionary.insert(
            "技术".to_string(),
            vec!["bug".to_string(), "故障".to_string(), "问题".to_string()],
        );

        // Entity patterns: dates, numbers, emails, etc.
        let entity_patterns = vec![
            Regex::new(r"\d{4}[-/]\d{1,2}[-/]\d{1,2}").expect("invalid date regex"),
            Regex::new(r"\d+元|\d+块|\$\d+").expect("invalid price regex"),
            Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b").expect("invalid email regex"),
        ];

        Self {
            tag_dictionary,
            entity_patterns,
        }
    }

    /// Extract tags from text using dictionary matching and entity patterns
    pub fn extract(&self, text: &str) -> Vec<String> {
        let mut tags = Vec::new();
        let lower = text.to_lowercase();

        // 1. Dictionary matching with synonym expansion
        for (canonical, synonyms) in &self.tag_dictionary {
            let all_forms: Vec<&str> = std::iter::once(canonical.as_str())
                .chain(synonyms.iter().map(|s| s.as_str()))
                .collect();
            if all_forms.iter().any(|form| lower.contains(form)) {
                tags.push(canonical.clone());
            }
        }

        // 2. Entity pattern extraction
        for pattern in &self.entity_patterns {
            for cap in pattern.find_iter(text) {
                let tag = cap.as_str().to_lowercase();
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
        }

        // 3. Simple keyword extraction (words > 3 chars, not common stop words)
        let stop_words: Vec<&str> = vec![
            "的", "是", "在", "了", "和", "也", "有", "就", "不", "人", "都", "这", "那", "the",
            "is", "are", "was", "were",
        ];
        for word in text.split_whitespace() {
            let clean: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
            if clean.len() > 3 && !stop_words.contains(&clean.to_lowercase().as_str()) {
                let tag = clean.to_lowercase();
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
        }

        tags.dedup();
        tags.truncate(10);
        tags
    }
}

impl Default for TagExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dictionary_matching() {
        let extractor = TagExtractor::new();
        let tags = extractor.extract("这个产品多少钱");
        assert!(tags.contains(&"产品".to_string()));
        assert!(tags.contains(&"价格".to_string()));
    }

    #[test]
    fn test_entity_extraction() {
        let extractor = TagExtractor::new();
        let tags = extractor.extract("订单日期 2024-01-15 金额 100元");
        assert!(tags.iter().any(|t| t.contains("2024")));
        assert!(tags.iter().any(|t| t.contains("100")));
    }

    #[test]
    fn test_max_tags() {
        let extractor = TagExtractor::new();
        let text = "这是一个非常长的文本包含很多不同的关键词和实体信息用于测试标签提取器的上限限制功能是否正常工作";
        let tags = extractor.extract(text);
        assert!(tags.len() <= 10);
    }
}
