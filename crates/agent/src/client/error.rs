//! Error — re-exports DockerError from the docker module.
//!
//! When the docker module is fully migrated into client,
//! the canonical error type will live here.

pub use crate::docker::client::DockerError;