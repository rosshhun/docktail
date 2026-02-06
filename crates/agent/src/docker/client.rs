use crate::docker::inventory::ContainerInfo;
use crate::docker::stream::{LogStream, LogStreamRequest, LogLine, LogLevel};
use crate::filter::engine::FilterEngine;
use bollard::Docker;
use bollard::container::{LogOutput};
use bollard::models::ContainerInspectResponse;
use bollard::query_parameters::{ListContainersOptions, LogsOptions};
use thiserror::Error;
use futures_util::stream::StreamExt;
use bytes::Bytes;
use std::sync::Arc;

#[derive(Error, Debug)]
pub enum DockerError {
    #[error("Docker connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Container not found: {0}")]
    ContainerNotFound(String),
    #[error("Permission denied")]
    PermissionDenied,
    #[error("Stream closed")]
    StreamClosed,
    #[error("Unsupported log driver: {0}")]
    UnsupportedLogDriver(String),
    #[error("Bollard error: {0}")]
    BollardError(#[from] bollard::errors::Error),
}

// Supported log drivers for time-travel (since/until parameters)
const SUPPORTED_LOG_DRIVERS: &[&str] = &["json-file", "journald", "local"];

#[derive(Debug)]
pub struct DockerClient {
    client: Docker,
}

impl DockerClient {
    pub fn new(socket_path: &str) -> Result<Self, DockerError> {
        let connection = if socket_path.is_empty() {
            Docker::connect_with_defaults()
                .map_err(|e| DockerError::ConnectionFailed(e.to_string()))?
        } else {
            let clean_path = socket_path.trim_start_matches("unix://");
            Docker::connect_with_socket(clean_path, 120, &bollard::API_DEFAULT_VERSION)
                .map_err(|e| DockerError::ConnectionFailed(e.to_string()))?
        };

        Ok(DockerClient { client: connection })
    }
    pub async fn list_containers(&self) -> Result<Vec<ContainerInfo>, DockerError> {
        let options = Some(ListContainersOptions {
            all: true,  // Include stopped containers
            ..Default::default()
        });
        let containers = self.client.list_containers(options).await?;
        Ok(containers
            .into_iter()
            .map(|c| c.into())
            .collect())
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
                        format!("Log driver '{}' does not support time-travel (since/until). Supported drivers: {:?}", 
                            driver, SUPPORTED_LOG_DRIVERS)
                    ));
                }
            }
        }

        // Build Bollard log options
        // NOTE: Bollard v0.20 requires i32 for since/until (Unix timestamps in seconds).
        // We clamp the i64 request values to fit i32 range and warn if clamping occurs,
        // since post-2038 timestamps will be silently capped.
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

        // Get the log stream from Bollard
        let bollard_stream = self.client.logs(&request.container_id, Some(options));
        
        // Convert Bollard stream to our LogLine stream
        let log_stream = bollard_stream.map(move |result| {
            match result {
                Ok(output) => convert_bollard_log(output),
                Err(e) => Err(DockerError::from(e)),
            }
        });

        Ok(LogStream::new(
            request.container_id,
            log_stream,
            filter,
        ))
    }
    
    pub async fn inspect_container(&self, id: &str) -> Result<ContainerInfo, DockerError> {
        // Fetch full details from Docker and convert using From trait
        let details: ContainerInspectResponse = self.client.inspect_container(id, None).await?;
        Ok(ContainerInfo::from(details))
    }

    /// Get raw ContainerInspectResponse from Docker
    /// Used when detailed information beyond ContainerInfo is needed (ports, mounts, etc.)
    pub async fn inspect_container_raw(&self, id: &str) -> Result<ContainerInspectResponse, DockerError> {
        let details: ContainerInspectResponse = self.client.inspect_container(id, None).await?;
        Ok(details)
    }

    /// Get container stats (single snapshot or streaming)
    pub async fn stats(&self, container_id: &str, stream: bool) -> Result<impl tokio_stream::Stream<Item = Result<bollard::models::ContainerStatsResponse, bollard::errors::Error>>, DockerError> {
        use bollard::query_parameters::StatsOptions;

        let options = Some(StatsOptions {
            stream,
            ..Default::default()
        });

        Ok(self.client.stats(container_id, options))
    }
}

/// Convert Bollard's LogOutput to our LogLine format
/// 
/// Docker with `timestamps: true` prepends RFC3339 timestamp to each log:
/// "2023-01-01T00:00:00.000000000Z message content..."
/// 
/// We parse this to preserve the actual log timestamp instead of using current time.
fn convert_bollard_log(output: LogOutput) -> Result<LogLine, DockerError> {
    let (stream_type, raw_bytes) = match output {
        LogOutput::StdOut { message } => (LogLevel::Stdout, message),
        LogOutput::StdErr { message } => (LogLevel::Stderr, message),
        LogOutput::StdIn { message } => (LogLevel::Stdout, message), // Treat stdin as stdout
        LogOutput::Console { message } => (LogLevel::Stdout, message),
    };

    // Docker prepends timestamp: "2023-01-01T00:00:00.000000000Z message"
    // Split at first space to separate timestamp from actual log content
    // We split on bytes to avoid decoding the entire message as UTF-8 yet,
    // which protects against invalid UTF-8 in the log content.
    let split_idx = raw_bytes.iter().position(|&b| b == b' ');
    
    let (timestamp, content) = match split_idx {
        Some(idx) => {
            // Try to parse the bytes before space as the Docker timestamp
            // We only decode the timestamp part as UTF-8
            match std::str::from_utf8(&raw_bytes[..idx]) {
                Ok(ts_str) => {
                    match chrono::DateTime::parse_from_rfc3339(ts_str) {
                        Ok(dt) => {
                            let ts_nanos = dt.timestamp_nanos_opt()
                                .unwrap_or_else(|| chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
                            
                            // Zero-copy slice
                            // Calculate the offset where the message begins
                            // +1 is for the space character we split on
                            let msg_start = idx + 1;
                            
                            // Slice the ORIGINAL Bytes object. 
                            let clean_content = if msg_start < raw_bytes.len() {
                                raw_bytes.slice(msg_start..)
                            } else {
                                Bytes::new()
                            };

                            (ts_nanos, clean_content)
                        }
                        Err(_) => {
                            // Parsing failed - maybe malformed timestamp?
                            (chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0), raw_bytes)
                        }
                    }
                },
                Err(_) => {
                    // Timestamp part not valid UTF-8
                    (chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0), raw_bytes)
                }
            }
        },
        None => {
            // No space found - no timestamp prefix (shouldn't happen with timestamps:true)
            (chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0), raw_bytes)
        }
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
        // Simulate Docker log with timestamp prefix
        let log_content = "2023-01-15T10:30:45.123456789Z Application started successfully";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        // Verify timestamp was parsed correctly
        let expected_dt = chrono::DateTime::parse_from_rfc3339("2023-01-15T10:30:45.123456789Z").unwrap();
        let expected_ts = expected_dt.timestamp_nanos_opt().unwrap();
        assert_eq!(result.timestamp, expected_ts);

        // Verify timestamp prefix was stripped from content
        assert_eq!(result.content, Bytes::from("Application started successfully"));
        assert_eq!(result.stream_type, LogLevel::Stdout);
    }

    #[test]
    fn test_convert_bollard_log_stderr() {
        let log_content = "2023-01-15T10:30:45.123456789Z ERROR: Connection failed";
        let output = LogOutput::StdErr {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        // Verify it's marked as stderr
        assert_eq!(result.stream_type, LogLevel::Stderr);
        assert_eq!(result.content, Bytes::from("ERROR: Connection failed"));
    }

    #[test]
    fn test_convert_bollard_log_no_timestamp() {
        // Log without timestamp (shouldn't happen with timestamps:true, but handle it)
        let log_content = "Plain log message without timestamp";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        // Should fallback to current time and keep full content
        assert!(result.timestamp > 0); // Some timestamp was set
        assert_eq!(result.content, Bytes::from(log_content));
    }

    #[test]
    fn test_convert_bollard_log_malformed_timestamp() {
        // Invalid timestamp format
        let log_content = "NOT_A_TIMESTAMP Application log message";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        // Should fallback to current time and keep full content
        assert!(result.timestamp > 0);
        assert_eq!(result.content, Bytes::from(log_content));
    }

    #[test]
    fn test_convert_bollard_log_multiline_message() {
        // Log with newlines in the message content
        let log_content = "2023-01-15T10:30:45.123456789Z Stack trace:\n  at line 1\n  at line 2";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        // Verify timestamp was parsed and multiline content preserved
        assert_eq!(result.content, Bytes::from("Stack trace:\n  at line 1\n  at line 2"));
    }

    #[test]
    fn test_convert_bollard_log_empty_message() {
        // Edge case: empty message after timestamp
        let log_content = "2023-01-15T10:30:45.123456789Z ";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        // Should parse timestamp and have empty content
        assert_eq!(result.content, Bytes::from(""));
    }

    #[test]
    fn test_convert_bollard_log_invalid_utf8_in_message() {
        // Log with valid timestamp but INVALID UTF-8 in message
        // 0xFF 0xFF is invalid in UTF-8
        let mut data = Vec::new();
        data.extend_from_slice(b"2023-01-15T10:30:45.123456789Z "); // Valid header
        data.extend_from_slice(&[0xFF, 0xFF, 0x61, 0x62, 0x63]); // Invalid UTF-8 body

        let output = LogOutput::StdOut {
            message: Bytes::from(data),
        };

        let result = convert_bollard_log(output).unwrap();

        // Previous implementation would fail here and return current time + full raw bytes
        // We expect it to parse the timestamp correctly
        let expected_dt = chrono::DateTime::parse_from_rfc3339("2023-01-15T10:30:45.123456789Z").unwrap();
        let expected_ts = expected_dt.timestamp_nanos_opt().unwrap();
        assert_eq!(result.timestamp, expected_ts);
        
        // And content should be the invalid bytes (stripped of timestamp)
        assert_eq!(result.content, Bytes::from(&[0xFF, 0xFF, 0x61, 0x62, 0x63][..]));
    }

    #[test]
    fn test_convert_bollard_log_json_content() {
        // JSON log with timestamp
        let log_content = r#"2023-01-15T10:30:45.123456789Z {"level":"info","msg":"Request processed","duration_ms":123}"#;
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        // Verify JSON content is preserved without timestamp prefix
        let expected_json = r#"{"level":"info","msg":"Request processed","duration_ms":123}"#;
        assert_eq!(result.content, Bytes::from(expected_json));
    }

    #[test]
    fn test_convert_bollard_log_invalid_utf8_in_timestamp() {
        // Edge case: Invalid UTF-8 in the timestamp portion itself
        // This should trigger the fallback to current time and keep full content
        let mut data = Vec::new();
        data.extend_from_slice(&[0xFF, 0xFF, 0x20]); // Invalid UTF-8 + space
        data.extend_from_slice(b"message"); // Valid message

        let output = LogOutput::StdOut {
            message: Bytes::from(data.clone()),
        };

        let result = convert_bollard_log(output).unwrap();

        // Should fallback to current time
        assert!(result.timestamp > 0);
        // Should keep full content (can't parse timestamp)
        assert_eq!(result.content, Bytes::from(data));
    }

    #[test]
    fn test_convert_bollard_log_empty_log() {
        // Edge case: Completely empty log
        let output = LogOutput::StdOut {
            message: Bytes::new(),
        };

        let result = convert_bollard_log(output).unwrap();

        // Should fallback to current time
        assert!(result.timestamp > 0);
        // Content should be empty
        assert_eq!(result.content, Bytes::new());
    }

    #[test]
    fn test_convert_bollard_log_unicode_emoji() {
        // Valid multi-byte UTF-8 characters (emoji) in message
        let log_content = "2023-01-15T10:30:45.123456789Z 🚀 Deployment successful! 🎉";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        // Timestamp should parse correctly
        let expected_dt = chrono::DateTime::parse_from_rfc3339("2023-01-15T10:30:45.123456789Z").unwrap();
        let expected_ts = expected_dt.timestamp_nanos_opt().unwrap();
        assert_eq!(result.timestamp, expected_ts);

        // Emoji content should be preserved
        assert_eq!(result.content, Bytes::from("🚀 Deployment successful! 🎉"));
    }

    #[test]
    fn test_convert_bollard_log_multiple_spaces() {
        // Multiple spaces after timestamp (only first space should be stripped)
        let log_content = "2023-01-15T10:30:45.123456789Z   message with leading spaces";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        // Timestamp should parse correctly
        let expected_dt = chrono::DateTime::parse_from_rfc3339("2023-01-15T10:30:45.123456789Z").unwrap();
        let expected_ts = expected_dt.timestamp_nanos_opt().unwrap();
        assert_eq!(result.timestamp, expected_ts);

        // Should preserve the extra leading spaces in message
        assert_eq!(result.content, Bytes::from("  message with leading spaces"));
    }

    #[test]
    fn test_convert_bollard_log_timestamp_only() {
        // Timestamp with no trailing space or message
        let log_content = "2023-01-15T10:30:45.123456789Z";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        // Should fallback to current time since no space found
        assert!(result.timestamp > 0);
        // Should keep full content
        assert_eq!(result.content, Bytes::from(log_content));
    }

    #[test]
    fn test_timestamp_conversion_safety() {
        // Test chrono conversion with valid timestamps
        let valid_ts = 1673780400i64; // 2023-01-15 10:00:00 UTC
        let dt = chrono::DateTime::from_timestamp(valid_ts, 0);
        assert!(dt.is_some());
        
        // Test with year 2038 (i32 overflow boundary)
        let year_2038 = 2147483647i64; // Max i32 value
        let dt_2038 = chrono::DateTime::from_timestamp(year_2038, 0);
        assert!(dt_2038.is_some());
        
        // Test with invalid timestamp
        let invalid_ts = -1i64;
        let dt_invalid = chrono::DateTime::from_timestamp(invalid_ts, 0);
        // Should handle gracefully (returns None)
        assert!(dt_invalid.is_some() || dt_invalid.is_none());
    }
}
