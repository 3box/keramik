//! Provides helper functions for initializing telemetry collection and publication.
use std::{convert::Infallible, net::SocketAddr, str::FromStr, sync::OnceLock, time::Duration};

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
use tracing_subscriber::{
    filter::LevelFilter,
    fmt::{
        format::{Compact, DefaultFields, Json, JsonFields, Pretty},
        time::SystemTime,
        FormatEvent, FormatFields,
    },
    prelude::*,
    EnvFilter, Registry,
};

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Default,
    clap::ValueEnum,
    serde::Deserialize,
    serde::Serialize,
)]
/// The format to use for logging
#[serde(rename_all = "camelCase")]
pub enum LogFormat {
    /// Compact format
    SingleLine,
    /// Pretty format
    #[default]
    MultiLine,
    /// JSON format
    Json,
}

impl FromStr for LogFormat {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "singleLine" | "single-line" | "SINGLE_LINE" => Ok(LogFormat::SingleLine),
            "multiLine" | "multi-line" | "MULTI_LINE" => Ok(LogFormat::MultiLine),
            "json" => Ok(LogFormat::Json),
            _ => Err(anyhow::anyhow!("invalid log format: {}", s)),
        }
    }
}

impl std::fmt::Display for LogFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // we match the clap enum format to make it easier when passing as an argument
            LogFormat::SingleLine => write!(f, "single-line"),
            LogFormat::MultiLine => write!(f, "multi-line"),
            LogFormat::Json => write!(f, "json"),
        }
    }
}

// create a new prometheus registry
static PROM_REGISTRY: OnceLock<prometheus::Registry> = OnceLock::new();

/// Initialize tracing
pub async fn init_tracing(otlp_endpoint: Option<String>, log_format: LogFormat) -> Result<()> {
    //// Setup log filter
    //// Default to INFO if no env is specified
    let log_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env()?;

    let fields_format = FieldsFormat::new(log_format);
    let event_format = EventFormat::new(log_format);

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
            .event_format(event_format)
            .fmt_fields(fields_format)
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
            .event_format(event_format)
            .fmt_fields(fields_format)
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

// Implement a FormatEvent type that can be configured to one of a set of log formats.
struct EventFormat {
    kind: LogFormat,
    single: tracing_subscriber::fmt::format::Format<Compact, SystemTime>,
    multi: tracing_subscriber::fmt::format::Format<Pretty, SystemTime>,
    json: tracing_subscriber::fmt::format::Format<Json, SystemTime>,
}

impl EventFormat {
    fn new(kind: LogFormat) -> Self {
        Self {
            kind,
            single: tracing_subscriber::fmt::format().compact(),
            multi: tracing_subscriber::fmt::format().pretty(),
            json: tracing_subscriber::fmt::format().json(),
        }
    }
}

impl<S, N> FormatEvent<S, N> for EventFormat
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        match self.kind {
            LogFormat::SingleLine => self.single.format_event(ctx, writer, event),
            LogFormat::MultiLine => self.multi.format_event(ctx, writer, event),
            LogFormat::Json => self.json.format_event(ctx, writer, event),
        }
    }
}

// Implement a FormatFields type that can be configured to one of a set of log formats.
struct FieldsFormat {
    kind: LogFormat,
    default_fields: DefaultFields,
    json_fields: JsonFields,
}

impl FieldsFormat {
    pub fn new(kind: LogFormat) -> Self {
        Self {
            kind,
            default_fields: DefaultFields::new(),
            json_fields: JsonFields::new(),
        }
    }
}

impl<'writer> FormatFields<'writer> for FieldsFormat {
    fn format_fields<R: tracing_subscriber::prelude::__tracing_subscriber_field_RecordFields>(
        &self,
        writer: tracing_subscriber::fmt::format::Writer<'writer>,
        fields: R,
    ) -> std::fmt::Result {
        match self.kind {
            LogFormat::SingleLine => self.default_fields.format_fields(writer, fields),
            LogFormat::MultiLine => self.default_fields.format_fields(writer, fields),
            LogFormat::Json => self.json_fields.format_fields(writer, fields),
        }
    }
}
