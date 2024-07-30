//! Provides API for the operator and related tooling.
#![warn(missing_docs)]

/// Labels module for managing resource labels.
#[cfg(feature = "controller")]
pub(crate) mod labels;
/// Lgen module for generating load.
pub mod lgen;
/// Monitoring module for monitoring resources.
#[cfg(feature = "controller")]
pub mod monitoring;
/// Network module for managing network resources.
pub mod network;
/// Simulation module for running simulations.
pub mod simulation;
/// Utils module for shared utility functions.
#[cfg(feature = "controller")]
pub mod utils;

/// A list of constants used in various K8s resources
#[cfg(feature = "controller")]
const CONTROLLER_NAME: &str = "keramik";
