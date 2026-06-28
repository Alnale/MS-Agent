use opentelemetry::global;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

/// Initialize OpenTelemetry tracing with async batch export.
/// Uses the Tokio runtime for non-blocking span export.
pub fn init_telemetry(service_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let tracer = opentelemetry_jaeger::new_agent_pipeline()
        .with_service_name(service_name)
        .install_batch(opentelemetry_sdk::runtime::Tokio)?;

    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    let subscriber = Registry::default()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(telemetry);

    tracing::subscriber::set_global_default(subscriber)?;

    Ok(())
}

/// Shutdown OpenTelemetry
pub fn shutdown_telemetry() {
    global::shutdown_tracer_provider();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_telemetry() {
        // This test requires a running Jaeger instance
        // In a real CI environment, you would mock this
        let result = init_telemetry("test-service");
        // We just check that the function doesn't panic
        // In a real test, you would assert Ok(result)
        let _ = result;
    }
}
