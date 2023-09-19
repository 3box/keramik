//! Simulation is a k8s custom resource that defines a Ceramic simulation.

// Export all spec types
mod spec;
pub use spec::*;

// All other mods are behind the controller flag to keep the deps to a minimum
#[cfg(feature = "controller")]
pub(crate) mod controller;
#[cfg(feature = "controller")]
pub(crate) mod job;
#[cfg(feature = "controller")]
pub(crate) mod manager;
#[cfg(feature = "controller")]
pub(crate) mod redis;
#[cfg(feature = "controller")]
pub(crate) mod worker;

#[cfg(test)]
#[cfg(feature = "controller")]
pub mod stub;

#[cfg(feature = "controller")]
pub use controller::run;
