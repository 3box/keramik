//! Provides helper functions for initializing telemetry collection and publication.
use std::{convert::Infallible, net::SocketAddr, sync::OnceLock, time::Duration};

use anyhow::Result;
use hyper::{
    body::Body,
    http::HeaderValue,
    service::{make_service_fn, service_fn},
    Request, Response,
};
use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    metrics::{
        reader::{DefaultAggregationSelector, DefaultTemporalitySelector},
        MeterProvider,
    },
    runtime, Resource,
};
use prometheus::{Encoder, TextEncoder};
use tokio::{sync::oneshot, task::JoinHandle};
use tracing_subscriber::{filter::LevelFilter, prelude::*, EnvFilter, Registry};

// create a new prometheus registry
static PROM_REGISTRY: OnceLock<prometheus::Registry> = OnceLock::new();

/// Initialize tracing
pub async fn init_tracing(otlp_endpoint: Option<String>) -> Result<()> {
    //// Setup log filter
    //// Default to INFO if no env is specified
    let log_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env()?;

    // If we have an otlp_endpoint setup export of traces
    if let Some(otlp_endpoint) = otlp_endpoint {
        let tracer = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(
                opentelemetry_otlp::new_exporter()
                    .tonic()
                    .with_endpoint(otlp_endpoint.clone()),
            )
            .with_trace_config(
                opentelemetry_sdk::trace::config().with_resource(Resource::new(vec![
                    opentelemetry::KeyValue::new(
                        "hostname",
                        gethostname::gethostname()
                            .into_string()
                            .expect("hostname should be valid utf-8"),
                    ),
                    opentelemetry::KeyValue::new("service.name", "keramik"),
                ])),
            )
            .install_batch(runtime::Tokio)?;

        // Setup otlp export filter
        // Default to INFO if no env is specified
        let otlp_filter = EnvFilter::builder()
            .with_default_directive(LevelFilter::INFO.into())
            .from_env()?;

        // Setup tracing layers
        let telemetry = tracing_opentelemetry::layer()
            .with_tracer(tracer)
            .with_filter(otlp_filter);
        // Setup logging to stdout
        let logger = tracing_subscriber::fmt::layer()
            .with_ansi(true)
            .pretty()
            .with_filter(log_filter);

        let collector = Registry::default().with(telemetry).with(logger);

        #[cfg(feature = "tokio-console")]
        let collector = {
            let console_filter = EnvFilter::builder().parse("tokio=trace,runtime=trace")?;
            let console_layer = console_subscriber::spawn().with_filter(console_filter);
            collector.with(console_layer)
        };

        tracing::subscriber::set_global_default(collector)?;
    } else {
        // Setup basic log only tracing
        let logger = tracing_subscriber::fmt::layer()
            .with_ansi(true)
            .pretty()
            .with_filter(log_filter);
        tracing_subscriber::registry().with(logger).init()
    }
    Ok(())
}

/// Initialize metrics such that metrics are pushed to the otlp_endpoint.
pub async fn init_metrics_otlp(otlp_endpoint: String) -> Result<MeterProvider> {
    // configure OpenTelemetry to use this registry
    let meter = opentelemetry_otlp::new_pipeline()
        .metrics(runtime::Tokio)
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(otlp_endpoint),
        )
        .with_resource(Resource::new(vec![
            opentelemetry::KeyValue::new(
                "hostname",
                gethostname::gethostname()
                    .into_string()
                    .expect("hostname should be valid utf-8"),
            ),
            opentelemetry::KeyValue::new("service.name", "keramik"),
        ]))
        .with_period(Duration::from_secs(10))
        .with_aggregation_selector(DefaultAggregationSelector::new())
        .with_temporality_selector(DefaultTemporalitySelector::new())
        // Build starts the meter and sets it as the global meter provider
        .build()?;

    Ok(meter)
}

type MetricsServerShutdown = oneshot::Sender<()>;
type MetricsServerJoin = JoinHandle<Result<(), hyper::Error>>;

/// Initialize metrics such that metrics are expose via a Prometheus scrape endpoint.
/// A send on the MetricsServerShutdown channel will cause the server to shutdown gracefully.
pub async fn init_metrics_prom(
    addr: &SocketAddr,
) -> Result<(MeterProvider, MetricsServerShutdown, MetricsServerJoin)> {
    let prom_registry = prometheus::Registry::default();
    let prom_exporter = opentelemetry_prometheus::exporter()
        .with_registry(prom_registry.clone())
        .build()?;
    PROM_REGISTRY
        .set(prom_registry)
        .expect("should be able to set PROM_REGISTRY");
    let meter = MeterProvider::builder().with_reader(prom_exporter).build();
    global::set_meter_provider(meter.clone());

    let (shutdown, join) = start_metrics_server(addr);
    Ok((meter, shutdown, join))
}

// /metrics scrape endpoint.
async fn handle(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let mut data = Vec::new();
    let encoder = TextEncoder::new();
    let metric_families = PROM_REGISTRY
        .get()
        .expect("PROM_REGISTRY should be initialized")
        .gather();
    encoder.encode(&metric_families, &mut data).unwrap();

    // Add EOF marker, this is part of openmetrics spec but not prometheus
    // So we have to add it ourselves.
    // https://github.com/OpenObservability/OpenMetrics/blob/main/specification/OpenMetrics.md#overall-structure
    data.extend(b"# EOF\n");

    let mut resp = Response::new(Body::from(data));
    resp.headers_mut().insert(
        "Content-Type",
        // Use OpenMetrics content type so prometheus knows to parse it accordingly
        HeaderValue::from_static("application/openmetrics-text"),
    );
    Ok(resp)
}

// Start metrics server.
// Sending on the returned channel will cause the server to shutdown gracefully.
fn start_metrics_server(addr: &SocketAddr) -> (MetricsServerShutdown, MetricsServerJoin) {
    let (tx, rx) = oneshot::channel::<()>();
    let service = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle)) });

    let server = hyper::Server::bind(addr)
        .serve(service)
        .with_graceful_shutdown(async {
            rx.await.ok();
        });
    (tx, tokio::spawn(server))
}
