//! WebSocket ↔ gRPC bidirectional shell bridge
//!
//! This module provides an axum WebSocket endpoint that proxies an interactive
//! terminal session between the browser and a container via the agent's
//! `ShellService.OpenShell` gRPC stream.
//!
//! ## Protocol (JSON over WebSocket)
//!
//! **Client → Server:**
//! - `{ "type": "init", "container_id": "...", "agent_id": "...", "command": ["/bin/sh"], "tty": true, "cols": 80, "rows": 24 }`
//! - `{ "type": "input", "data": "<base64-encoded stdin>" }`
//! - `{ "type": "resize", "cols": 120, "rows": 40 }`
//!
//! **Server → Client:**
//! - `{ "type": "output", "data": "<base64-encoded stdout/stderr>" }`
//! - `{ "type": "exit", "exit_code": 0, "message": "..." }`
//! - `{ "type": "error", "message": "...", "code": "..." }`

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::agent::client::{
    OpenShellInit, ShellInput, ShellResize, ShellRequest, TerminalSize,
    shell_request, shell_response,
};
use crate::state::AppState;

// ============================================================================
// WebSocket JSON Protocol Types
// ============================================================================

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WsClientMessage {
    Init {
        container_id: String,
        agent_id: String,
        #[serde(default = "default_command")]
        command: Vec<String>,
        #[serde(default)]
        working_dir: Option<String>,
        #[serde(default = "default_true")]
        tty: bool,
        #[serde(default = "default_cols")]
        cols: u32,
        #[serde(default = "default_rows")]
        rows: u32,
    },
    Input {
        /// Base64-encoded stdin data
        data: String,
    },
    Resize {
        cols: u32,
        rows: u32,
    },
}

fn default_command() -> Vec<String> {
    vec!["/bin/sh".to_string()]
}
fn default_true() -> bool {
    true
}
fn default_cols() -> u32 {
    80
}
fn default_rows() -> u32 {
    24
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WsServerMessage {
    Output {
        /// Base64-encoded stdout/stderr data
        data: String,
    },
    Exit {
        exit_code: i32,
        message: String,
    },
    Error {
        message: String,
        code: String,
    },
}

// ============================================================================
// Query Parameters
// ============================================================================

/// Optional query params for the shell WebSocket endpoint.
/// The actual init is done via the first WebSocket message.
#[derive(Deserialize, Default)]
pub struct ShellQuery {
    /// Kept for potential future use (e.g. auth tokens)
    #[serde(default)]
    _token: Option<String>,
}

// ============================================================================
// WebSocket Handler
// ============================================================================

/// Axum handler — upgrades the HTTP connection to a WebSocket
pub async fn shell_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<crate::state::AppState>,
    Query(_query): Query<ShellQuery>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_shell_socket(socket, state))
}

/// Main WebSocket session loop
async fn handle_shell_socket(socket: WebSocket, state: AppState) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // ── Step 1: Wait for the "init" message ──────────────────────────────
    let init_msg = loop {
        match ws_rx.next().await {
            Some(Ok(Message::Text(text))) => {
                match serde_json::from_str::<WsClientMessage>(&text) {
                    Ok(msg @ WsClientMessage::Init { .. }) => break msg,
                    Ok(_) => {
                        let err = WsServerMessage::Error {
                            message: "First message must be type 'init'".into(),
                            code: "PROTOCOL_ERROR".into(),
                        };
                        let _ = ws_tx
                            .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                            .await;
                        return;
                    }
                    Err(e) => {
                        let err = WsServerMessage::Error {
                            message: format!("Invalid JSON: {}", e),
                            code: "PARSE_ERROR".into(),
                        };
                        let _ = ws_tx
                            .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                            .await;
                        return;
                    }
                }
            }
            Some(Ok(Message::Close(_))) | None => return,
            Some(Ok(_)) => continue, // ignore ping/pong/binary
            Some(Err(e)) => {
                warn!("WebSocket error before init: {}", e);
                return;
            }
        }
    };

    let WsClientMessage::Init {
        container_id,
        agent_id,
        command,
        working_dir,
        tty,
        cols,
        rows,
    } = init_msg
    else {
        unreachable!()
    };

    info!(
        container_id = %container_id,
        agent_id = %agent_id,
        "Shell WebSocket session starting"
    );

    // ── Step 2: Get the gRPC client ──────────────────────────────────────
    let agent = match state.agent_pool.get_agent(&agent_id) {
        Some(a) => a,
        None => {
            let err = WsServerMessage::Error {
                message: format!("Agent not found: {}", agent_id),
                code: "AGENT_NOT_FOUND".into(),
            };
            let _ = ws_tx
                .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                .await;
            return;
        }
    };

    let mut client = {
        let guard = agent.client.lock().await;
        guard.clone()
    };

    // ── Step 3: Build the gRPC request stream via mpsc ───────────────────
    let (grpc_tx, grpc_rx) = mpsc::channel::<ShellRequest>(64);

    // Send init message
    let init_request = ShellRequest {
        request: Some(shell_request::Request::Init(OpenShellInit {
            container_id: container_id.clone(),
            command,
            working_dir,
            env: Default::default(),
            tty,
            terminal_size: Some(TerminalSize { rows, cols }),
        })),
    };
    if grpc_tx.send(init_request).await.is_err() {
        return;
    }

    // Open the bidirectional stream
    let grpc_stream = tokio_stream::wrappers::ReceiverStream::new(grpc_rx);
    let mut response_stream = match client.open_shell(grpc_stream).await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to open shell stream: {}", e);
            let err = WsServerMessage::Error {
                message: format!("Failed to open shell: {}", e),
                code: "GRPC_ERROR".into(),
            };
            let _ = ws_tx
                .send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                .await;
            return;
        }
    };

    // ── Step 4: Bidirectional relay ──────────────────────────────────────
    // Task A: gRPC response → WebSocket client
    let (out_tx, mut out_rx) = mpsc::channel::<WsServerMessage>(128);

    let grpc_to_ws = {
        let out_tx = out_tx.clone();
        tokio::spawn(async move {
            while let Some(result) = response_stream.next().await {
                match result {
                    Ok(resp) => {
                        let ws_msg = match resp.response {
                            Some(shell_response::Response::Output(output)) => {
                                WsServerMessage::Output {
                                    data: BASE64.encode(&output.data),
                                }
                            }
                            Some(shell_response::Response::Exit(exit)) => {
                                let msg = WsServerMessage::Exit {
                                    exit_code: exit.exit_code,
                                    message: exit.message,
                                };
                                let _ = out_tx.send(msg).await;
                                return; // stream done
                            }
                            Some(shell_response::Response::Error(err)) => {
                                WsServerMessage::Error {
                                    message: err.message,
                                    code: err.code,
                                }
                            }
                            None => continue,
                        };
                        if out_tx.send(ws_msg).await.is_err() {
                            return; // WebSocket closed
                        }
                    }
                    Err(e) => {
                        let _ = out_tx
                            .send(WsServerMessage::Error {
                                message: e.to_string(),
                                code: "STREAM_ERROR".into(),
                            })
                            .await;
                        return;
                    }
                }
            }
        })
    };

    // Task B: WebSocket client → gRPC request stream
    let ws_to_grpc = {
        let grpc_tx = grpc_tx.clone();
        tokio::spawn(async move {
            while let Some(result) = ws_rx.next().await {
                match result {
                    Ok(Message::Text(text)) => {
                        match serde_json::from_str::<WsClientMessage>(&text) {
                            Ok(WsClientMessage::Input { data }) => {
                                let bytes = match BASE64.decode(&data) {
                                    Ok(b) => b,
                                    Err(_) => {
                                        // Treat as raw UTF-8 if not valid base64
                                        data.into_bytes()
                                    }
                                };
                                let req = ShellRequest {
                                    request: Some(shell_request::Request::Input(ShellInput {
                                        data: bytes,
                                    })),
                                };
                                if grpc_tx.send(req).await.is_err() {
                                    return;
                                }
                            }
                            Ok(WsClientMessage::Resize { cols, rows }) => {
                                let req = ShellRequest {
                                    request: Some(shell_request::Request::Resize(ShellResize {
                                        size: Some(TerminalSize { rows, cols }),
                                    })),
                                };
                                if grpc_tx.send(req).await.is_err() {
                                    return;
                                }
                            }
                            Ok(WsClientMessage::Init { .. }) => {
                                // Ignore duplicate init
                                debug!("Ignoring duplicate init message");
                            }
                            Err(e) => {
                                debug!("Ignoring unparseable WS message: {}", e);
                            }
                        }
                    }
                    Ok(Message::Binary(bytes)) => {
                        // Treat raw binary frames as stdin input
                        let req = ShellRequest {
                            request: Some(shell_request::Request::Input(ShellInput {
                                data: bytes.to_vec(),
                            })),
                        };
                        if grpc_tx.send(req).await.is_err() {
                            return;
                        }
                    }
                    Ok(Message::Close(_)) | Err(_) => return,
                    Ok(_) => {} // ping/pong handled by axum
                }
            }
        })
    };

    // Task C: Relay outbound messages to the WebSocket sink
    let ws_writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            let text = serde_json::to_string(&msg).unwrap();
            if ws_tx.send(Message::Text(text.into())).await.is_err() {
                return;
            }
        }
    });

    // Wait for any task to finish, then abort the others
    tokio::select! {
        _ = grpc_to_ws => {},
        _ = ws_to_grpc => {},
        _ = ws_writer => {},
    }

    info!(
        container_id = %container_id,
        agent_id = %agent_id,
        "Shell WebSocket session ended"
    );
}
