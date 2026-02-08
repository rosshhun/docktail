// Domain-driven module structure for the Docktail Agent.

// Core infrastructure
pub mod docker;
pub mod filter;
pub mod config;
pub mod state;
pub mod parser;

// Domain modules
pub mod runtime;
pub mod conf;
pub mod client;
pub mod swarm;
pub mod logs;
pub mod container;
pub mod stats;
pub mod control;
pub mod shell;
pub mod health;
pub mod proto;
pub mod job;
