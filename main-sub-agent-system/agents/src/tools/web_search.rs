//! Web Search 工具执行器
//!
//! 基于 Mimo 平台的 Web Search 功能，为 LLM 提供实时网络搜索能力。
//! 这是一个平台级工具，搜索由 LLM 服务端执行，不在本地运行。

use agent_core::tool::{Tool, ToolBuilder, ToolCall, ToolExecutionContext, ToolExecutor, ToolResult, tool_error};
use agent_core::error::Result;
use async_trait::async_trait;

/// Web Search 工具 — 基于 Mimo 平台的在线搜索能力
pub struct WebSearchTool;

impl WebSearchTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolExecutor for WebSearchTool {
    fn executor_id(&self) -> &str {
        "web_search"
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            ToolBuilder::new("web_search")
                .description(concat!(
                    "联网搜索工具，帮助模型获取实时网络信息（如新闻、产品、天气等）。\n",
                    "搜索由平台服务端执行，模型会自动判断是否需要搜索。\n\n",
                    "参数说明：\n",
                    "- force_search: 是否强制搜索（默认 false，模型自主判断）\n",
                    "- max_keyword: 每轮搜索最大关键词数（控制搜索次数和费用）\n",
                    "- limit: 返回结果数量\n",
                    "- user_location: 用户位置信息，帮助获取更精准的本地化结果\n\n",
                    "注意：此工具需要在 Mimo 平台启用 Web Search 插件。",
                ))
                .tool_type("web_search")
                .executor("web_search")
                .tag("network")
                .tag("search")
                .timeout(30_000)
                .param_bool("force_search", "是否强制执行搜索（默认 false，模型自主判断是否需要搜索）", false)
                .param_integer("max_keyword", "每轮搜索最大关键词数（默认由模型决定，可控制搜索次数和费用）", false)
                .param_integer("limit", "返回搜索结果数量", false)
                .param_raw("user_location", serde_json::json!({
                    "type": "object",
                    "description": "用户位置信息，帮助获取更精准的本地化搜索结果",
                    "properties": {
                        "type": {
                            "type": "string",
                            "description": "位置类型",
                            "default": "approximate"
                        },
                        "country": {
                            "type": "string",
                            "description": "国家（如 China、US）"
                        },
                        "region": {
                            "type": "string",
                            "description": "省份/州（如 Hubei、California）"
                        },
                        "city": {
                            "type": "string",
                            "description": "城市（如 Wuhan、San Francisco）"
                        }
                    }
                }), false)
                .build(),
        ]
    }

    async fn execute(
        &self,
        call: &ToolCall,
        _ctx: &ToolExecutionContext,
    ) -> Result<ToolResult> {
        // Web search is a platform-level tool — it should never be executed locally.
        // The LLM handles the search server-side and returns results as annotations.
        // If we somehow get here, it means the tool was incorrectly routed.
        let ms = 0;
        Ok(tool_error(
            call,
            "Web Search 是平台级工具，不支持本地执行",
            "搜索由 LLM 服务端自动执行，无需手动调用",
            "请直接向模型提问需要搜索的问题，模型会自动触发搜索",
            ms,
        ))
    }
}
