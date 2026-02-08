//! Boot — logging init, config load, Docker connection, state creation.

use std::sync::Arc;
use tracing::{info, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::conf::AgentConfig;
use crate::docker::client::DockerClient;
use crate::state::{AgentState, SharedState};
use crate::job;

/// Initialise the tracing / logging subsystem.
pub fn init_logging() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agent=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

/// Load config, connect to Docker, build shared state,
/// and spawn the background inventory-sync task.
///
/// Returns `(SharedState, AgentConfig)` on success.
pub async fn boot() -> Result<(SharedState, AgentConfig), Box<dyn std::error::Error>> {
    info!("Starting Docktail Agent v0.1.0 (Phase 1)");

    // Load configuration (file or env)
    let config = AgentConfig::load()?;
    info!("Loaded configuration: bind_address={}", config.bind_address);
    info!(
        "Multiline grouping: enabled={}, timeout={}ms, max_lines={}",
        config.multiline.enabled, config.multiline.timeout_ms, config.multiline.max_lines
    );

    // Initialize Docker client
    info!(
        "Connecting to Docker daemon at: {}",
        if config.docker_socket.is_empty() {
            "default socket"
        } else {
            &config.docker_socket
        }
    );

    let docker_client = DockerClient::new(&config.docker_socket).map_err(|e| {
        error!("Failed to connect to Docker: {}", e);
        e
    })?;

    info!("Successfully connected to Docker daemon");

    // Create shared application state
    let state = Arc::new(AgentState::new(docker_client, config.clone()));
    info!("Initialized shared application state");

    // Start background inventory sync task
    let sync_interval = config.inventory_sync_interval_secs;
    info!("Starting background inventory sync (interval: {}s)", sync_interval);
    tokio::spawn(job::background_inventory_sync(
        Arc::clone(&state),
        sync_interval,
    ));

    Ok((state, config))
}