// #![allow(dead_code)]

use std::sync::Arc;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::signal;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_rustls::TlsAcceptor;
use tokio_stream::wrappers::TcpListenerStream;
use tokio_stream::StreamExt;
use tonic::transport::Server;
use tracing::{info, error, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod service;
mod docker;
mod filter;
mod config;
mod state;
mod parser;

use config::AgentConfig;
use docker::client::DockerClient;
use state::AgentState;
use service::{
    LogServiceImpl, InventoryServiceImpl, HealthServiceImpl, StatsServiceImpl, ControlServiceImpl, SwarmServiceImpl, ShellServiceImpl,
    LogServiceServer, InventoryServiceServer, HealthServiceServer, StatsServiceServer, ControlServiceServer, SwarmServiceServer, ShellServiceServer,
};

/// Wrapper for TlsStream that implements tonic's Connected trait
struct TlsStreamWrapper(tokio_rustls::server::TlsStream<TcpStream>);

impl tonic::transport::server::Connected for TlsStreamWrapper {
    type ConnectInfo = TlsConnectInfo;

    fn connect_info(&self) -> Self::ConnectInfo {
        TlsConnectInfo {
            peer_addr: self.0.get_ref().0.peer_addr().ok(),
        }
    }
}

#[derive(Clone, Debug)]
struct TlsConnectInfo {
    peer_addr: Option<SocketAddr>,
}

impl AsyncRead for TlsStreamWrapper {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for TlsStreamWrapper {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agent=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting Docktail Agent v0.1.0 (Phase 1)");

    // Load configuration (file or env)
    let config = AgentConfig::load()?;
    info!("Loaded configuration: bind_address={}", config.bind_address);
    info!("Multiline grouping: enabled={}, timeout={}ms, max_lines={}", 
        config.multiline.enabled, config.multiline.timeout_ms, config.multiline.max_lines);

    // Initialize Docker client
    info!("Connecting to Docker daemon at: {}", 
        if config.docker_socket.is_empty() { "default socket" } else { &config.docker_socket });
    
    let docker_client = DockerClient::new(&config.docker_socket)
        .map_err(|e| {
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
    tokio::spawn(service::background::background_inventory_sync(
        Arc::clone(&state),
        sync_interval,
    ));

    // Create service implementations
    let log_service = LogServiceImpl::new(Arc::clone(&state));
    let inventory_service = InventoryServiceImpl::new(Arc::clone(&state));
    let health_service = HealthServiceImpl::new(Arc::clone(&state.metrics));
    let stats_service = StatsServiceImpl::new(Arc::clone(&state));
    let control_service = ControlServiceImpl::new(Arc::clone(&state));
    let swarm_service = SwarmServiceImpl::new(Arc::clone(&state));
    let shell_service = ShellServiceImpl::new(Arc::clone(&state));

    let addr: SocketAddr = config.bind_address.parse()
        .map_err(|e| {
            error!("Invalid bind address: {}", e);
            e
        })?;

    info!("gRPC server will bind to: {}", addr);

    config.validate()
        .map_err(|e| {
            error!("TLS certificate validation failed: {}", e);
            error!("Set AGENT_TLS_CERT, AGENT_TLS_KEY, and AGENT_TLS_CA environment variables");
            e
        })?;

    info!("Loading TLS certificates...");
    let rustls_config = config.build_rustls_config()
        .map_err(|e| {
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
                    Ok(stream) => {
                        match tls_acceptor.accept(stream).await {
                            Ok(tls_stream) => {
                                Some(Ok::<_, std::io::Error>(TlsStreamWrapper(tls_stream)))
                            }
                            Err(e) => {
                                warn!("TLS handshake failed: {}", e);
                                None
                            }
                        }
                    }
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

/// Graceful shutdown signal handler
/// Listens for SIGINT (Ctrl+C) or SIGTERM
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C signal, initiating graceful shutdown...");
        },
        _ = terminate => {
            info!("Received SIGTERM signal, initiating graceful shutdown...");
        },
    }

    info!("Draining active streams and closing connections...");
}