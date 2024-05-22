//! Provides API for the operator and related tooling.
#![warn(missing_docs)]

#[cfg(feature = "controller")]
pub(crate) mod labels;
#[cfg(feature = "controller")]
pub mod monitoring;
pub mod network;
pub mod simulation;
#[cfg(feature = "controller")]
pub mod utils;

/// A list of constants used in various K8s resources
#[cfg(feature = "controller")]
const CONTROLLER_NAME: &str = "keramik";

static NETWORK_LOG_FORMAT: std::sync::OnceLock<keramik_common::telemetry::LogFormat> =
    std::sync::OnceLock::new();

/// Sets the log format for the network
pub fn set_network_log_format(format: keramik_common::telemetry::LogFormat) {
    let _ = NETWORK_LOG_FORMAT.get_or_init(|| format);
}

/// Sets the log format for the network. Not public outside of main
pub(crate) fn network_log_format() -> keramik_common::telemetry::LogFormat {
    NETWORK_LOG_FORMAT
        .get_or_init(keramik_common::telemetry::LogFormat::default)
        .to_owned()
}
