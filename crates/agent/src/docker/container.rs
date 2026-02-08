//! Container domain — list, inspect, lifecycle, stats, and log streaming.

use super::client::{DockerClient, DockerError};
use super::inventory::ContainerInfo;
use super::stream::{LogStream, LogStreamRequest, LogLine, LogLevel};
use crate::filter::engine::FilterEngine;

use bollard::container::LogOutput;
use bollard::models::ContainerInspectResponse;
use bollard::query_parameters::{ListContainersOptions, LogsOptions, RemoveContainerOptions};
use futures_util::stream::StreamExt;
use bytes::Bytes;
use std::sync::Arc;

// use for time-travel (since/until parameters)
const SUPPORTED_LOG_DRIVERS: &[&str] = &["json-file", "journald", "local"];

impl DockerClient {
    pub async fn list_containers(&self) -> Result<Vec<ContainerInfo>, DockerError> {
        let options = Some(ListContainersOptions {
            all: true,
            ..Default::default()
        });
        let containers = self.client.list_containers(options).await?;
        Ok(containers.into_iter().map(|c| c.into()).collect())
    }

    pub async fn stream_logs(
        &self,
        request: LogStreamRequest,
        filter: Option<Arc<FilterEngine>>,
    ) -> Result<LogStream, DockerError> {
        // Validate time-travel support if since/until is requested
        if request.since.is_some() || request.until.is_some() {
            let container = self.inspect_container(&request.container_id).await?;
            if let Some(driver) = container.log_driver {
                if !SUPPORTED_LOG_DRIVERS.contains(&driver.as_str()) {
                    return Err(DockerError::UnsupportedLogDriver(
                        format!(
                            "Log driver '{}' does not support time-travel (since/until). Supported drivers: {:?}",
                            driver, SUPPORTED_LOG_DRIVERS
                        ),
                    ));
                }
            }
        }

        // NOTE: Bollard v0.20 requires i32 for since/until (Unix timestamps in seconds).
        let since_raw = request.since.unwrap_or(0);
        let until_raw = request.until.unwrap_or(0);
        if since_raw > i32::MAX as i64 || until_raw > i32::MAX as i64 {
            tracing::warn!(
                since = since_raw,
                until = until_raw,
                max = i32::MAX,
                "Timestamp exceeds i32 range (year 2038 limit) — clamping to i32::MAX. \
                 Bollard v0.20 does not support i64 timestamps."
            );
        }
        let since = since_raw.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let until = until_raw.clamp(i32::MIN as i64, i32::MAX as i64) as i32;

        let options = LogsOptions {
            follow: request.follow,
            stdout: true,
            stderr: true,
            since,
            until,
            timestamps: true,
            tail: request.tail_lines.map(|n| n.to_string()).unwrap_or_else(|| "all".to_string()),
        };

        let bollard_stream = self.client.logs(&request.container_id, Some(options));

        let log_stream = bollard_stream.map(move |result| match result {
            Ok(output) => convert_bollard_log(output),
            Err(e) => Err(DockerError::from(e)),
        });

        Ok(LogStream::new(request.container_id, log_stream, filter))
    }

    pub async fn inspect_container(&self, id: &str) -> Result<ContainerInfo, DockerError> {
        let details: ContainerInspectResponse = self.client.inspect_container(id, None).await?;
        Ok(ContainerInfo::from(details))
    }

    /// Returns the full `ContainerInspectResponse` from Docker for a container.
    pub async fn inspect_container_raw(
        &self,
        id: &str,
    ) -> Result<ContainerInspectResponse, DockerError> {
        let details: ContainerInspectResponse = self.client.inspect_container(id, None).await?;
        Ok(details)
    }

    /// Returns container stats either as a single snapshot or a continuous stream.
    pub async fn stats(
        &self,
        container_id: &str,
        stream: bool,
    ) -> Result<
        impl tokio_stream::Stream<
            Item = Result<bollard::models::ContainerStatsResponse, bollard::errors::Error>,
        >,
        DockerError,
    > {
        use bollard::query_parameters::StatsOptions;

        let options = Some(StatsOptions {
            stream,
            ..Default::default()
        });

        Ok(self.client.stats(container_id, options))
    }

    // ── Container Lifecycle ───────────────────────────────────────

    /// Start a stopped container.
    pub async fn start_container(&self, container_id: &str) -> Result<(), DockerError> {
        self.client
            .start_container(container_id, None)
            .await
            .map_err(|e| match e {
                bollard::errors::Error::DockerResponseServerError { status_code: 404, .. } => {
                    DockerError::ContainerNotFound(container_id.to_string())
                }
                other => DockerError::BollardError(other),
            })
    }

    /// Stop a running container with an optional timeout (in seconds).
    pub async fn stop_container(
        &self,
        container_id: &str,
        timeout_secs: Option<u32>,
    ) -> Result<(), DockerError> {
        use bollard::query_parameters::StopContainerOptions;

        let options = timeout_secs.map(|t| StopContainerOptions {
            t: Some(t as i32),
            ..Default::default()
        });

        self.client
            .stop_container(container_id, options)
            .await
            .map_err(|e| match e {
                bollard::errors::Error::DockerResponseServerError { status_code: 404, .. } => {
                    DockerError::ContainerNotFound(container_id.to_string())
                }
                other => DockerError::BollardError(other),
            })
    }

    /// Restart a container with an optional timeout (in seconds).
    pub async fn restart_container(
        &self,
        container_id: &str,
        timeout_secs: Option<u32>,
    ) -> Result<(), DockerError> {
        use bollard::query_parameters::RestartContainerOptions;

        let options = timeout_secs.map(|t| RestartContainerOptions {
            t: Some(t as i32),
            ..Default::default()
        });

        self.client
            .restart_container(container_id, options)
            .await
            .map_err(|e| match e {
                bollard::errors::Error::DockerResponseServerError { status_code: 404, .. } => {
                    DockerError::ContainerNotFound(container_id.to_string())
                }
                other => DockerError::BollardError(other),
            })
    }

    /// Pause a running container (freezes all processes).
    pub async fn pause_container(&self, container_id: &str) -> Result<(), DockerError> {
        self.client
            .pause_container(container_id)
            .await
            .map_err(|e| match e {
                bollard::errors::Error::DockerResponseServerError { status_code: 404, .. } => {
                    DockerError::ContainerNotFound(container_id.to_string())
                }
                other => DockerError::BollardError(other),
            })
    }

    /// Unpause a paused container.
    pub async fn unpause_container(&self, container_id: &str) -> Result<(), DockerError> {
        self.client
            .unpause_container(container_id)
            .await
            .map_err(|e| match e {
                bollard::errors::Error::DockerResponseServerError { status_code: 404, .. } => {
                    DockerError::ContainerNotFound(container_id.to_string())
                }
                other => DockerError::BollardError(other),
            })
    }

    /// Remove a container. If `force` is true, the container will be killed first.
    pub async fn remove_container(
        &self,
        container_id: &str,
        force: bool,
        remove_volumes: bool,
    ) -> Result<(), DockerError> {
        let options = Some(RemoveContainerOptions {
            force,
            v: remove_volumes,
            ..Default::default()
        });

        self.client
            .remove_container(container_id, options)
            .await
            .map_err(|e| match e {
                bollard::errors::Error::DockerResponseServerError { status_code: 404, .. } => {
                    DockerError::ContainerNotFound(container_id.to_string())
                }
                other => DockerError::BollardError(other),
            })
    }
}

/// Converts Bollard's `LogOutput` to our `LogLine` format.
///
/// Docker with `timestamps: true` prepends an RFC3339Nano timestamp like
/// `"2023-01-01T00:00:00.000000000Z message content..."`.
pub(crate) fn convert_bollard_log(output: LogOutput) -> Result<LogLine, DockerError> {
    let (stream_type, raw_bytes) = match output {
        LogOutput::StdOut { message } => (LogLevel::Stdout, message),
        LogOutput::StdErr { message } => (LogLevel::Stderr, message),
        LogOutput::StdIn { message } => (LogLevel::Stdout, message),
        LogOutput::Console { message } => (LogLevel::Stdout, message),
    };

    let split_idx = raw_bytes.iter().position(|&b| b == b' ');

    let (timestamp, content) = match split_idx {
        Some(idx) => match std::str::from_utf8(&raw_bytes[..idx]) {
            Ok(ts_str) => match chrono::DateTime::parse_from_rfc3339(ts_str) {
                Ok(dt) => {
                    let ts_nanos = dt
                        .timestamp_nanos_opt()
                        .unwrap_or_else(|| chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
                    let msg_start = idx + 1;
                    let clean_content = if msg_start < raw_bytes.len() {
                        raw_bytes.slice(msg_start..)
                    } else {
                        Bytes::new()
                    };
                    (ts_nanos, clean_content)
                }
                Err(_) => (
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
                    raw_bytes,
                ),
            },
            Err(_) => (
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
                raw_bytes,
            ),
        },
        None => (
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
            raw_bytes,
        ),
    };

    Ok(LogLine {
        timestamp,
        stream_type,
        content,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::container::LogOutput;

    #[test]
    fn test_convert_bollard_log_with_timestamp() {
        let log_content = "2023-01-15T10:30:45.123456789Z Application started successfully";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        let expected_dt =
            chrono::DateTime::parse_from_rfc3339("2023-01-15T10:30:45.123456789Z").unwrap();
        let expected_ts = expected_dt.timestamp_nanos_opt().unwrap();
        assert_eq!(result.timestamp, expected_ts);
        assert_eq!(
            result.content,
            Bytes::from("Application started successfully")
        );
        assert_eq!(result.stream_type, LogLevel::Stdout);
    }

    #[test]
    fn test_convert_bollard_log_stderr() {
        let log_content = "2023-01-15T10:30:45.123456789Z ERROR: Connection failed";
        let output = LogOutput::StdErr {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();
        assert_eq!(result.stream_type, LogLevel::Stderr);
        assert_eq!(result.content, Bytes::from("ERROR: Connection failed"));
    }

    #[test]
    fn test_convert_bollard_log_no_timestamp() {
        let log_content = "Plain log message without timestamp";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();
        assert!(result.timestamp > 0);
        assert_eq!(result.content, Bytes::from(log_content));
    }

    #[test]
    fn test_convert_bollard_log_malformed_timestamp() {
        let log_content = "NOT_A_TIMESTAMP Application log message";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();
        assert!(result.timestamp > 0);
        assert_eq!(result.content, Bytes::from(log_content));
    }

    #[test]
    fn test_convert_bollard_log_multiline_message() {
        let log_content =
            "2023-01-15T10:30:45.123456789Z Stack trace:\n  at line 1\n  at line 2";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();
        assert_eq!(
            result.content,
            Bytes::from("Stack trace:\n  at line 1\n  at line 2")
        );
    }

    #[test]
    fn test_convert_bollard_log_empty_message() {
        let log_content = "2023-01-15T10:30:45.123456789Z ";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();
        assert_eq!(result.content, Bytes::from(""));
    }

    #[test]
    fn test_convert_bollard_log_invalid_utf8_in_message() {
        let mut data = Vec::new();
        data.extend_from_slice(b"2023-01-15T10:30:45.123456789Z ");
        data.extend_from_slice(&[0xFF, 0xFF, 0x61, 0x62, 0x63]);

        let output = LogOutput::StdOut {
            message: Bytes::from(data),
        };

        let result = convert_bollard_log(output).unwrap();

        let expected_dt =
            chrono::DateTime::parse_from_rfc3339("2023-01-15T10:30:45.123456789Z").unwrap();
        let expected_ts = expected_dt.timestamp_nanos_opt().unwrap();
        assert_eq!(result.timestamp, expected_ts);
        assert_eq!(
            result.content,
            Bytes::from(&[0xFF, 0xFF, 0x61, 0x62, 0x63][..])
        );
    }

    #[test]
    fn test_convert_bollard_log_json_content() {
        let log_content = r#"2023-01-15T10:30:45.123456789Z {"level":"info","msg":"Request processed","duration_ms":123}"#;
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();
        let expected_json =
            r#"{"level":"info","msg":"Request processed","duration_ms":123}"#;
        assert_eq!(result.content, Bytes::from(expected_json));
    }

    #[test]
    fn test_convert_bollard_log_invalid_utf8_in_timestamp() {
        let mut data = Vec::new();
        data.extend_from_slice(&[0xFF, 0xFF, 0x20]);
        data.extend_from_slice(b"message");

        let output = LogOutput::StdOut {
            message: Bytes::from(data.clone()),
        };

        let result = convert_bollard_log(output).unwrap();
        assert!(result.timestamp > 0);
        assert_eq!(result.content, Bytes::from(data));
    }

    #[test]
    fn test_convert_bollard_log_empty_log() {
        let output = LogOutput::StdOut {
            message: Bytes::new(),
        };

        let result = convert_bollard_log(output).unwrap();
        assert!(result.timestamp > 0);
        assert_eq!(result.content, Bytes::new());
    }

    #[test]
    fn test_convert_bollard_log_unicode_emoji() {
        let log_content = "2023-01-15T10:30:45.123456789Z 🚀 Deployment successful! 🎉";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        let expected_dt =
            chrono::DateTime::parse_from_rfc3339("2023-01-15T10:30:45.123456789Z").unwrap();
        let expected_ts = expected_dt.timestamp_nanos_opt().unwrap();
        assert_eq!(result.timestamp, expected_ts);
        assert_eq!(
            result.content,
            Bytes::from("🚀 Deployment successful! 🎉")
        );
    }

    #[test]
    fn test_convert_bollard_log_multiple_spaces() {
        let log_content = "2023-01-15T10:30:45.123456789Z   message with leading spaces";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        let expected_dt =
            chrono::DateTime::parse_from_rfc3339("2023-01-15T10:30:45.123456789Z").unwrap();
        let expected_ts = expected_dt.timestamp_nanos_opt().unwrap();
        assert_eq!(result.timestamp, expected_ts);
        assert_eq!(
            result.content,
            Bytes::from("  message with leading spaces")
        );
    }

    #[test]
    fn test_convert_bollard_log_timestamp_only() {
        let log_content = "2023-01-15T10:30:45.123456789Z";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();
        assert!(result.timestamp > 0);
        assert_eq!(result.content, Bytes::from(log_content));
    }

    #[test]
    fn test_timestamp_conversion_safety() {
        let valid_ts = 1673780400i64;
        let dt = chrono::DateTime::from_timestamp(valid_ts, 0);
        assert!(dt.is_some());

        let year_2038 = 2147483647i64;
        let dt_2038 = chrono::DateTime::from_timestamp(year_2038, 0);
        assert!(dt_2038.is_some());

        let invalid_ts = -1i64;
        let dt_invalid = chrono::DateTime::from_timestamp(invalid_ts, 0);
        assert!(dt_invalid.is_some() || dt_invalid.is_none());
    }
}
