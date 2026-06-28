use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use agent_teams_core::error::{AgentTeamsError, Result};
use agent_teams_core::tool::{
    Tool, ToolCall, ToolExecutionContext, ToolExecutor, ToolParameters, ToolResult,
    UnifiedToolRegistry,
};

/// MCP transport type
#[derive(Debug, Clone)]
pub enum McpTransport {
    /// Server-Sent Events transport
    Sse,
    /// Standard I/O transport
    Stdio,
}

/// MCP tool definition from server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// MCP server info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInfo {
    pub name: String,
    pub version: String,
}

/// MCP tool call result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolResult {
    pub content: serde_json::Value,
    #[serde(rename = "isError")]
    pub is_error: bool,
}

/// MCP Client for communicating with MCP servers
pub struct McpClient {
    endpoint: String,
    client: reqwest::Client,
    server_info: Option<McpServerInfo>,
    /// Auto-incrementing request ID for JSON-RPC calls
    request_id: std::sync::atomic::AtomicU64,
}

impl McpClient {
    pub fn new(endpoint: &str, _transport: McpTransport) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("Failed to build MCP HTTP client"),
            server_info: None,
            request_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Get the next unique request ID
    fn next_id(&self) -> u64 {
        self.request_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Initialize the MCP connection
    pub async fn initialize(&mut self) -> Result<()> {
        let id = self.next_id();
        let response = self
            .client
            .post(format!("{}/initialize", self.endpoint))
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "agent-teams",
                        "version": "1.0.0"
                    }
                }
            }))
            .send()
            .await
            .map_err(|e| AgentTeamsError::Provider(format!("MCP init failed: {}", e)))?;

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AgentTeamsError::Provider(format!("MCP init parse failed: {}", e)))?;

        if let Some(result) = body.get("result") {
            if let Some(info) = result.get("serverInfo") {
                self.server_info = Some(McpServerInfo {
                    name: info["name"].as_str().unwrap_or("unknown").to_string(),
                    version: info["version"].as_str().unwrap_or("0.0.0").to_string(),
                });
            }
        }

        Ok(())
    }

    /// List available tools from the MCP server
    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>> {
        let id = self.next_id();
        let response = self
            .client
            .post(format!("{}/tools/list", self.endpoint))
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/list",
                "params": {}
            }))
            .send()
            .await
            .map_err(|e| AgentTeamsError::Provider(format!("MCP list_tools failed: {}", e)))?;

        let body: serde_json::Value = response.json().await.map_err(|e| {
            AgentTeamsError::Provider(format!("MCP list_tools parse failed: {}", e))
        })?;

        let tools = body
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();

        let mut result = Vec::new();
        for tool_value in tools {
            if let Ok(tool) = serde_json::from_value::<McpToolDef>(tool_value) {
                result.push(tool);
            }
        }

        Ok(result)
    }

    /// Call a tool on the MCP server
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolResult> {
        let id = self.next_id();
        let response = self
            .client
            .post(format!("{}/tools/call", self.endpoint))
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {
                    "name": name,
                    "arguments": arguments
                }
            }))
            .send()
            .await
            .map_err(|e| AgentTeamsError::Provider(format!("MCP call_tool failed: {}", e)))?;

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AgentTeamsError::Provider(format!("MCP call_tool parse failed: {}", e)))?;

        if let Some(result) = body.get("result") {
            Ok(McpToolResult {
                content: result
                    .get("content")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                is_error: result
                    .get("isError")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            })
        } else {
            Err(AgentTeamsError::Provider(
                "MCP call_tool: no result in response".to_string(),
            ))
        }
    }

    pub fn server_info(&self) -> Option<&McpServerInfo> {
        self.server_info.as_ref()
    }
}

/// MCP Tool Adapter: bridges MCP server tools into the UnifiedToolRegistry
pub struct McpToolAdapter {
    client: McpClient,
    discovered_tools: Vec<McpToolDef>,
}

impl McpToolAdapter {
    /// Connect to an MCP server and discover tools
    pub async fn connect(endpoint: &str, transport: McpTransport) -> Result<Self> {
        let mut client = McpClient::new(endpoint, transport);
        client.initialize().await?;
        let tools = client.list_tools().await?;

        tracing::info!(
            "Connected to MCP server '{}', discovered {} tools",
            client
                .server_info()
                .map(|s| s.name.as_str())
                .unwrap_or("unknown"),
            tools.len()
        );

        Ok(Self {
            client,
            discovered_tools: tools,
        })
    }

    /// Register discovered tools into the UnifiedToolRegistry
    pub async fn register_tools(&self, registry: &UnifiedToolRegistry) -> Result<()> {
        let executor_id = self
            .client
            .server_info()
            .map(|s| format!("mcp:{}", s.name))
            .unwrap_or_else(|| "mcp:unknown".to_string());

        for mcp_tool in &self.discovered_tools {
            let tool = Tool {
                name: mcp_tool.name.clone(),
                description: mcp_tool.description.clone(),
                parameters: ToolParameters {
                    schema: mcp_tool.input_schema.clone(),
                    required: mcp_tool
                        .input_schema
                        .get("required")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default(),
                },
                executor_id: executor_id.clone(),
                permission_tags: vec!["mcp".to_string()],
                allow_parallel: true,
                default_timeout_ms: 30_000,
                data_flow_hints: vec![],
                prerequisites: vec![],
                output_fields: vec![],
            };
            registry.register_tool(tool, Some("mcp"));
        }

        Ok(())
    }

    /// Get discovered tools
    pub fn discovered_tools(&self) -> &[McpToolDef] {
        &self.discovered_tools
    }
}

/// MCP Tool Executor: executes tools via MCP protocol
pub struct McpToolExecutor {
    client: McpClient,
}

impl McpToolExecutor {
    pub fn new(client: McpClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ToolExecutor for McpToolExecutor {
    fn executor_id(&self) -> &str {
        "mcp"
    }

    fn list_tools(&self) -> Vec<Tool> {
        Vec::new() // Tools are registered via McpToolAdapter
    }

    async fn execute(&self, call: &ToolCall, _ctx: &ToolExecutionContext) -> Result<ToolResult> {
        let start = std::time::Instant::now();
        let result = self
            .client
            .call_tool(&call.name, call.arguments.clone())
            .await?;

        Ok(ToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            success: !result.is_error,
            output: result.content,
            error: if result.is_error {
                Some("MCP tool returned error".to_string())
            } else {
                None
            },
            execution_duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}
