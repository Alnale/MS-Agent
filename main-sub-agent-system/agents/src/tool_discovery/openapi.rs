use serde::{Deserialize, Serialize};

use agent_core::error::{AgentTeamsError, Result};
use agent_core::tool::{Tool, ToolParameters, UnifiedToolRegistry};

/// OpenAPI specification (simplified)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiSpec {
    pub openapi: String,
    pub info: OpenApiInfo,
    pub paths: std::collections::HashMap<String, PathItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiInfo {
    pub title: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathItem {
    #[serde(flatten)]
    pub methods: std::collections::HashMap<String, Operation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    #[serde(rename = "operationId")]
    pub operation_id: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub parameters: Option<Vec<Parameter>>,
    #[serde(rename = "requestBody")]
    pub request_body: Option<RequestBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "in")]
    pub location: String,
    pub required: Option<bool>,
    pub schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestBody {
    pub content: std::collections::HashMap<String, MediaType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaType {
    pub schema: Option<serde_json::Value>,
}

/// OpenAPI authentication
#[derive(Debug, Clone)]
pub enum OpenApiAuth {
    Bearer { token: String },
    ApiKey { header: String, value: String },
}

/// OpenAPI Importer: imports tools from OpenAPI specifications
pub struct OpenApiImporter;

impl OpenApiImporter {
    /// Import tools from an OpenAPI specification URL
    pub async fn import_from_url(url: &str, registry: &UnifiedToolRegistry) -> Result<Vec<Tool>> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| AgentTeamsError::Provider(format!("Failed to build HTTP client: {}", e)))?;

        let spec: OpenApiSpec = client
            .get(url)
            .send()
            .await
            .map_err(|e| AgentTeamsError::Provider(format!("Failed to fetch OpenAPI spec: {}", e)))?
            .json()
            .await
            .map_err(|e| {
                AgentTeamsError::Provider(format!("Failed to parse OpenAPI spec: {}", e))
            })?;

        Self::import_from_spec(&spec, registry)
    }

    /// Import tools from an OpenAPI specification
    pub fn import_from_spec(
        spec: &OpenApiSpec,
        registry: &UnifiedToolRegistry,
    ) -> Result<Vec<Tool>> {
        let mut tools = Vec::new();

        for (path, path_item) in &spec.paths {
            for (method, operation) in &path_item.methods {
                // Skip non-HTTP methods
                if !["get", "post", "put", "delete", "patch"].contains(&method.as_str()) {
                    continue;
                }

                let tool_name = operation
                    .operation_id
                    .clone()
                    .unwrap_or_else(|| format!("{}_{}", method, path));

                let description = operation
                    .description
                    .clone()
                    .or_else(|| operation.summary.clone())
                    .unwrap_or_default();

                // Build parameter schema
                let mut properties = serde_json::Map::new();
                let mut required = Vec::new();

                if let Some(params) = &operation.parameters {
                    for param in params {
                        if let Some(schema) = &param.schema {
                            properties.insert(param.name.clone(), schema.clone());
                        }
                        if param.required.unwrap_or(false) {
                            required.push(param.name.clone());
                        }
                    }
                }

                // Add request body schema if present
                if let Some(body) = &operation.request_body {
                    for media_type in body.content.values() {
                        if let Some(schema) = &media_type.schema {
                            if let Some(obj) = schema.as_object() {
                                if let Some(props) = obj.get("properties") {
                                    if let Some(props_obj) = props.as_object() {
                                        for (key, value) in props_obj {
                                            properties.insert(key.clone(), value.clone());
                                        }
                                    }
                                }
                                if let Some(req) = obj.get("required") {
                                    if let Some(req_arr) = req.as_array() {
                                        for r in req_arr {
                                            if let Some(s) = r.as_str() {
                                                required.push(s.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let schema = serde_json::json!({
                    "type": "object",
                    "properties": properties,
                    "required": required,
                });

                let tool = Tool {
                    name: tool_name.clone(),
                    description: format!("[{} {}] {}", method.to_uppercase(), path, description),
                    parameters: ToolParameters { schema, required },
                    tool_type: "function".to_string(),
                    executor_id: "openapi".to_string(),
                    permission_tags: vec!["openapi".to_string(), method.clone()],
                    allow_parallel: true,
                    default_timeout_ms: 30_000,
                    data_flow_hints: vec![],
                    prerequisites: vec![],
                    output_fields: vec![],
                };

                registry.register_tool(tool.clone(), Some("openapi"));
                tools.push(tool);
            }
        }

        tracing::info!(
            "Imported {} tools from OpenAPI spec '{}'",
            tools.len(),
            spec.info.title
        );

        Ok(tools)
    }
}
