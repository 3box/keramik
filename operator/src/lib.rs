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
