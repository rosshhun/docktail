mod agent;
mod config;
mod error;
mod graphql;
mod metrics;
mod state;

use anyhow::{Context, Result};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse, GraphQLSubscription};
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::{header, Method, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::{get, post, delete},
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
    agent::discovery::AgentDiscovery,
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

    // Start Swarm-based agent discovery (if enabled)
    if config.discovery.swarm_discovery {
        let discovery = AgentDiscovery::new(
            state.agent_pool.clone(),
            config.discovery.clone(),
            state.shutdown_tx.subscribe(),
        );
        tokio::spawn(async move {
            discovery.start_swarm_discovery().await;
        });
        info!("✓ Swarm agent discovery enabled (label={}, interval={}s)",
            config.discovery.discovery_label,
            config.discovery.discovery_interval_secs,
        );
    }

    if config.discovery.registration_enabled {
        info!("✓ Agent registration API enabled at POST /api/agents/register");
    }

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

    // Shell / Exec WebSocket endpoint (uses its own AppState)
    let shell_router = Router::new()
        .route("/shell", get(graphql::shell_ws::shell_ws_handler))
        .with_state(state.app_state.clone());

    // Agent registration API — only mount when registration is enabled.
    // When disabled, none of the /api/agents endpoints are reachable,
    // preventing leakage of internal agent metadata.
    let api_router = if state.app_state.config.discovery.registration_enabled {
        Router::new()
            .route("/api/agents/register", post(register_agent_handler))
            .route("/api/agents/{id}", delete(deregister_agent_handler))
            .route("/api/agents", get(list_agents_handler))
            .with_state(state.app_state.clone())
    } else {
        Router::new().with_state(state.app_state.clone())
    };

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
        
        // Shell WebSocket endpoint (uses AppState directly)
        .merge(shell_router)
        
        // Agent registration API
        .merge(api_router)
        
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

// ============================================================================
// Agent Registration API Handlers
// ============================================================================

/// Agent registration request body
#[derive(serde::Deserialize)]
struct RegisterAgentRequest {
    /// Unique agent ID
    id: String,
    /// Human-readable name
    name: String,
    /// gRPC address (host:port)
    address: String,
    /// TLS certificate PEM path (optional, falls back to discovery config)
    tls_cert: Option<String>,
    /// TLS key PEM path (optional)
    tls_key: Option<String>,
    /// TLS CA PEM path (optional)
    tls_ca: Option<String>,
    /// TLS domain override (optional)
    tls_domain: Option<String>,
    /// Optional labels
    #[serde(default)]
    labels: std::collections::HashMap<String, String>,
}

/// POST /api/agents/register — register a new agent dynamically
async fn register_agent_handler(
    State(state): State<AppState>,
    Json(body): Json<RegisterAgentRequest>,
) -> impl IntoResponse {
    if !state.config.discovery.registration_enabled {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Agent registration is disabled. Set discovery.registration_enabled = true in cluster.toml"
            })),
        );
    }

    // Check if agent already exists
    if state.agent_pool.get_agent(&body.id).is_some() {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": format!("Agent '{}' already registered", body.id),
                "agent_id": body.id,
            })),
        );
    }

    // Resolve TLS credentials: request body → discovery config → error
    let tls_cert = body.tls_cert
        .or_else(|| state.config.discovery.tls_cert.clone())
        .unwrap_or_default();
    let tls_key = body.tls_key
        .or_else(|| state.config.discovery.tls_key.clone())
        .unwrap_or_default();
    let tls_ca = body.tls_ca
        .or_else(|| state.config.discovery.tls_ca.clone())
        .unwrap_or_default();
    let tls_domain = body.tls_domain
        .unwrap_or_else(|| state.config.discovery.tls_domain.clone());

    if tls_cert.is_empty() || tls_key.is_empty() || tls_ca.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "TLS credentials required. Provide tls_cert/tls_key/tls_ca in request body or configure discovery.tls_cert/tls_key/tls_ca in cluster.toml"
            })),
        );
    }

    let mut labels = body.labels;
    labels.insert("discovery.source".to_string(), "registered".to_string());

    let agent_config = crate::config::AgentConfig {
        id: body.id.clone(),
        name: body.name.clone(),
        address: body.address.clone(),
        tls_cert,
        tls_key,
        tls_ca,
        tls_domain,
        labels,
    };

    match state.agent_pool.add_dynamic_agent(agent_config, crate::agent::AgentSource::Registered).await {
        Ok(_) => {
            info!("Agent registered via API: {} ({})", body.name, body.id);
            (
                StatusCode::CREATED,
                Json(json!({
                    "status": "registered",
                    "agent_id": body.id,
                    "name": body.name,
                    "address": body.address,
                })),
            )
        }
        Err(e) => {
            warn!("Failed to register agent {}: {}", body.id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": format!("Failed to register agent: {}", e),
                    "agent_id": body.id,
                })),
            )
        }
    }
}

/// DELETE /api/agents/:id — deregister a dynamic agent
async fn deregister_agent_handler(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> impl IntoResponse {
    if !state.config.discovery.registration_enabled {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Agent registration API is disabled"
            })),
        );
    }

    // Check if agent exists
    let conn = state.agent_pool.get_agent(&agent_id);
    match conn {
        Some(c) => {
            // Don't allow removing static agents via API
            if c.source == crate::agent::AgentSource::Static {
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "error": "Cannot deregister a static agent. Remove it from cluster.toml instead.",
                        "agent_id": agent_id,
                    })),
                );
            }

            state.agent_pool.remove_agent(&agent_id);
            info!("Agent deregistered via API: {}", agent_id);
            (
                StatusCode::OK,
                Json(json!({
                    "status": "deregistered",
                    "agent_id": agent_id,
                })),
            )
        }
        None => {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": format!("Agent '{}' not found", agent_id),
                })),
            )
        }
    }
}

/// GET /api/agents — list all agents with their source and status
async fn list_agents_handler(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let agents = state.agent_pool.list_agents();
    let mut result = Vec::new();

    for conn in &agents {
        let source = match conn.source {
            crate::agent::AgentSource::Static => "static",
            crate::agent::AgentSource::Discovered => "discovered",
            crate::agent::AgentSource::Registered => "registered",
        };

        result.push(json!({
            "id": conn.info.id,
            "name": conn.info.name,
            "address": conn.info.address,
            "source": source,
            "status": format!("{:?}", conn.health_status()),
            "labels": conn.info.labels,
        }));
    }

    Json(json!({
        "agents": result,
        "total": result.len(),
        "discovery": {
            "swarm_discovery": state.config.discovery.swarm_discovery,
            "registration_enabled": state.config.discovery.registration_enabled,
            "discovery_label": state.config.discovery.discovery_label,
        }
    }))
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
