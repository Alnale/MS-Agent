use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent_memory_cache::CacheMode;
use crate::memory::MemoryConfig;
use crate::pipeline::PipelineDef;

/// Top-level application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub version: String,
    pub description: Option<String>,
    pub providers: ProvidersConfig,
    pub main_agent: MainAgentConfig,
    #[serde(default)]
    pub sub_agents: HashMap<String, SubAgentConfig>,
    pub pipeline: Option<PipelineDef>,
    pub context_providers: Option<Value>,
    pub runtime: RuntimeConfig,
    pub features: Option<FeaturesConfig>,
    pub concurrency: Option<ConcurrencyConfig>,
    pub timeouts: Option<TimeoutConfig>,
    pub security: Option<SecurityConfig>,
    pub cost_optimization: Option<CostOptimizationConfig>,
    pub degradation: Option<DegradationConfig>,
    pub session: Option<SessionConfig>,
    pub database: Option<DatabaseConfig>,
    #[serde(default)]
    pub memory: Option<MemoryConfig>,
    #[serde(default)]
    pub unified_cache: Option<UnifiedCacheConfig>,
    /// External tool sources (MCP servers, OpenAPI specs)
    #[serde(default)]
    pub external_tools: Option<Vec<crate::tool::ExternalToolSource>>,
}

fn default_true() -> bool { true }

impl AppConfig {
    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        let mut errors = Vec::new();

        if self.version.is_empty() {
            errors.push("Version cannot be empty".to_string());
        }

        if let Err(e) = self.runtime.validate() {
            errors.push(e);
        }

        if let Err(e) = self.main_agent.validate() {
            errors.push(e);
        }

        if let Some(ref timeouts) = self.timeouts {
            if let Err(e) = timeouts.validate() {
                errors.push(e);
            }
        }

        if let Some(ref concurrency) = self.concurrency {
            if let Err(e) = concurrency.validate() {
                errors.push(e);
            }
        }

        if let Some(ref security) = self.security {
            if let Err(e) = security.validate() {
                errors.push(e);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MainAgentConfig {
    pub id: String,
    pub thinking: ThinkingSettings,
    #[serde(default)]
    pub critic: CriticSettings,
    #[serde(default)]
    pub plan_cache: PlanCacheSettings,
    #[serde(default)]
    pub total_timeout_ms: u64,
    pub config: Option<AgentConfig>,
}

/// Providers configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProvidersConfig {
    pub anthropic: Option<ProviderConfig>,
    pub openai: Option<ProviderConfig>,
    pub openai_responses: Option<ProviderConfig>,
    pub ollama: Option<ProviderConfig>,
}

/// Individual provider configuration
#[derive(Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub base_url: String,
    #[serde(skip_serializing)]
    pub api_key: Option<String>,
    pub default_model: String,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_retry_delay")]
    pub retry_base_delay_ms: u64,
    #[serde(default = "default_cb_threshold")]
    pub circuit_breaker_threshold: u32,
    #[serde(default = "default_cb_duration")]
    pub circuit_breaker_open_duration_secs: u64,
}

fn default_max_retries() -> u32 {
    3
}
fn default_retry_delay() -> u64 {
    1000
}
fn default_cb_threshold() -> u32 {
    5
}
fn default_cb_duration() -> u64 {
    60
}

impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("default_model", &self.default_model)
            .field("max_retries", &self.max_retries)
            .field("retry_base_delay_ms", &self.retry_base_delay_ms)
            .field("circuit_breaker_threshold", &self.circuit_breaker_threshold)
            .field(
                "circuit_breaker_open_duration_secs",
                &self.circuit_breaker_open_duration_secs,
            )
            .finish()
    }
}

impl ProvidersConfig {
    /// Get the default model from the first available provider
    pub fn default_model(&self) -> String {
        self.anthropic
            .as_ref()
            .map(|p| p.default_model.clone())
            .unwrap_or_else(|| "mimo-v2.5".to_string())
    }
}

impl Default for MainAgentConfig {
    fn default() -> Self {
        Self {
            id: "main_agent".to_string(),
            thinking: ThinkingSettings::default(),
            critic: CriticSettings::default(),
            plan_cache: PlanCacheSettings::default(),
            total_timeout_ms: 90_000,
            config: None,
        }
    }
}

impl Default for ThinkingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            budget_tokens: 0,
            strategy: "Auto".to_string(),
        }
    }
}

impl Default for CriticSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_refinement_rounds: 1,
            thinking: None,
        }
    }
}

impl Default for PlanCacheSettings {
    fn default() -> Self {
        Self {
            ttl_secs: 300,
            capacity: 500,
        }
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            port: 3000,
            host: "0.0.0.0".to_string(),
            log_level: "info".to_string(),
            cors_enabled: Some(true),
            cors_allowed_origins: None,
            max_concurrent_requests: Some(100),
            rate_limit_max_requests: None,
            rate_limit_window_secs: None,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: "1.0.0".to_string(),
            description: Some("Default config".to_string()),
            providers: ProvidersConfig::default(),
            main_agent: MainAgentConfig::default(),
            sub_agents: HashMap::new(),
            pipeline: None,
            context_providers: None,
            runtime: RuntimeConfig::default(),
            features: None,
            concurrency: None,
            timeouts: None,
            security: None,
            cost_optimization: None,
            degradation: None,
            session: None,
            database: None,
            memory: None,
            unified_cache: None,
            external_tools: None,
        }
    }
}

impl MainAgentConfig {
    /// Validate main agent configuration
    pub fn validate(&self) -> Result<(), String> {
        // Validate ID
        if self.id.is_empty() {
            return Err("Main agent ID cannot be empty".to_string());
        }

        // Validate total timeout
        if self.total_timeout_ms == 0 {
            return Err("Total timeout cannot be 0".to_string());
        }

        // Validate thinking settings
        self.thinking.validate()?;

        // Validate critic settings
        self.critic.validate()?;

        // Validate plan cache settings
        self.plan_cache.validate()?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingSettings {
    pub enabled: bool,
    pub budget_tokens: u32,
    #[serde(default = "default_thinking_strategy")]
    pub strategy: String,
}

fn default_thinking_strategy() -> String {
    "Auto".to_string()
}

impl ThinkingSettings {
    /// Convert to runtime ThinkingConfig
    pub fn to_thinking_config(&self) -> crate::provider::ThinkingConfig {
        crate::provider::ThinkingConfig {
            enabled: self.enabled,
            budget_tokens: self.budget_tokens,
            strategy: self.strategy.clone(),
        }
    }

    /// Validate thinking settings
    pub fn validate(&self) -> Result<(), String> {
        if self.enabled && self.budget_tokens == 0 {
            return Err("Budget tokens cannot be 0 when thinking is enabled".to_string());
        }

        let normalized = self.strategy.to_lowercase();
        let valid_strategies = ["auto", "always", "never"];
        if !valid_strategies.contains(&normalized.as_str()) {
            return Err(format!(
                "Invalid thinking strategy '{}'. Must be one of: Auto, Always, Never",
                self.strategy
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriticSettings {
    pub enabled: bool,
    pub max_refinement_rounds: u8,
    /// Thinking configuration for the critic agent. If None, thinking is disabled.
    #[serde(default)]
    pub thinking: Option<ThinkingSettings>,
}

impl CriticSettings {
    /// Validate critic settings
    pub fn validate(&self) -> Result<(), String> {
        if self.enabled && self.max_refinement_rounds == 0 {
            return Err("Max refinement rounds cannot be 0 when critic is enabled".to_string());
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanCacheSettings {
    pub ttl_secs: u64,
    pub capacity: usize,
}

impl PlanCacheSettings {
    /// Validate plan cache settings
    pub fn validate(&self) -> Result<(), String> {
        if self.capacity == 0 {
            return Err("Plan cache capacity cannot be 0".to_string());
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

/// Sub-agent configuration entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentConfig {
    pub expertise: String,
    #[serde(default)]
    pub message_types: Vec<String>,
    #[serde(default = "default_true")]
    pub requires_llm: bool,
    #[serde(default)]
    pub priority: u32,
    #[serde(default)]
    pub optional: bool,
    pub config: Option<AgentConfig>,
    /// Thinking configuration for this sub-agent. If None, thinking is disabled.
    #[serde(default)]
    pub thinking: Option<ThinkingSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub port: u16,
    pub host: String,
    pub log_level: String,
    pub cors_enabled: Option<bool>,
    pub cors_allowed_origins: Option<Vec<String>>,
    pub max_concurrent_requests: Option<usize>,
    /// Max requests per IP per window. 0 or absent = no rate limiting.
    pub rate_limit_max_requests: Option<u32>,
    /// Rate limit window in seconds. Default: 60.
    pub rate_limit_window_secs: Option<u64>,
}

impl RuntimeConfig {
    /// Validate runtime configuration
    pub fn validate(&self) -> Result<(), String> {
        // Validate port
        if self.port == 0 {
            return Err("Port cannot be 0".to_string());
        }

        // Validate host
        if self.host.is_empty() {
            return Err("Host cannot be empty".to_string());
        }

        // Validate log level
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&self.log_level.as_str()) {
            return Err(format!(
                "Invalid log level '{}'. Must be one of: {:?}",
                self.log_level, valid_levels
            ));
        }

        // Validate max concurrent requests
        if let Some(max) = self.max_concurrent_requests {
            if max == 0 {
                return Err("Max concurrent requests cannot be 0".to_string());
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeaturesConfig {
    pub streaming: bool,
    pub thinking: bool,
    pub critic: bool,
    pub caching: bool,
    pub hot_reload: Option<bool>,
    pub grpc: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcurrencyConfig {
    pub max_concurrent_agents: usize,
    pub max_concurrent_per_agent: usize,
    pub max_concurrent_requests: usize,
    pub permit_wait_timeout_ms: u64,
    pub queue_enabled: bool,
    pub queue_capacity: usize,
}

impl ConcurrencyConfig {
    /// Validate concurrency configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.max_concurrent_agents == 0 {
            return Err("Max concurrent agents cannot be 0".to_string());
        }

        if self.max_concurrent_per_agent == 0 {
            return Err("Max concurrent per agent cannot be 0".to_string());
        }

        if self.max_concurrent_requests == 0 {
            return Err("Max concurrent requests cannot be 0".to_string());
        }

        if self.permit_wait_timeout_ms == 0 {
            return Err("Permit wait timeout cannot be 0".to_string());
        }

        if self.queue_enabled && self.queue_capacity == 0 {
            return Err("Queue capacity cannot be 0 when queue is enabled".to_string());
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    pub request_timeout_ms: u64,
    pub pipeline_timeout_ms: u64,
    pub agent_timeout_ms: u64,
    pub thinking_timeout_ms: u64,
    pub plan_timeout_ms: u64,
    pub provider_timeout_ms: u64,
    pub tool_timeout_ms: u64,
    pub critic_timeout_ms: u64,
}

impl TimeoutConfig {
    /// Validate timeout configuration
    pub fn validate(&self) -> Result<(), String> {
        let timeouts = [
            ("request_timeout_ms", self.request_timeout_ms),
            ("pipeline_timeout_ms", self.pipeline_timeout_ms),
            ("agent_timeout_ms", self.agent_timeout_ms),
            ("thinking_timeout_ms", self.thinking_timeout_ms),
            ("plan_timeout_ms", self.plan_timeout_ms),
            ("provider_timeout_ms", self.provider_timeout_ms),
            ("tool_timeout_ms", self.tool_timeout_ms),
            ("critic_timeout_ms", self.critic_timeout_ms),
        ];

        for (name, value) in timeouts {
            if value == 0 {
                return Err(format!("{} cannot be 0", name));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub auth_mode: String,
    pub api_keys: Vec<String>,
    pub input_sanitization: Option<InputSanitizationConfig>,
    pub output_sanitization: Option<OutputSanitizationConfig>,
}

impl SecurityConfig {
    /// Validate security configuration
    pub fn validate(&self) -> Result<(), String> {
        let valid_auth_modes = ["none", "api_key", "bearer"];
        if !valid_auth_modes.contains(&self.auth_mode.as_str()) {
            return Err(format!(
                "Invalid auth mode '{}'. Must be one of: {:?}",
                self.auth_mode, valid_auth_modes
            ));
        }

        if self.auth_mode == "api_key" && self.api_keys.is_empty() {
            return Err("API keys cannot be empty when auth mode is 'api_key'".to_string());
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputSanitizationConfig {
    pub max_input_length: usize,
    pub injection_detection: bool,
    pub content_filter: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputSanitizationConfig {
    pub strip_tool_tags: bool,
    pub strip_system_tags: bool,
    pub pii_masking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CostOptimizationConfig {
    pub prompt_caching: bool,
    pub skip_thinking_for_simple: bool,
    pub simple_query_token_threshold: usize,
    pub skip_critic_for_simple: bool,
    pub max_output_tokens_per_agent: u32,
    pub sub_agent_caching: bool,
    pub cache_ttl_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegradationConfig {
    pub l0_to_l1: DegradationTrigger,
    pub l1_to_l2: DegradationTrigger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegradationTrigger {
    pub consecutive_failures: u32,
    pub error_rate_threshold: f32,
    pub latency_threshold_ms: u64,
    pub window_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub max_history_turns: usize,
    pub history_truncate_chars: usize,
    pub session_timeout_minutes: u64,
    pub memory_compression_threshold: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

/// Unified cache system configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedCacheConfig {
    /// Enable unified memory bus
    #[serde(default = "default_true")]
    pub enable_unified_bus: bool,
    /// L1 Hot cache capacity (per agent)
    #[serde(default = "default_hot_cache_capacity")]
    pub hot_cache_capacity: usize,
    /// L2 Warm cache capacity (per agent)
    #[serde(default = "default_warm_cache_capacity")]
    pub warm_cache_capacity: usize,
    /// L3 Shared cache capacity (global)
    #[serde(default = "default_shared_cache_capacity")]
    pub shared_cache_capacity: usize,
    /// Default cache mode
    #[serde(default)]
    pub default_cache_mode: CacheModeConfig,
    /// Always sync agent memories (not just in force mode)
    #[serde(default = "default_true")]
    pub always_sync_memories: bool,
    /// Default TTL for cache entries in seconds
    #[serde(default = "default_cache_ttl")]
    pub default_ttl_secs: u64,
    /// Session timeout in seconds
    #[serde(default = "default_session_timeout")]
    pub session_timeout_secs: u64,
    /// Background cleanup interval in seconds
    #[serde(default = "default_cleanup_interval")]
    pub cleanup_interval_secs: u64,
    /// Enable cross-session cache lookup
    #[serde(default = "default_true")]
    pub enable_cross_session_cache: bool,
}

impl Default for UnifiedCacheConfig {
    fn default() -> Self {
        Self {
            enable_unified_bus: true,
            hot_cache_capacity: 100,
            warm_cache_capacity: 500,
            shared_cache_capacity: 2000,
            default_cache_mode: CacheModeConfig::ReadWrite,
            always_sync_memories: true,
            default_ttl_secs: 3600,
            session_timeout_secs: 86400,
            cleanup_interval_secs: 600,
            enable_cross_session_cache: true,
        }
    }
}

fn default_hot_cache_capacity() -> usize {
    100
}
fn default_warm_cache_capacity() -> usize {
    500
}
fn default_shared_cache_capacity() -> usize {
    2000
}
fn default_cache_ttl() -> u64 {
    3600
}
fn default_session_timeout() -> u64 {
    86400
}
fn default_cleanup_interval() -> u64 {
    600
}

/// Serializable cache mode for configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CacheModeConfig {
    #[default]
    ReadWrite,
    WriteThrough,
    Bypass,
}

impl From<CacheModeConfig> for CacheMode {
    fn from(c: CacheModeConfig) -> Self {
        match c {
            CacheModeConfig::ReadWrite => CacheMode::ReadWrite,
            CacheModeConfig::WriteThrough => CacheMode::WriteThrough,
            CacheModeConfig::Bypass => CacheMode::Bypass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_valid() {
        let config = AppConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_empty_version_rejected() {
        let mut config = AppConfig::default();
        config.version = String::new();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_zero_port_rejected() {
        let mut config = AppConfig::default();
        config.runtime.port = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_empty_host_rejected() {
        let mut config = AppConfig::default();
        config.runtime.host = String::new();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_log_level_rejected() {
        let mut config = AppConfig::default();
        config.runtime.log_level = "verbose".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_zero_max_concurrent_rejected() {
        let mut config = AppConfig::default();
        config.runtime.max_concurrent_requests = Some(0);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_thinking_budget_zero_when_enabled_rejected() {
        let mut config = AppConfig::default();
        config.main_agent.thinking.enabled = true;
        config.main_agent.thinking.budget_tokens = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_thinking_strategy_rejected() {
        let mut config = AppConfig::default();
        config.main_agent.thinking.strategy = "Maybe".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_critic_enabled_with_zero_rounds_rejected() {
        let mut config = AppConfig::default();
        config.main_agent.critic.enabled = true;
        config.main_agent.critic.max_refinement_rounds = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_zero_plan_cache_capacity_rejected() {
        let mut config = AppConfig::default();
        config.main_agent.plan_cache.capacity = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_zero_total_timeout_rejected() {
        let mut config = AppConfig::default();
        config.main_agent.total_timeout_ms = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_empty_main_agent_id_rejected() {
        let mut config = AppConfig::default();
        config.main_agent.id = String::new();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_concurrency_zero_values_rejected() {
        let mut config = AppConfig::default();
        config.concurrency = Some(ConcurrencyConfig {
            max_concurrent_agents: 0,
            max_concurrent_per_agent: 1,
            max_concurrent_requests: 1,
            permit_wait_timeout_ms: 1000,
            queue_enabled: false,
            queue_capacity: 0,
        });
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_timeout_zero_values_rejected() {
        let mut config = AppConfig::default();
        config.timeouts = Some(TimeoutConfig {
            request_timeout_ms: 0,
            pipeline_timeout_ms: 1000,
            agent_timeout_ms: 1000,
            thinking_timeout_ms: 1000,
            plan_timeout_ms: 1000,
            provider_timeout_ms: 1000,
            tool_timeout_ms: 1000,
            critic_timeout_ms: 1000,
        });
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_security_invalid_auth_mode_rejected() {
        let mut config = AppConfig::default();
        config.security = Some(SecurityConfig {
            auth_mode: "oauth".to_string(),
            api_keys: vec![],
            input_sanitization: None,
            output_sanitization: None,
        });
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_security_api_key_mode_requires_keys() {
        let mut config = AppConfig::default();
        config.security = Some(SecurityConfig {
            auth_mode: "api_key".to_string(),
            api_keys: vec![],
            input_sanitization: None,
            output_sanitization: None,
        });
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_deserialize_config_json() {
        let json = r#"{
            "version": "1.0.0",
            "providers": {},
            "main_agent": {
                "id": "main",
                "thinking": {"enabled": false, "budget_tokens": 0, "strategy": "Never"},
                "critic": {"enabled": false, "max_refinement_rounds": 0},
                "plan_cache": {"ttl_secs": 60, "capacity": 100},
                "total_timeout_ms": 30000
            },
            "runtime": {
                "port": 8080,
                "host": "127.0.0.1",
                "log_level": "debug"
            }
        }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.version, "1.0.0");
        assert_eq!(config.runtime.port, 8080);
        assert!(config.validate().is_ok());
    }
}
