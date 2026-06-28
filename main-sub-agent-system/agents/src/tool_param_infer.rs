use std::collections::HashMap;
use std::sync::Arc;

use agent_teams_core::provider::{ChatMessage, CompletionRequest, LlmProvider};
use agent_teams_core::tool::{Tool, ToolCall, ToolResult};
use serde_json::Value;

/// Parameter inference context extracted from conversation history
#[derive(Debug, Clone, Default)]
pub struct ConversationContext {
    /// Extracted entities from conversation (e.g., city names, file paths, URLs)
    pub entities: HashMap<String, Vec<String>>,
    /// Recent tool call results for reference
    pub recent_results: Vec<(String, Value)>,
    /// User preferences mentioned in conversation
    pub preferences: HashMap<String, String>,
    /// Current topic/task context
    pub topic: Option<String>,
    /// Conversation history for pattern matching
    pub conversation_history: Vec<String>,
}

/// Entity extractor using regex patterns
struct EntityExtractor {
    url_pattern: regex::Regex,
    path_pattern: regex::Regex,
    time_pattern: regex::Regex,
    number_pattern: regex::Regex,
}

impl EntityExtractor {
    fn new() -> Self {
        Self {
            url_pattern: regex::Regex::new(r"https?://[^\s]+").unwrap(),
            // Matches: Unix paths (/home/...), Windows drive paths (C:\... or C:/...), and filenames with extensions
            path_pattern: regex::Regex::new(r#"(?:[A-Za-z]:\\[\w\\.-]+)|(?:[A-Za-z]:/[\w/.-]+)|(?:/[\w.-]+)+|[\w.-]+\.[\w]+"#).unwrap(),
            time_pattern: regex::Regex::new(r"\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}:\d{2})?").unwrap(),
            number_pattern: regex::Regex::new(r"\d+").unwrap(),
        }
    }

    fn extract_urls(&self, text: &str) -> Vec<String> {
        self.url_pattern.find_iter(text).map(|m| m.as_str().to_string()).collect()
    }

    fn extract_paths(&self, text: &str) -> Vec<String> {
        self.path_pattern
            .find_iter(text)
            .map(|m| m.as_str().to_string())
            .filter(|p| p.contains('/') || p.contains('\\') || p.contains('.'))
            .collect()
    }

    fn extract_times(&self, text: &str) -> Vec<String> {
        self.time_pattern.find_iter(text).map(|m| m.as_str().to_string()).collect()
    }

    fn extract_numbers(&self, text: &str) -> Vec<f64> {
        self.number_pattern
            .find_iter(text)
            .filter_map(|m| m.as_str().parse::<f64>().ok())
            .collect()
    }
}

/// Parameter inferrer - automatically fills missing tool parameters from conversation context
pub struct ParameterInferrer {
    provider: Arc<dyn LlmProvider>,
    inference_rules: Vec<InferenceRule>,
    entity_extractor: EntityExtractor,
}

#[derive(Debug, Clone)]
struct InferenceRule {
    param_pattern: String,
    tool_pattern: Option<String>,
    entity_type: String,
    default: Option<Value>,
    priority: u32,
}

impl ParameterInferrer {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            inference_rules: Self::default_rules(),
            entity_extractor: EntityExtractor::new(),
        }
    }

    fn default_rules() -> Vec<InferenceRule> {
        vec![
            InferenceRule {
                param_pattern: "url".to_string(),
                tool_pattern: Some("http".to_string()),
                entity_type: "url".to_string(),
                default: None,
                priority: 10,
            },
            InferenceRule {
                param_pattern: "path".to_string(),
                tool_pattern: Some("file".to_string()),
                entity_type: "file_path".to_string(),
                default: None,
                priority: 10,
            },
            InferenceRule {
                param_pattern: "city".to_string(),
                tool_pattern: Some("weather".to_string()),
                entity_type: "location".to_string(),
                default: None,
                priority: 8,
            },
            // Action defaults — LLM often omits 'action', these provide safe fallbacks
            InferenceRule {
                param_pattern: "action".to_string(),
                tool_pattern: Some("datetime".to_string()),
                entity_type: "action".to_string(),
                default: Some(Value::String("now".to_string())),
                priority: 9,
            },
            InferenceRule {
                param_pattern: "action".to_string(),
                tool_pattern: Some("file".to_string()),
                entity_type: "action".to_string(),
                default: Some(Value::String("read".to_string())),
                priority: 9,
            },
            InferenceRule {
                param_pattern: "timezone".to_string(),
                tool_pattern: Some("datetime".to_string()),
                entity_type: "timezone".to_string(),
                default: Some(Value::String("Asia/Shanghai".to_string())),
                priority: 5,
            },
            InferenceRule {
                param_pattern: "encoding".to_string(),
                tool_pattern: None,
                entity_type: "encoding".to_string(),
                default: Some(Value::String("utf-8".to_string())),
                priority: 3,
            },
        ]
    }

    /// Extract conversation context from message history
    pub fn extract_context(&self, messages: &[ChatMessage]) -> ConversationContext {
        let mut context = ConversationContext::default();

        for msg in messages {
            let content = &msg.content;

            // Extract entities using regex
            let urls = self.entity_extractor.extract_urls(content);
            if !urls.is_empty() {
                context.entities.entry("url".to_string()).or_default().extend(urls);
            }
            let paths = self.entity_extractor.extract_paths(content);
            if !paths.is_empty() {
                context.entities.entry("file_path".to_string()).or_default().extend(paths);
            }
            let times = self.entity_extractor.extract_times(content);
            if !times.is_empty() {
                context.entities.entry("time".to_string()).or_default().extend(times);
            }
            let numbers = self.entity_extractor.extract_numbers(content);
            if !numbers.is_empty() {
                let strs: Vec<String> = numbers.iter().map(|n| n.to_string()).collect();
                context.entities.entry("number".to_string()).or_default().extend(strs);
            }

            // Extract mentioned tool results
            if msg.role == "tool" {
                if let Ok(result) = serde_json::from_str::<Value>(&msg.content) {
                    if let Some(tool_name) = result.get("tool").and_then(|v| v.as_str()) {
                        context.recent_results.push((tool_name.to_string(), result));
                    }
                }
            }

            // Topic detection
            let lower = content.to_lowercase();
            if lower.contains("天气") || lower.contains("weather") {
                context.topic = Some("weather".to_string());
            } else if lower.contains("文件") || lower.contains("file") {
                context.topic = Some("file".to_string());
            } else if lower.contains("时间") || lower.contains("time") || lower.contains("date")
                || lower.contains("几点") || lower.contains("日期") || lower.contains("星期")
                || lower.contains("timestamp") || lower.contains("时间戳") {
                context.topic = Some("datetime".to_string());
            } else if lower.contains("http") || lower.contains("api") || lower.contains("url") {
                context.topic = Some("http".to_string());
            }

            // Detect specific action intent for common tools
            if lower.contains("写") || lower.contains("保存") || lower.contains("创建") || lower.contains("新建") {
                context.preferences.insert("file_action".to_string(), "write".to_string());
            } else if lower.contains("列") || lower.contains("目录") || lower.contains("查看目录") {
                context.preferences.insert("file_action".to_string(), "list".to_string());
            } else if lower.contains("删") || lower.contains("删除") {
                context.preferences.insert("file_action".to_string(), "delete".to_string());
            } else if lower.contains("搜索文件") || lower.contains("grep") || lower.contains("查找文件") {
                context.preferences.insert("file_action".to_string(), "search".to_string());
            }

            if lower.contains("几点") || lower.contains("现在时间") || lower.contains("当前时间") {
                context.preferences.insert("datetime_action".to_string(), "now".to_string());
            } else if lower.contains("时间戳") || lower.contains("timestamp") || lower.contains("unix") {
                context.preferences.insert("datetime_action".to_string(), if lower.contains("转") || lower.contains("from") { "from_unix".to_string() } else { "to_unix".to_string() });
            } else if lower.contains("时间差") || lower.contains("相差") || lower.contains("间隔") {
                context.preferences.insert("datetime_action".to_string(), "diff".to_string());
            } else if lower.contains("格式") || lower.contains("format") {
                context.preferences.insert("datetime_action".to_string(), "format".to_string());
            }

            context.conversation_history.push(content.clone());
        }

        context
    }

    /// Infer missing parameters for a tool call based on conversation context
    pub async fn infer_parameters(
        &self,
        tool: &Tool,
        partial_args: &Value,
        context: &ConversationContext,
        messages: &[ChatMessage],
    ) -> Value {
        self.infer_parameters_with_history(tool, partial_args, context, messages, &[]).await
    }

    /// Infer missing parameters with access to tool execution history (from ReAct loop).
    /// This enables cross-tool data flow: output of tool A can be used as input for tool B.
    pub async fn infer_parameters_with_history(
        &self,
        tool: &Tool,
        partial_args: &Value,
        context: &ConversationContext,
        messages: &[ChatMessage],
        tool_history: &[(ToolCall, ToolResult)],
    ) -> Value {
        let mut args = partial_args.clone();
        let mut missing_params = Vec::new();

        for required in &tool.parameters.required {
            if args.get(required).is_none_or(|v| v.is_null()) {
                missing_params.push(required.clone());
            }
        }

        if missing_params.is_empty() {
            return args;
        }

        // 1. Rule-based inference (sorted by priority — higher priority rules applied first)
        let mut sorted_rules: Vec<&InferenceRule> = self.inference_rules.iter().collect();
        sorted_rules.sort_by(|a, b| b.priority.cmp(&a.priority));
        for param_name in &missing_params.clone() {
            if let Some(inferred) = self.try_rule_inference_sorted(param_name, tool, context, &sorted_rules) {
                args[param_name] = inferred;
                missing_params.retain(|p| p != param_name);
            }
        }

        // 2. Context-based inference (topic + recent tool results)
        for param_name in &missing_params.clone() {
            if let Some(inferred) = self.try_context_inference(param_name, tool, context) {
                args[param_name] = inferred;
                missing_params.retain(|p| p != param_name);
            }
        }

        // 3. Tool history inference (cross-tool data flow from ReAct loop)
        for param_name in &missing_params.clone() {
            if let Some(inferred) = self.try_tool_history_inference(param_name, tool, tool_history) {
                args[param_name] = inferred;
                missing_params.retain(|p| p != param_name);
            }
        }

        // 4. History-based inference
        for param_name in &missing_params.clone() {
            if let Some(inferred) = self.try_history_inference(param_name, context) {
                args[param_name] = inferred;
                missing_params.retain(|p| p != param_name);
            }
        }

        // 4. LLM inference for remaining params
        if !missing_params.is_empty() {
            if let Some(inferred) = self.try_llm_inference(tool, &args, &missing_params, messages).await {
                for (key, value) in inferred {
                    args[key] = value;
                }
            }
        }

        // 5. Apply defaults for remaining optional params
        if let Some(properties) = tool.parameters.schema.get("properties") {
            if let Some(props) = properties.as_object() {
                for (param_name, param_schema) in props {
                    if args.get(param_name).is_none() {
                        if let Some(default) = param_schema.get("default") {
                            args[param_name] = default.clone();
                        }
                    }
                }
            }
        }

        args
    }

    /// Context-based inference: match param name against topic and recent results
    fn try_context_inference(
        &self,
        param_name: &str,
        tool: &Tool,
        context: &ConversationContext,
    ) -> Option<Value> {
        if let Some(ref topic) = context.topic {
            match param_name {
                "url" if topic == "http" || topic == "web" => {
                    if let Some(urls) = context.entities.get("url") {
                        return urls.first().map(|u| Value::String(u.clone()));
                    }
                }
                "path" if topic == "file" => {
                    if let Some(paths) = context.entities.get("file_path") {
                        return paths.first().map(|p| Value::String(p.clone()));
                    }
                }
                "action" => {
                    // Infer action from detected preferences
                    if tool.name == "datetime" {
                        if let Some(action) = context.preferences.get("datetime_action") {
                            return Some(Value::String(action.clone()));
                        }
                    }
                    if tool.name == "file" {
                        if let Some(action) = context.preferences.get("file_action") {
                            return Some(Value::String(action.clone()));
                        }
                    }
                }
                _ => {}
            }
        }

        // Check recent tool call results
        for (_tool_name, result) in &context.recent_results {
            if let Some(output) = result.get("output") {
                if let Some(value) = output.get(param_name) {
                    return Some(value.clone());
                }
            }
        }

        None
    }

    /// Tool history inference: extract param values from previous tool outputs in the ReAct loop.
    /// This enables cross-tool data flow — e.g., file(write) output path → next tool's path param.
    fn try_tool_history_inference(
        &self,
        param_name: &str,
        tool: &Tool,
        tool_history: &[(ToolCall, ToolResult)],
    ) -> Option<Value> {
        if tool_history.is_empty() {
            return None;
        }

        let is_path_param = param_name == "path" || param_name == "file_path" || param_name == "output";

        // Look at recent tool results in reverse order (most recent first)
        for (call, result) in tool_history.iter().rev() {
            if !result.success {
                continue;
            }

            // For path params: if a previous file(write) succeeded, use its output path
            if is_path_param && call.name == "file" {
                if let Some(action) = call.arguments.get("action").and_then(|v| v.as_str()) {
                    if action == "write" {
                        if let Some(path) = result.output.get("path").and_then(|v| v.as_str()) {
                            return Some(Value::String(path.to_string()));
                        }
                        // Also check the call arguments for the path
                        if let Some(path) = call.arguments.get("path").and_then(|v| v.as_str()) {
                            return Some(Value::String(path.to_string()));
                        }
                    }
                }
            }

            // For content/data params: if a previous tool produced output, use it as content
            if (param_name == "content" || param_name == "body" || param_name == "data" || param_name == "answers")
                && call.name != tool.name {
                    // Use the previous tool's output as input data
                    let output_str = result.output.to_string();
                    if output_str.len() > 10 && output_str != "null" {
                        return Some(result.output.clone());
                    }
                }

            // For url params: if a previous http/search tool returned a URL
            if param_name == "url" {
                if let Some(url) = result.output.get("url").and_then(|v| v.as_str()) {
                    return Some(Value::String(url.to_string()));
                }
                // Check if output contains extracted URLs
                if let Some(links) = result.output.get("links").and_then(|v| v.as_array()) {
                    if let Some(first) = links.first().and_then(|v| v.as_str()) {
                        return Some(Value::String(first.to_string()));
                    }
                }
            }

            // Generic: check if previous output has a field matching the param name
            if let Some(value) = result.output.get(param_name) {
                if !value.is_null() {
                    return Some(value.clone());
                }
            }
        }

        None
    }

    /// History-based inference: extract param value from conversation history
    fn try_history_inference(
        &self,
        param_name: &str,
        context: &ConversationContext,
    ) -> Option<Value> {
        for msg in &context.conversation_history {
            match param_name {
                "url" => {
                    let urls = self.entity_extractor.extract_urls(msg);
                    if let Some(first) = urls.first() {
                        return Some(Value::String(first.clone()));
                    }
                }
                "path" => {
                    let paths = self.entity_extractor.extract_paths(msg);
                    if let Some(first) = paths.first() {
                        return Some(Value::String(first.clone()));
                    }
                }
                "timeout" => {
                    let numbers = self.entity_extractor.extract_numbers(msg);
                    if let Some(first) = numbers.first() {
                        return Some(Value::Number((*first as i64).into()));
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Try to infer parameter using rules (accepts pre-sorted rules by priority)
    fn try_rule_inference_sorted(
        &self,
        param_name: &str,
        tool: &Tool,
        context: &ConversationContext,
        sorted_rules: &[&InferenceRule],
    ) -> Option<Value> {
        for rule in sorted_rules {
            if !param_name.contains(&rule.param_pattern) {
                continue;
            }

            if let Some(ref tool_pattern) = rule.tool_pattern {
                if !tool.name.contains(tool_pattern) {
                    continue;
                }
            }

            if let Some(entities) = context.entities.get(&rule.entity_type) {
                if let Some(first) = entities.first() {
                    return Some(Value::String(first.clone()));
                }
            }

            if let Some(ref default) = rule.default {
                return Some(default.clone());
            }
        }

        None
    }

    /// Use LLM to infer missing parameters
    async fn try_llm_inference(
        &self,
        tool: &Tool,
        current_args: &Value,
        missing_params: &[String],
        messages: &[ChatMessage],
    ) -> Option<HashMap<String, Value>> {
        let recent_messages: Vec<String> = messages
            .iter()
            .rev()
            .take(5)
            .map(|m| format!("{}: {}", m.role, m.content.chars().take(200).collect::<String>()))
            .collect();

        let prompt = format!(
            r#"根据对话上下文，推断工具调用的缺失参数。

工具名称: {}
工具描述: {}
当前参数: {}
缺失参数: {}

最近对话:
{}

请返回JSON格式的推断结果，只包含推断出的参数。如果无法推断，返回空对象。
示例: {{"city": "北京", "unit": "celsius"}}"#,
            tool.name,
            tool.description,
            current_args,
            missing_params.join(", "),
            recent_messages.join("\n")
        );

        let request = CompletionRequest {
            model: String::new(),
            messages: vec![ChatMessage::simple("user", &prompt)],
            max_tokens: Some(4096),
            temperature: Some(0.1),
            system: Some("你是一个参数推断助手。根据对话上下文推断缺失的工具参数。只返回JSON，不要解释。".to_string()),
            stream: false,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
        };

        match self.provider.complete(request).await {
            Ok(resp) => {
                let content = resp.content.trim();
                // Try to parse JSON from response
                if let Ok(parsed) = serde_json::from_str::<Value>(content) {
                    if let Some(obj) = parsed.as_object() {
                        let mut result = HashMap::new();
                        for (k, v) in obj {
                            result.insert(k.clone(), v.clone());
                        }
                        return Some(result);
                    }
                }
                None
            }
            Err(_) => None,
        }
    }

    /// Enrich a tool call with inferred parameters
    pub async fn enrich_tool_call(
        &self,
        call: &ToolCall,
        tool: &Tool,
        messages: &[ChatMessage],
    ) -> ToolCall {
        self.enrich_tool_call_with_history(call, tool, messages, &[]).await
    }

    /// Enrich a tool call with inferred parameters, using tool history for cross-tool data flow
    pub async fn enrich_tool_call_with_history(
        &self,
        call: &ToolCall,
        tool: &Tool,
        messages: &[ChatMessage],
        tool_history: &[(ToolCall, ToolResult)],
    ) -> ToolCall {
        let context = self.extract_context(messages);
        let enriched_args = self.infer_parameters_with_history(tool, &call.arguments, &context, messages, tool_history).await;

        ToolCall {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: enriched_args,
        }
    }
}

/// Parameter suggestion for tool calls - helps LLM understand what parameters are needed
pub fn build_parameter_hints(tool: &Tool, context: &ConversationContext) -> String {
    let mut hints = Vec::new();

    if let Some(properties) = tool.parameters.schema.get("properties") {
        if let Some(props) = properties.as_object() {
            for (param_name, param_schema) in props {
                let is_required = tool.parameters.required.contains(param_name);
                let desc = param_schema.get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("");

                let mut hint = format!("- {}: {}", param_name, desc);

                // Add context-based suggestions
                if param_name == "city" || param_name == "location" {
                    if let Some(locations) = context.entities.get("location") {
                        if !locations.is_empty() {
                            hint.push_str(&format!(" (可能是: {})", locations.join(", ")));
                        }
                    }
                }

                if param_name == "url" {
                    if let Some(urls) = context.entities.get("url") {
                        if !urls.is_empty() {
                            hint.push_str(&format!(" (提到的URL: {})", urls.join(", ")));
                        }
                    }
                }

                if param_name == "path" || param_name == "file_path" {
                    if let Some(paths) = context.entities.get("file_path") {
                        if !paths.is_empty() {
                            hint.push_str(&format!(" (提到的文件: {})", paths.join(", ")));
                        }
                    }
                }

                if is_required {
                    hint.push_str(" [必填]");
                }

                hints.push(hint);
            }
        }
    }

    // Append data flow hints from the tool definition
    if !tool.data_flow_hints.is_empty() {
        hints.push(format!("数据流提示: {}", tool.data_flow_hints.join("；")));
    }

    if hints.is_empty() {
        String::new()
    } else {
        format!("参数提示:\n{}", hints.join("\n"))
    }
}

/// Detect if a tool call needs preparatory steps based on data flow analysis.
/// Returns a list of suggested preparatory tool calls if any are needed.
pub fn detect_preparatory_steps(
    tool: &Tool,
    args: &serde_json::Value,
    context: &ConversationContext,
) -> Vec<PreparatoryStep> {
    let mut steps = Vec::new();

    let required_params: std::collections::HashSet<&str> =
        tool.parameters.required.iter().map(|s| s.as_str()).collect();

    if let Some(properties) = tool.parameters.schema.get("properties") {
        if let Some(props) = properties.as_object() {
            // Only check required params — optional params being missing is normal
            for (param_name, param_schema) in props {
                if !required_params.contains(param_name.as_str()) {
                    continue;
                }

                let desc = param_schema.get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_lowercase();

                let is_path_param = desc.contains("路径") || desc.contains("path")
                    || desc.contains("文件") || desc.contains("file");

                if !is_path_param {
                    continue;
                }

                // If the tool expects a file path but the argument is missing or looks like inline data
                let arg_value = args.get(param_name);
                let needs_file_write = match arg_value {
                    None => {
                        // Missing required path param — check if context has data that should be written
                        !context.recent_results.is_empty() || !context.conversation_history.is_empty()
                    }
                    Some(v) => {
                        // If the value looks like inline data (long string, JSON object), not a path
                        if let Some(s) = v.as_str() {
                            let looks_like_path = s.contains('/') || s.contains('\\')
                                || (s.contains('.') && s.len() < 500);
                            !looks_like_path && s.len() > 100
                        } else {
                            v.is_object() || v.is_array()
                        }
                    }
                };

                if needs_file_write {
                    // Determine what data to write
                    let data_source = if let Some(last_result) = context.recent_results.last() {
                        Some(format!("最近一次 {} 工具的输出", last_result.0))
                    } else if !context.conversation_history.is_empty() {
                        Some("对话上下文中的数据".to_string())
                    } else {
                        None
                    };

                    if let Some(source) = data_source {
                        steps.push(PreparatoryStep {
                            tool_name: "file".to_string(),
                            reason: format!(
                                "工具 {} 的参数 '{}' 需要文件路径，但数据来自{}。建议先用 file(write) 将数据写入临时文件。",
                                tool.name, param_name, source
                            ),
                            suggested_args: serde_json::json!({
                                "action": "write",
                                "path": format!("/tmp/tool_input_{}.json", tool.name),
                                "content": "<从上下文中提取的数据>"
                            }),
                            target_param: param_name.clone(),
                        });
                    }
                }
            }
        }
    }

    steps
}

/// A suggested preparatory tool call before the target tool
#[derive(Debug, Clone)]
pub struct PreparatoryStep {
    pub tool_name: String,
    pub reason: String,
    pub suggested_args: serde_json::Value,
    pub target_param: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_teams_core::provider::ProviderError;

    #[test]
    fn test_extract_context_urls() {
        let inferrer = ParameterInferrer::new(Arc::new(MockProvider));
        let messages = vec![
            ChatMessage::simple("user", "请访问 https://api.example.com 获取数据"),
        ];

        let context = inferrer.extract_context(&messages);
        assert!(context.entities.contains_key("url"));
        assert_eq!(context.entities["url"].len(), 1);
        assert!(context.entities["url"][0].contains("api.example.com"));
    }

    #[test]
    fn test_extract_context_topic() {
        let inferrer = ParameterInferrer::new(Arc::new(MockProvider));
        let messages = vec![
            ChatMessage::simple("user", "北京今天天气怎么样？"),
        ];

        let context = inferrer.extract_context(&messages);
        assert_eq!(context.topic, Some("weather".to_string()));
    }

    #[test]
    fn test_extract_context_file_paths() {
        let inferrer = ParameterInferrer::new(Arc::new(MockProvider));
        let messages = vec![
            ChatMessage::simple("user", "请读取 src/main.rs 文件"),
        ];

        let context = inferrer.extract_context(&messages);
        assert!(context.entities.contains_key("file_path"));
        assert!(context.entities["file_path"][0].contains("main.rs"));
    }

    // Mock provider for tests
    struct MockProvider;

    #[async_trait::async_trait]
    impl agent_teams_core::provider::LlmProvider for MockProvider {
        fn id(&self) -> &str {
            "mock"
        }

        fn name(&self) -> &str {
            "Mock Provider"
        }

        fn models(&self) -> Vec<String> {
            vec!["mock-model".to_string()]
        }

        async fn complete(&self, _req: CompletionRequest) -> std::result::Result<agent_teams_core::provider::CompletionResponse, ProviderError> {
            Ok(agent_teams_core::provider::CompletionResponse {
                content: "{}".to_string(),
                tool_calls: vec![],
                thinking: None,
                model: "mock-model".to_string(),
                usage: Default::default(),
                stop_reason: Some("stop".to_string()),
            })
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<
            Box<dyn futures::Stream<Item = std::result::Result<agent_teams_core::provider::CompletionChunk, ProviderError>> + Unpin + Send>,
            ProviderError,
        > {
            Err(ProviderError::Other("Mock provider does not support streaming".to_string()))
        }
    }
}
