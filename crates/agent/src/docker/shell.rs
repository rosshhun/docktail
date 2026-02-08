//! Shell domain — exec create, start, resize, inspect.

use super::client::{DockerClient, DockerError};

impl DockerClient {
    /// Create an exec instance in a container.
    /// Returns the exec ID that can be used with `start_exec`.
    pub async fn create_exec(
        &self,
        container_id: &str,
        cmd: Vec<String>,
        tty: bool,
        working_dir: Option<String>,
        env: Vec<String>,
    ) -> Result<String, DockerError> {
        use bollard::models::ExecConfig;

        let config = ExecConfig {
            attach_stdin: Some(true),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            tty: Some(tty),
            cmd: Some(cmd),
            env: if env.is_empty() { None } else { Some(env) },
            working_dir,
            ..Default::default()
        };

        let result = self
            .client
            .create_exec(container_id, config)
            .await
            .map_err(|e| match e {
                bollard::errors::Error::DockerResponseServerError {
                    status_code: 404, ..
                } => DockerError::ContainerNotFound(container_id.to_string()),
                other => DockerError::BollardError(other),
            })?;

        Ok(result.id)
    }

    /// Start an exec instance in attached mode.
    pub async fn start_exec(
        &self,
        exec_id: &str,
        tty: bool,
    ) -> Result<bollard::exec::StartExecResults, DockerError> {
        use bollard::exec::StartExecOptions;

        let options = Some(StartExecOptions {
            detach: false,
            tty,
            ..Default::default()
        });

        self.client
            .start_exec(exec_id, options)
            .await
            .map_err(DockerError::from)
    }

    /// Resize the TTY of an exec instance.
    pub async fn resize_exec(
        &self,
        exec_id: &str,
        height: u16,
        width: u16,
    ) -> Result<(), DockerError> {
        use bollard::exec::ResizeExecOptions;

        let options = ResizeExecOptions { height, width };

        self.client
            .resize_exec(exec_id, options)
            .await
            .map_err(DockerError::from)
    }

    /// Inspect an exec instance to get its exit code and running status.
    pub async fn inspect_exec(
        &self,
        exec_id: &str,
    ) -> Result<bollard::models::ExecInspectResponse, DockerError> {
        self.client
            .inspect_exec(exec_id)
            .await
            .map_err(DockerError::from)
    }
}
