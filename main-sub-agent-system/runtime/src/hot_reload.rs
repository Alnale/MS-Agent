use std::path::PathBuf;
use std::time::SystemTime;

use agent_teams_core::config::AppConfig;
use tokio::fs;

/// Tool configuration change
#[derive(Debug, Clone)]
pub struct ToolConfigChange {
    pub tool_name: String,
    pub action: ToolConfigAction,
}

#[derive(Debug, Clone)]
pub enum ToolConfigAction {
    Register,
    Unregister,
    Update,
}

/// Changes that can be applied via hot-reload
#[derive(Debug, Clone)]
pub struct HotReloadChanges {
    pub log_level_changed: Option<String>,
    pub max_concurrent_requests_changed: Option<usize>,
    pub request_timeout_changed: Option<u64>,
    pub pipeline_timeout_changed: Option<u64>,
    pub cost_optimization_changed: Option<agent_teams_core::config::CostOptimizationConfig>,
    /// Tool configuration changes detected
    pub tool_changes: Vec<ToolConfigChange>,
}

impl HotReloadChanges {
    pub fn has_changes(&self) -> bool {
        self.log_level_changed.is_some()
            || self.max_concurrent_requests_changed.is_some()
            || self.request_timeout_changed.is_some()
            || self.pipeline_timeout_changed.is_some()
            || self.cost_optimization_changed.is_some()
            || !self.tool_changes.is_empty()
    }
}

/// Configuration hot-reload watcher (polling-based).
///
/// **Limitations**: This watcher detects config file changes and logs them,
/// but does NOT apply certain changes at runtime:
/// - Log level changes require a tracing-subscriber rebuild (server restart)
/// - Concurrency limit changes require a server restart
/// - Only `request_timeout`, `pipeline_timeout`, and `cost_optimization` are
///   safe to hot-reload without restart
pub struct HotReloadWatcher {
    config_path: PathBuf,
    last_modified: Option<SystemTime>,
    current_config: Option<AppConfig>,
}

impl HotReloadWatcher {
    pub fn new(config_path: &str) -> Self {
        let last_modified = std::fs::metadata(config_path)
            .and_then(|m| m.modified())
            .ok();
        Self {
            config_path: PathBuf::from(config_path),
            last_modified,
            current_config: None,
        }
    }

    /// Check if config has changed since last check.
    /// Returns Some(HotReloadChanges) if changed, None otherwise.
    pub async fn check_reload(&mut self) -> Option<HotReloadChanges> {
        let modified = fs::metadata(&self.config_path)
            .await
            .and_then(|m| m.modified())
            .ok();

        let should_reload = match (modified, self.last_modified) {
            (Some(current), Some(last)) if current > last => true,
            (Some(_), None) => true,
            _ => false,
        };

        if should_reload {
            self.last_modified = modified;
            match fs::read_to_string(&self.config_path).await {
                Ok(content) => {
                    // Resolve environment variables
                    let content = crate::resolve_env_vars(&content);
                    match serde_json::from_str::<AppConfig>(&content) {
                        Ok(new_config) => {
                            // Validate the new config
                            if let Err(e) = new_config.validate() {
                                tracing::warn!("Hot-reload config validation failed: {}", e);
                                return None;
                            }

                            // Calculate diff
                            let changes = self.calculate_changes(&new_config);
                            self.current_config = Some(new_config);

                            if changes.has_changes() {
                                Some(changes)
                            } else {
                                None
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse hot-reload config: {}", e);
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to read hot-reload config: {}", e);
                    None
                }
            }
        } else {
            None
        }
    }

    /// Calculate changes between current and new config
    fn calculate_changes(&self, new_config: &AppConfig) -> HotReloadChanges {
        let old_config = &self.current_config;

        let log_level_changed = old_config
            .as_ref()
            .and_then(|old| {
                if old.runtime.log_level != new_config.runtime.log_level {
                    Some(new_config.runtime.log_level.clone())
                } else {
                    None
                }
            })
            .or_else(|| {
                if old_config.is_none() {
                    Some(new_config.runtime.log_level.clone())
                } else {
                    None
                }
            });

        let max_concurrent_requests_changed = old_config
            .as_ref()
            .and_then(|old| {
                if old.runtime.max_concurrent_requests != new_config.runtime.max_concurrent_requests
                {
                    new_config.runtime.max_concurrent_requests
                } else {
                    None
                }
            })
            .or(new_config.runtime.max_concurrent_requests);

        let request_timeout_changed = old_config
            .as_ref()
            .and_then(|old| match (&old.timeouts, &new_config.timeouts) {
                (Some(old_t), Some(new_t))
                    if old_t.request_timeout_ms != new_t.request_timeout_ms =>
                {
                    Some(new_t.request_timeout_ms)
                }
                (None, Some(new_t)) => Some(new_t.request_timeout_ms),
                _ => None,
            })
            .or_else(|| new_config.timeouts.as_ref().map(|t| t.request_timeout_ms));

        let pipeline_timeout_changed = old_config
            .as_ref()
            .and_then(|old| match (&old.timeouts, &new_config.timeouts) {
                (Some(old_t), Some(new_t))
                    if old_t.pipeline_timeout_ms != new_t.pipeline_timeout_ms =>
                {
                    Some(new_t.pipeline_timeout_ms)
                }
                (None, Some(new_t)) => Some(new_t.pipeline_timeout_ms),
                _ => None,
            })
            .or_else(|| new_config.timeouts.as_ref().map(|t| t.pipeline_timeout_ms));

        let cost_optimization_changed = old_config
            .as_ref()
            .and_then(|old| {
                if old.cost_optimization != new_config.cost_optimization {
                    new_config.cost_optimization.clone()
                } else {
                    None
                }
            })
            .or_else(|| new_config.cost_optimization.clone());

        HotReloadChanges {
            log_level_changed,
            max_concurrent_requests_changed,
            request_timeout_changed,
            pipeline_timeout_changed,
            cost_optimization_changed,
            tool_changes: Vec::new(),
        }
    }

    /// Get the current config
    pub fn current_config(&self) -> Option<&AppConfig> {
        self.current_config.as_ref()
    }

    pub fn config_path(&self) -> &str {
        self.config_path.to_str().unwrap_or("")
    }
}
