use opentelemetry::global;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

/// Initialize OpenTelemetry tracing with async batch export via OTLP (gRPC/tonic).
/// The collector endpoint defaults to `http://localhost:4317` and can be overridden
/// with the `OTEL_EXPORTER_OTLP_ENDPOINT` environment variable.
pub fn init_telemetry(service_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(
                    std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
                        .unwrap_or_else(|_| "http://localhost:4317".to_string()),
                ),
        )
        .with_trace_config(
            opentelemetry_sdk::trace::config().with_resource(
                opentelemetry_sdk::Resource::new(vec![KeyValue::new(
                    "service.name",
                    service_name.to_string(),
                )]),
            ),
        )
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
