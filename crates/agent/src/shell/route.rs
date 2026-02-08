//! Route — ShellService gRPC handler.

use std::pin::Pin;
use tokio::io::AsyncWriteExt;
use tokio_stream::{Stream, StreamExt};
use tonic::{Request, Response, Status, Streaming};
use tracing::{info, warn, debug};

use crate::state::SharedState;
use crate::shell::map::map_docker_error;

use crate::proto::{
    shell_service_server::ShellService,
    ShellRequest, ShellResponse,
    ShellOutput, ShellExit, ShellError,
    ExecCommandRequest, ExecCommandResponse,
    LogLevel,
};

/// Implementation of the ShellService gRPC service.
/// Provides interactive shell access (bidirectional stream) and
/// one-shot command execution for containers.
pub struct ShellServiceImpl {
    state: SharedState,
}

impl ShellServiceImpl {
    pub fn new(state: SharedState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl ShellService for ShellServiceImpl {
    type OpenShellStream = Pin<Box<dyn Stream<Item = Result<ShellResponse, Status>> + Send>>;

    /// Open an interactive shell in a container.
    ///
    /// Protocol:
    /// 1. Client sends first message as `ShellRequest::Init` with container ID, command, TTY settings
    /// 2. Server creates exec, starts it, and begins streaming output
    /// 3. Client sends `ShellRequest::Input` messages with stdin data
    /// 4. Client sends `ShellRequest::Resize` messages to resize the TTY
    /// 5. Server sends `ShellResponse::Output` with stdout/stderr data
    /// 6. When exec finishes, server sends `ShellResponse::Exit` with exit code
    async fn open_shell(
        &self,
        request: Request<Streaming<ShellRequest>>,
    ) -> Result<Response<Self::OpenShellStream>, Status> {
        let mut in_stream = request.into_inner();

        // Wait for the init message
        let init_msg = in_stream.next().await
            .ok_or_else(|| Status::invalid_argument("No init message received"))?
            .map_err(|e| Status::internal(format!("Failed to receive init: {}", e)))?;

        let init = match init_msg.request {
            Some(crate::proto::shell_request::Request::Init(init)) => init,
            _ => return Err(Status::invalid_argument(
                "First message must be an Init message"
            )),
        };

        let container_id = init.container_id.clone();
        let cmd = if init.command.is_empty() {
            vec!["/bin/sh".to_string()]
        } else {
            init.command
        };
        let tty = init.tty;

        // Convert env map to Docker format: ["KEY=VALUE", ...]
        let env: Vec<String> = init.env.into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        info!(
            container_id = %container_id,
            cmd = ?cmd,
            tty = tty,
            "Opening interactive shell"
        );

        // Create exec instance
        let exec_id = self.state.docker
            .create_exec(&container_id, cmd, tty, init.working_dir, env)
            .await
            .map_err(map_docker_error)?;

        debug!(exec_id = %exec_id, "Exec instance created");

        // Apply initial terminal size if provided
        if let Some(size) = init.terminal_size {
            if let Err(e) = self.state.docker
                .resize_exec(&exec_id, size.rows as u16, size.cols as u16)
                .await
            {
                warn!(exec_id = %exec_id, "Failed to set initial terminal size: {}", e);
            }
        }

        // Start exec in attached mode
        let exec_result = self.state.docker
            .start_exec(&exec_id, tty)
            .await
            .map_err(map_docker_error)?;

        match exec_result {
            bollard::exec::StartExecResults::Attached { output, input } => {
                let docker_state = self.state.clone();
                let exec_id_clone = exec_id.clone();

                // Create a channel to send output back to the client
                let (tx, rx) = tokio::sync::mpsc::channel::<Result<ShellResponse, Status>>(256);

                // Spawn a task to read Docker output and forward to gRPC stream
                let tx_output = tx.clone();
                tokio::spawn(async move {
                    let mut output = output;
                    while let Some(result) = output.next().await {
                        match result {
                            Ok(log_output) => {
                                let (data, stream) = match log_output {
                                    bollard::container::LogOutput::StdOut { message } => {
                                        (message.to_vec(), LogLevel::Stdout as i32)
                                    }
                                    bollard::container::LogOutput::StdErr { message } => {
                                        (message.to_vec(), LogLevel::Stderr as i32)
                                    }
                                    bollard::container::LogOutput::StdIn { message } => {
                                        (message.to_vec(), LogLevel::Stdout as i32)
                                    }
                                    bollard::container::LogOutput::Console { message } => {
                                        (message.to_vec(), LogLevel::Stdout as i32)
                                    }
                                };

                                let response = ShellResponse {
                                    response: Some(crate::proto::shell_response::Response::Output(
                                        ShellOutput { data, stream },
                                    )),
                                };

                                if tx_output.send(Ok(response)).await.is_err() {
                                    break; // Client disconnected
                                }
                            }
                            Err(e) => {
                                warn!(exec_id = %exec_id_clone, "Exec output error: {}", e);
                                let _ = tx_output.send(Ok(ShellResponse {
                                    response: Some(crate::proto::shell_response::Response::Error(
                                        ShellError {
                                            message: format!("Output error: {}", e),
                                            code: "EXEC_OUTPUT_ERROR".to_string(),
                                        },
                                    )),
                                })).await;
                                break;
                            }
                        }
                    }

                    // Exec finished — try to get exit code
                    match docker_state.docker.inspect_exec(&exec_id_clone).await {
                        Ok(inspect) => {
                            let exit_code = inspect.exit_code.unwrap_or(-1) as i32;
                            let _ = tx_output.send(Ok(ShellResponse {
                                response: Some(crate::proto::shell_response::Response::Exit(
                                    ShellExit {
                                        exit_code,
                                        message: if exit_code == 0 {
                                            "Process exited normally".to_string()
                                        } else {
                                            format!("Process exited with code {}", exit_code)
                                        },
                                    },
                                )),
                            })).await;
                        }
                        Err(e) => {
                            warn!(exec_id = %exec_id_clone, "Failed to inspect exec: {}", e);
                            let _ = tx_output.send(Ok(ShellResponse {
                                response: Some(crate::proto::shell_response::Response::Exit(
                                    ShellExit {
                                        exit_code: -1,
                                        message: "Process exited (unable to determine exit code)".to_string(),
                                    },
                                )),
                            })).await;
                        }
                    }
                });

                // Spawn a task to read client input and forward to Docker stdin
                let docker_state2 = self.state.clone();
                let exec_id_for_input = exec_id.clone();
                let container_id_for_input = container_id.clone();
                tokio::spawn(async move {
                    let mut input = input;
                    while let Some(result) = in_stream.next().await {
                        match result {
                            Ok(shell_req) => {
                                match shell_req.request {
                                    Some(crate::proto::shell_request::Request::Input(shell_input)) => {
                                        if let Err(e) = input.write_all(&shell_input.data).await {
                                            warn!(
                                                exec_id = %exec_id_for_input,
                                                "Failed to write to exec stdin: {}",
                                                e
                                            );
                                            break;
                                        }
                                        if let Err(e) = input.flush().await {
                                            warn!(
                                                exec_id = %exec_id_for_input,
                                                "Failed to flush exec stdin: {}",
                                                e
                                            );
                                            break;
                                        }
                                    }
                                    Some(crate::proto::shell_request::Request::Resize(resize)) => {
                                        if let Some(size) = resize.size {
                                            if let Err(e) = docker_state2.docker.resize_exec(
                                                &exec_id_for_input,
                                                size.rows as u16,
                                                size.cols as u16,
                                            ).await {
                                                warn!(
                                                    exec_id = %exec_id_for_input,
                                                    "Failed to resize exec: {}",
                                                    e
                                                );
                                            }
                                        }
                                    }
                                    Some(crate::proto::shell_request::Request::Init(_)) => {
                                        warn!("Received duplicate Init message, ignoring");
                                    }
                                    None => {}
                                }
                            }
                            Err(e) => {
                                debug!(
                                    exec_id = %exec_id_for_input,
                                    "Client stream ended: {}",
                                    e
                                );
                                break;
                            }
                        }
                    }

                    // Client disconnected — kill the exec process to avoid orphans.
                    // Drop stdin first to signal EOF to the process.
                    drop(input);
                    if let Ok(inspect) = docker_state2.docker.inspect_exec(&exec_id_for_input).await {
                        if inspect.running.unwrap_or(false) {
                            warn!(exec_id = %exec_id_for_input, "Client disconnected while exec still running, killing process");
                            if let Some(pid) = inspect.pid {
                                let kill_cmd = vec!["kill".to_string(), "-9".to_string(), pid.to_string()];
                                if let Ok(kill_exec_id) = docker_state2.docker
                                    .create_exec(&container_id_for_input, kill_cmd, false, None, vec![])
                                    .await
                                {
                                    let _ = docker_state2.docker.start_exec(&kill_exec_id, false).await;
                                }
                            }
                        }
                    }
                });

                // Return the output stream
                let output_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
                Ok(Response::new(Box::pin(output_stream) as Self::OpenShellStream))
            }
            bollard::exec::StartExecResults::Detached => {
                Err(Status::internal("Exec started in detached mode unexpectedly"))
            }
        }
    }

    /// Execute a one-shot command in a container (non-interactive).
    /// Returns the command output and exit code.
    async fn exec_command(
        &self,
        request: Request<ExecCommandRequest>,
    ) -> Result<Response<ExecCommandResponse>, Status> {
        let req = request.into_inner();
        let container_id = &req.container_id;
        let cmd = if req.command.is_empty() {
            return Err(Status::invalid_argument("Command must not be empty"));
        } else {
            req.command
        };

        let capture_stdout = req.capture_stdout;
        let capture_stderr = req.capture_stderr;
        let timeout_secs = req.timeout;

        info!(
            container_id = %container_id,
            cmd = ?cmd,
            timeout = ?timeout_secs,
            "Executing one-shot command"
        );

        let start_time = std::time::Instant::now();

        // Convert env map to Docker format
        let env: Vec<String> = req.env.into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        // Create exec instance (no TTY for one-shot commands)
        let exec_id = self.state.docker
            .create_exec(container_id, cmd, false, req.working_dir, env)
            .await
            .map_err(map_docker_error)?;

        // Start exec in attached mode to capture output
        let exec_result = self.state.docker
            .start_exec(&exec_id, false)
            .await
            .map_err(map_docker_error)?;

        match exec_result {
            bollard::exec::StartExecResults::Attached { mut output, .. } => {
                let mut stdout_buf = Vec::new();
                let mut stderr_buf = Vec::new();
                let mut timed_out = false;

                // Collect output with optional timeout
                let collect_future = async {
                    while let Some(result) = output.next().await {
                        match result {
                            Ok(log_output) => match log_output {
                                bollard::container::LogOutput::StdOut { message } => {
                                    if capture_stdout {
                                        stdout_buf.extend_from_slice(&message);
                                    }
                                }
                                bollard::container::LogOutput::StdErr { message } => {
                                    if capture_stderr {
                                        stderr_buf.extend_from_slice(&message);
                                    }
                                }
                                _ => {}
                            },
                            Err(e) => {
                                warn!(exec_id = %exec_id, "Exec output error: {}", e);
                                break;
                            }
                        }
                    }
                };

                if let Some(timeout) = timeout_secs {
                    if timeout > 0 {
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(timeout as u64),
                            collect_future,
                        ).await {
                            Ok(()) => {}
                            Err(_) => {
                                timed_out = true;
                                warn!(exec_id = %exec_id, timeout = timeout, "Exec command timed out");
                                // Drop the output stream to disconnect
                                drop(output);
                                // Kill the still-running exec process inside the container.
                                if let Ok(inspect) = self.state.docker.inspect_exec(&exec_id).await {
                                    if inspect.running.unwrap_or(false) {
                                        if let Some(pid) = inspect.pid {
                                            let kill_cmd = vec!["kill".to_string(), "-9".to_string(), pid.to_string()];
                                            if let Ok(kill_exec_id) = self.state.docker
                                                .create_exec(container_id, kill_cmd, false, None, vec![])
                                                .await
                                            {
                                                let _ = self.state.docker.start_exec(&kill_exec_id, false).await;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        collect_future.await;
                    }
                } else {
                    collect_future.await;
                }

                // Get exit code
                let exit_code = match self.state.docker.inspect_exec(&exec_id).await {
                    Ok(inspect) => inspect.exit_code.unwrap_or(-1) as i32,
                    Err(e) => {
                        warn!(exec_id = %exec_id, "Failed to inspect exec for exit code: {}", e);
                        -1
                    }
                };

                let execution_time_ms = start_time.elapsed().as_millis() as i64;

                info!(
                    container_id = %container_id,
                    exit_code = exit_code,
                    timed_out = timed_out,
                    execution_time_ms = execution_time_ms,
                    "Command execution completed"
                );

                Ok(Response::new(ExecCommandResponse {
                    exit_code,
                    stdout: stdout_buf,
                    stderr: stderr_buf,
                    execution_time_ms,
                    timed_out,
                }))
            }
            bollard::exec::StartExecResults::Detached => {
                Err(Status::internal("Exec started in detached mode unexpectedly"))
            }
        }
    }
}
