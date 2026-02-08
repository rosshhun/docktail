//! Serve — build the gRPC server and accept connections over mTLS.

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_stream::wrappers::TcpListenerStream;
use tokio_stream::StreamExt;
use tonic::transport::Server;
use tracing::{info, error, warn};

use crate::config::AgentConfig;
use crate::state::SharedState;
use crate::runtime::tls::TlsStreamWrapper;
use crate::runtime::stop::shutdown_signal;

use crate::proto::{
    log_service_server::LogServiceServer,
    inventory_service_server::InventoryServiceServer,
    health_service_server::HealthServiceServer,
    stats_service_server::StatsServiceServer,
    control_service_server::ControlServiceServer,
    swarm_service_server::SwarmServiceServer,
    shell_service_server::ShellServiceServer,
};

use crate::logs::LogServiceImpl;
use crate::container::InventoryServiceImpl;
use crate::health::HealthServiceImpl;
use crate::stats::StatsServiceImpl;
use crate::control::ControlServiceImpl;
use crate::swarm::route::SwarmServiceImpl;
use crate::shell::ShellServiceImpl;

/// Wire up all gRPC services, open the TLS listener, and serve until shutdown.
pub async fn serve(state: SharedState, config: AgentConfig) -> Result<(), Box<dyn std::error::Error>> {
    // Create service implementations
    let log_service = LogServiceImpl::new(Arc::clone(&state));
    let inventory_service = InventoryServiceImpl::new(Arc::clone(&state));
    let health_service = HealthServiceImpl::new(Arc::clone(&state.metrics), Arc::clone(&state));
    let stats_service = StatsServiceImpl::new(Arc::clone(&state));
    let control_service = ControlServiceImpl::new(Arc::clone(&state));
    let swarm_service = SwarmServiceImpl::new(Arc::clone(&state));
    let shell_service = ShellServiceImpl::new(Arc::clone(&state));

    let addr: SocketAddr = config.bind_address.parse().map_err(|e| {
        error!("Invalid bind address: {}", e);
        e
    })?;

    info!("gRPC server will bind to: {}", addr);

    config.validate().map_err(|e| {
        error!("TLS certificate validation failed: {}", e);
        error!("Set AGENT_TLS_CERT, AGENT_TLS_KEY, and AGENT_TLS_CA environment variables");
        e
    })?;

    info!("Loading TLS certificates...");
    let rustls_config = config.build_rustls_config().map_err(|e| {
        error!("Failed to load TLS certificates: {}", e);
        e
    })?;

    info!("✓ TLS certificates loaded successfully");
    info!("✓ mTLS enabled - client certificates required");

    let tls_acceptor = TlsAcceptor::from(rustls_config);
    let listener = TcpListener::bind(addr).await?;

    info!("✓ Registered LogService");
    info!("✓ Registered InventoryService");
    info!("✓ Registered HealthService");
    info!("✓ Registered StatsService");
    info!("✓ Registered ControlService");
    info!("✓ Registered SwarmService");
    info!("✓ Registered ShellService");
    info!("");
    info!("========================================");
    info!("Docktail Agent is ready!");
    info!("Listening on: {} (mTLS enabled)", addr);
    info!("Max concurrent streams: {}", config.max_concurrent_streams);
    info!("Press Ctrl+C to shutdown gracefully");
    info!("========================================");
    info!("");

    let incoming = TcpListenerStream::new(listener)
        .then(move |result| {
            let tls_acceptor = tls_acceptor.clone();
            async move {
                match result {
                    Ok(stream) => match tls_acceptor.accept(stream).await {
                        Ok(tls_stream) => Some(Ok::<_, std::io::Error>(TlsStreamWrapper(tls_stream))),
                        Err(e) => {
                            warn!("TLS handshake failed: {}", e);
                            None
                        }
                    },
                    Err(e) => {
                        error!("TCP accept error: {}", e);
                        None
                    }
                }
            }
        })
        .filter_map(|x| x);

    Server::builder()
        .initial_stream_window_size(1 << 20) // 1 MiB
        .concurrency_limit_per_connection(config.max_concurrent_streams)
        .add_service(LogServiceServer::new(log_service))
        .add_service(InventoryServiceServer::new(inventory_service))
        .add_service(HealthServiceServer::new(health_service))
        .add_service(StatsServiceServer::new(stats_service))
        .add_service(ControlServiceServer::new(control_service))
        .add_service(SwarmServiceServer::new(swarm_service))
        .add_service(ShellServiceServer::new(shell_service))
        .serve_with_incoming_shutdown(incoming, shutdown_signal())
        .await?;

    info!("Server shutdown complete");
    Ok(())
}
