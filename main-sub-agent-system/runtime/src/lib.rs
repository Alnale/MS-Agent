pub mod events;
pub mod hot_reload;
pub mod http;
pub mod rate_limit;
pub mod runtime;
pub mod sessions;
pub mod telemetry;
pub mod validation;

pub use runtime::RuntimeBuilder;

/// Resolve ${VAR_NAME} environment variable placeholders in a string.
/// Single-pass O(n) implementation. Missing vars keep the original ${VAR} placeholder.
pub fn resolve_env_vars(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(start) = remaining.find("${") {
        if let Some(end) = remaining[start..].find('}') {
            let var_name = &remaining[start + 2..start + end];
            output.push_str(&remaining[..start]);
            match std::env::var(var_name) {
                Ok(value) => output.push_str(&value),
                Err(_) => {
                    tracing::warn!("Environment variable {} not set, keeping placeholder", var_name);
                    output.push_str(&remaining[start..start + end + 1]);
                }
            }
            remaining = &remaining[start + end + 1..];
        } else {
            break;
        }
    }
    output.push_str(remaining);
    output
}
