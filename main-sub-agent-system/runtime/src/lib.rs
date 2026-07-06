pub mod events;
pub mod hot_reload;
pub mod http;
pub mod rate_limit;
pub mod runtime;
pub mod sessions;
pub mod telemetry;
pub mod validation;

pub use runtime::RuntimeBuilder;

/// Resolve ${VAR_NAME} and ${VAR_NAME:-default} environment variable placeholders.
/// Single-pass O(n) implementation. Missing vars without default keep the original placeholder.
pub fn resolve_env_vars(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(start) = remaining.find("${") {
        if let Some(end) = remaining[start..].find('}') {
            let expr = &remaining[start + 2..start + end];
            output.push_str(&remaining[..start]);

            // Support ${VAR:-default} syntax
            if let Some(colon_pos) = expr.find(":-") {
                let var_name = &expr[..colon_pos];
                let default_value = &expr[colon_pos + 2..];
                match std::env::var(var_name) {
                    Ok(value) if !value.is_empty() => output.push_str(&value),
                    _ => output.push_str(default_value),
                }
            } else {
                match std::env::var(expr) {
                    Ok(value) => output.push_str(&value),
                    Err(_) => {
                        tracing::warn!("Environment variable {} not set, keeping placeholder", expr);
                        output.push_str(&remaining[start..start + end + 1]);
                    }
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
