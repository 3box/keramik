/// Lgen controller arm module for reconciling load generator resources.
#[cfg(feature = "controller")]
pub mod controller;
/// Job module for creating load generator jobs.
#[cfg(feature = "controller")]
pub mod job;
/// Spec module for creating load generator specs.
#[cfg(feature = "controller")]
pub mod spec;
