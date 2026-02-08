//! Map — error mapping helpers for control service.
//!
//! Re-exports the shared Docker error mapper from [`crate::docker::error_map`].

pub use crate::docker::error_map::map_docker_error;
