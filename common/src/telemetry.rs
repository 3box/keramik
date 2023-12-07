//! Provides helper functions for initializing telemetry collection and publication.
use std::time::Duration;

use anyhow::Result;
use opentelemetry::{
    runtime,
    sdk::{
        export::metrics::aggregation,
        metrics::{controllers::BasicController, selectors},
    },
};
use opentelemetry_otlp::WithExportConfig;
use tracing_subscriber::{filter::LevelFilter, prelude::*, EnvFilter, Registry};

/// Initialize tracing and metrics
pub async fn init(otlp_endpoint: String) -> Result<BasicController> {
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(otlp_endpoint.clone()),
        )
        .with_trace_config(opentelemetry::sdk::trace::config().with_resource(
            opentelemetry::sdk::Resource::new(vec![
                opentelemetry::KeyValue::new(
                    "hostname",
                    gethostname::gethostname()
                        .into_string()
                        .expect("hostname should be valid utf-8"),
                ),
                opentelemetry::KeyValue::new(
                "service.name",
                "keramik",
            )]),
        ))
        .install_batch(runtime::Tokio)?;

    let meter = opentelemetry_otlp::new_pipeline()
        .metrics(
            selectors::simple::histogram([1.0, 2.0, 5.0, 10.0, 20.0, 50.0]),
            aggregation::cumulative_temporality_selector(),
            runtime::Tokio,
        )
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(otlp_endpoint),
        )
        .with_resource(opentelemetry::sdk::Resource::new(vec![
            opentelemetry::KeyValue::new(
                "hostname",
                gethostname::gethostname()
                    .into_string()
                    .expect("hostname should be valid utf-8"),
            ),
            opentelemetry::KeyValue::new("service.name", "keramik"),
        ]))
        .with_period(Duration::from_secs(10))
        // Build starts the meter and sets it as the global meter provider
        .build()?;

    // Setup filters
    // Default to INFO if no env is specified
    let log_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env()?;
    let otlp_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env()?;

    // Setup tracing layers
    let telemetry = tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_filter(otlp_filter);
    let logger = tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .compact()
        .with_filter(log_filter);

    let collector = Registry::default().with(telemetry).with(logger);

    #[cfg(feature = "tokio-console")]
    let collector = {
        let console_filter = EnvFilter::builder().parse("tokio=trace,runtime=trace")?;
        let console_layer = console_subscriber::spawn().with_filter(console_filter);
        collector.with(console_layer)
    };

    // Initialize tracing
    tracing::subscriber::set_global_default(collector)?;

    Ok(meter)
}
