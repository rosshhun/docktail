mod agent;
mod config;
mod error;
mod graphql;
mod metrics;
mod state;

use anyhow::{Context, Result};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse, GraphQLSubscription};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{header, Method, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde_json::json;
use std::net::SocketAddr;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::timeout::TimeoutLayer;
use tracing::{info, warn};

use crate::{
    config::{ClusterConfig, LogFormat, LogOutput},
    graphql::{
        build_schema,
        types::{container::ContainerDetailsCache, log::ContainerLookupCache},
    },
    state::AppState,
};

// Combined state for axum router
#[derive(Clone)]
struct RouterState {
    app_state: AppState,
    schema: graphql::ClusterSchema,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Phase 1: Basic tracing so we can log during config loading
    // Uses set_default (thread-local) so it can be replaced by Phase 2's global subscriber
    let _basic_tracing = init_tracing_basic();

    info!("Starting Docktail Cluster API v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration
    let config = ClusterConfig::load()
        .context("Failed to load configuration")?;
    
    config.validate()
        .context("Configuration validation failed")?;

    // Phase 2: Re-initialize tracing with config (format, level)
    // Drop the phase-1 thread-local guard so the global subscriber slot is free
    drop(_basic_tracing);
    init_tracing_from_config(&config);

    info!("Configuration loaded successfully");
    info!("Server will bind to: {}", config.server.bind_address);

    // Create application state
    let state = AppState::new(config.clone());

    // Initialize application state (connects to agents)
    state.initialize()
        .await
        .context("Failed to initialize application state")?;

    // Build GraphQL schema
    let schema = build_schema(state.clone());

    info!("GraphQL schema built successfully");

    // Build the application router
    let router_state = RouterState {
        app_state: state.clone(),
        schema,
    };
    let app = build_router(router_state);

    // Parse bind address
    let addr: SocketAddr = config.server.bind_address
        .parse()
        .context("Invalid bind address")?;

    info!("Starting HTTP server...");
    info!("  - GraphQL endpoint: http://{}/graphql", addr);
    if config.graphql.enable_graphiql {
        info!("  - GraphiQL playground: http://{}/graphiql", addr);
    }
    info!("  - Health check: http://{}/health", addr);
    info!("  - Readiness check: http://{}/ready", addr);

    // Start server with graceful shutdown
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .context("Failed to bind to address")?;

    info!("✓ Docktail Cluster API is ready!");
    info!("Listening on: http://{}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Server error")?;

    // Signal all background tasks (health monitoring, etc.) to stop
    state.shutdown();

    info!("Server shut down gracefully");
    Ok(())
}

/// Build the application router
fn build_router(state: RouterState) -> Router {
    // CORS configuration
    let cors = if state.app_state.config.server.enable_cors {
        // Use the actual origins from config
        let origins = state.app_state.config.server.cors_origins
            .iter()
            .filter_map(|s| s.parse::<axum::http::HeaderValue>().ok())
            .collect::<Vec<_>>();

        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
            .allow_credentials(true)
    } else {
        // When CORS is disabled, use a restrictive layer (same-origin only)
        CorsLayer::new()
    };

    // Request timeout from config (applies to all non-streaming routes)
    let request_timeout = Duration::from_secs(state.app_state.config.server.write_timeout_secs);

    Router::new()
        // Health endpoints (no body limit needed)
        .route("/health", get(health_handler))
        .route("/ready", get(readiness_handler))
        .route("/metrics", get(metrics_handler))
        
        // GraphQL endpoints
        .route("/graphql", post(graphql_handler).get(graphql_playground))
        .route("/graphiql", get(graphql_playground))  // Alias for playground
        .route_service("/ws", GraphQLSubscription::new(state.schema.clone()))
        
        // Root endpoint
        .route("/", get(root_handler))
        
        .layer(
            ServiceBuilder::new()
                // Timeout for requests (prevents indefinitely hanging connections)
                .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, request_timeout))
                // Limit request body size to 2MB to prevent abuse
                .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
                .layer(cors)
        )
        .with_state(state)
}

/// Root handler - shows API info
async fn root_handler() -> Json<serde_json::Value> {
    Json(json!({
        "name": "Docktail Cluster API",
        "version": env!("CARGO_PKG_VERSION"),
        "endpoints": {
            "graphql": "/graphql",
            "graphiql": "/graphiql",
            "health": "/health",
            "ready": "/ready",
            "metrics": "/metrics"
        }
    }))
}

/// Health check handler - reflects actual agent pool health
async fn health_handler(
    State(state): State<RouterState>,
) -> impl IntoResponse {
    let total = state.app_state.agent_pool.count();
    let healthy = state.app_state.agent_pool.count_healthy();

    // Healthy if no agents configured, or at least one is healthy
    let is_healthy = total == 0 || healthy > 0;
    let status_code = if is_healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status_code,
        Json(json!({
            "status": if is_healthy { "healthy" } else { "unhealthy" },
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "agents": {
                "total": total,
                "healthy": healthy
            }
        })),
    )
}

/// Metrics endpoint
async fn metrics_handler(
    State(state): State<RouterState>,
) -> impl IntoResponse {
    let metrics = &state.app_state.metrics;
    let agent_pool = &state.app_state.agent_pool;
    
    Json(json!({
        "subscriptions": {
            "active": metrics.active_count(),
            "total_created": metrics.total_created(),
            "failed": metrics.failed_count(),
            "by_agent": metrics.subscriptions_by_agent()
        },
        "messages": {
            "total": metrics.total_messages(),
            "total_bytes": metrics.total_bytes(),
            "total_mb": (metrics.total_bytes() as f64) / (1024.0 * 1024.0)
        },
        "agents": {
            "total": agent_pool.count(),
            "healthy": agent_pool.count_healthy(),
            "degraded": agent_pool.count_degraded(),
            "unhealthy": agent_pool.count_unhealthy(),
            "unknown": agent_pool.count_unknown()
        }
    }))
}

/// Readiness check handler
async fn readiness_handler(
    State(state): State<RouterState>,
) -> impl IntoResponse {
    // Check if we have at least one healthy agent
    let total = state.app_state.agent_pool.count();
    let healthy = state.app_state.agent_pool.count_healthy();
    let unhealthy = state.app_state.agent_pool.count_unhealthy();
    
    // Ready if we have at least one healthy agent, or if no agents are configured
    let ready = total == 0 || healthy > 0;

    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status,
        Json(json!({
            "ready": ready,
            "agents": {
                "total": total,
                "healthy": healthy,
                "unhealthy": unhealthy
            }
        })),
    )
}

/// GraphQL query handler
async fn graphql_handler(
    State(state): State<RouterState>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    // Add per-request caches so they are scoped to this query (not shared globally)
    let request = req.into_inner()
        .data(ContainerDetailsCache::new())
        .data(ContainerLookupCache::new());
    state.schema.execute(request).await.into()
}

/// GraphQL playground (GraphiQL)
async fn graphql_playground(
    State(state): State<RouterState>,
) -> impl IntoResponse {
    if !state.app_state.config.graphql.enable_graphiql {
        return (
            StatusCode::NOT_FOUND,
            Html("GraphiQL is disabled")
        );
    }

    (
        StatusCode::OK,
        Html(
            r#"
            <!DOCTYPE html>
            <html>
            <head>
                <title>Docktail Cluster API - GraphiQL</title>
                <style>
                    body {
                        margin: 0;
                        overflow: hidden;
                    }
                    #graphiql {
                        height: 100vh;
                    }
                </style>
                <script crossorigin src="https://unpkg.com/react/umd/react.production.min.js"></script>
                <script crossorigin src="https://unpkg.com/react-dom/umd/react-dom.production.min.js"></script>
                <script crossorigin src="https://unpkg.com/graphiql/graphiql.min.js"></script>
                <link rel="stylesheet" href="https://unpkg.com/graphiql/graphiql.min.css" />
            </head>
            <body>
                <div id="graphiql"></div>
                <script>
                    // Dynamic protocol detection: use wss:// for HTTPS, ws:// for HTTP
                    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
                    const fetcher = GraphiQL.createFetcher({
                        url: '/graphql',
                        subscriptionUrl: protocol + '//' + location.host + '/ws',
                    });

                    const root = ReactDOM.createRoot(document.getElementById('graphiql'));
                    root.render(
                        React.createElement(GraphiQL, {
                            fetcher: fetcher,
                            defaultQuery: '# Welcome to Docktail Cluster API\n# GraphQL endpoint for aggregating multiple agents\n\n# Try this query:\nquery {\n  health {\n    status\n    timestamp\n  }\n  version\n}',
                        })
                    );
                </script>
            </body>
            </html>
            "#,
        ),
    )
}

/// Phase 1: Basic tracing init so we can log during config loading.
/// Uses RUST_LOG env var or a sensible default.
fn init_tracing_basic() -> tracing::subscriber::DefaultGuard {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,cluster=debug"));

    let subscriber = fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .finish();

    tracing::subscriber::set_default(subscriber)
}

/// Phase 2: Re-initialize tracing with configuration values.
/// This replaces the global subscriber with one that respects config.
fn init_tracing_from_config(config: &ClusterConfig) {
    use tracing_subscriber::{fmt, EnvFilter, prelude::*};
    use std::sync::Arc;

    // Prefer RUST_LOG env var, fall back to config level
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.logging.level));

    match (&config.logging.format, &config.logging.output) {
        (LogFormat::Json, LogOutput::Stdout) => {
            let layer = fmt::layer()
                .json()
                .with_target(true)
                .with_thread_ids(true);
            tracing_subscriber::registry().with(filter).with(layer).init();
        }
        (LogFormat::Json, LogOutput::File { path }) => {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap_or_else(|e| panic!("Failed to open log file '{}': {}", path, e));
            let layer = fmt::layer()
                .json()
                .with_target(true)
                .with_thread_ids(true)
                .with_ansi(false)
                .with_writer(Arc::new(file));
            tracing_subscriber::registry().with(filter).with(layer).init();
        }
        (LogFormat::Pretty, LogOutput::Stdout) => {
            let layer = fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
                .with_file(false)
                .with_line_number(false);
            tracing_subscriber::registry().with(filter).with(layer).init();
        }
        (LogFormat::Pretty, LogOutput::File { path }) => {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap_or_else(|e| panic!("Failed to open log file '{}': {}", path, e));
            let layer = fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
                .with_file(false)
                .with_line_number(false)
                .with_ansi(false)
                .with_writer(Arc::new(file));
            tracing_subscriber::registry().with(filter).with(layer).init();
        }
    }
}

/// Graceful shutdown signal handler
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            warn!("Received Ctrl+C, initiating graceful shutdown...");
        },
        _ = terminate => {
            warn!("Received SIGTERM, initiating graceful shutdown...");
        },
    }
}
