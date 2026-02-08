//! Route — LogService gRPC handler.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use tokio_stream::{Stream, StreamExt};
use tonic::{Request, Response, Status};

use crate::docker::client::DockerError;
use crate::docker::stream::LogLine;
use crate::filter::engine::FilterEngine;
use crate::state::SharedState;
use crate::parser::{LogFormat, LogParser, strip_ansi_codes};
use crate::logs::detect;
use crate::logs::map;
use crate::logs::group::MultilineGrouper;

use crate::proto::{
    log_service_server::LogService,
    LogStreamRequest, NormalizedLogEntry,
    ParseMetadata as ProtoParseMetadata,
    LogFormat as ProtoLogFormat,
};

pub struct LogServiceImpl {
    state: SharedState,
}

impl LogServiceImpl {
    pub fn new(state: SharedState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl LogService for LogServiceImpl {
    type StreamLogsStream = Pin<Box<dyn Stream<Item = Result<NormalizedLogEntry, Status>> + Send>>;

    async fn stream_logs(
        &self,
        request: Request<LogStreamRequest>,
    ) -> Result<Response<Self::StreamLogsStream>, Status> {
        let req = request.into_inner();
        let container_id = req.container_id.trim().to_string();
        let disable_parsing = req.disable_parsing;

        if container_id.is_empty() {
            return Err(Status::invalid_argument("container_id must not be empty"));
        }

        // Convert protobuf request to internal request
        let mut req_with_trimmed_id = req.clone();
        req_with_trimmed_id.container_id = container_id.clone();
        let internal_req = map::convert_request(req_with_trimmed_id)
            .map_err(|e| Status::invalid_argument(format!("Invalid request: {}", e)))?;

        // Create filter if pattern is provided AND mode is explicitly Include or Exclude.
        let filter = if let Some(pattern) = &req.filter_pattern {
            if let Some(filter_mode) = map::convert_filter_mode(req.filter_mode) {
                match FilterEngine::new(pattern, false, filter_mode) {
                    Ok(engine) => Some(Arc::new(engine)),
                    Err(e) => {
                        return Err(Status::invalid_argument(format!("Invalid regex pattern: {}", e)));
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        // Get container labels for per-container multiline configuration
        let container_info = self.state.docker
            .inspect_container(&container_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to inspect container: {}", e)))?;

        // Get log stream from Docker client with filter
        let mut log_stream = self.state.docker
            .stream_logs(internal_req, filter.clone())
            .await
            .map_err(|e| match e {
                DockerError::ContainerNotFound(msg) => Status::not_found(msg),
                DockerError::PermissionDenied => Status::permission_denied("Permission denied"),
                DockerError::UnsupportedLogDriver(msg) => Status::failed_precondition(msg),
                _ => Status::internal(format!("Docker error: {}", e)),
            })?;

        // Clone parser_cache and metrics for use in stream
        let parser_cache = Arc::clone(&self.state.parser_cache);
        let metrics = Arc::clone(&self.state.metrics);
        let container_labels = container_info.labels.clone();

        // Create multiline grouper with config from state, applying container overrides
        let container_config = self.state.config.multiline.for_container(
            &container_info.name,
            &container_info.labels
        );
        let mut grouper = if container_config.enabled {
            Some(MultilineGrouper::new(&container_config))
        } else {
            None
        };

        let response_stream = async_stream::stream! {
            let mut format_resolved = false;
            let mut current_format = LogFormat::PlainText;
            let mut current_parser: Option<Box<dyn LogParser>> = None;

            let mut timeout_interval = tokio::time::interval(tokio::time::Duration::from_millis(150));
            timeout_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                let result = tokio::select! {
                    item = log_stream.next() => {
                        match item {
                            Some(r) => r,
                            None => break,
                        }
                    }
                    _ = timeout_interval.tick() => {
                        if let Some(ref mut g) = grouper {
                            while let Some(pending) = g.check_timeout() {
                                yield Ok(pending);
                            }
                        }
                        continue;
                    }
                };

                match result {
                    Ok(log_response) => {
                        let log_line = LogLine {
                            timestamp: log_response.timestamp,
                            stream_type: log_response.log_level,
                            content: log_response.content,
                        };
                        let sequence = log_response.sequence;

                        let cleaned = strip_ansi_codes(&log_line.content);
                        let cleaned_bytes = cleaned.as_ref();

                        // Resolve format on first line (one-time cost)
                        if !format_resolved && !disable_parsing && !parser_cache.is_disabled(&container_id) {
                            current_format = detect::resolve_format(
                                &container_id,
                                &container_labels,
                                &parser_cache,
                                cleaned_bytes,
                                &metrics,
                            );
                            current_parser = Some(map::get_parser(current_format));
                            format_resolved = true;

                            // Structured formats are self-contained per line — skip multiline grouping
                            if matches!(current_format, LogFormat::Json | LogFormat::Logfmt) {
                                if let Some(ref mut g) = grouper {
                                    g.set_passthrough(true);
                                }
                            }
                        }

                        // Parse the log line
                        let (parsed, metadata) = if disable_parsing {
                            (None, ProtoParseMetadata {
                                detected_format: ProtoLogFormat::Unknown as i32,
                                parse_success: false,
                                parse_error: Some("Parsing disabled".to_string()),
                                parse_time_nanos: 0,
                            })
                        } else if parser_cache.is_disabled(&container_id) {
                            (None, ProtoParseMetadata {
                                detected_format: ProtoLogFormat::PlainText as i32,
                                parse_success: false,
                                parse_error: Some("Parsing disabled for container".to_string()),
                                parse_time_nanos: 0,
                            })
                        } else if let Some(parser) = &current_parser {
                            let parse_start = Instant::now();
                            match parser.parse(cleaned_bytes) {
                                Ok(parsed_log) => {
                                    let parse_time = parse_start.elapsed().as_nanos() as u64;
                                    metrics.record_parse(current_format, parse_time);
                                    (
                                        Some(map::convert_parsed_log(parsed_log)),
                                        ProtoParseMetadata {
                                            detected_format: map::convert_log_format(current_format),
                                            parse_success: true,
                                            parse_error: None,
                                            parse_time_nanos: i64::try_from(parse_time).unwrap_or(i64::MAX),
                                        }
                                    )
                                }
                                Err(e) => {
                                    metrics.record_error(crate::parser::metrics::MetricErrorType::Other);
                                    let elapsed_nanos = parse_start.elapsed().as_nanos();
                                    (None, ProtoParseMetadata {
                                        detected_format: map::convert_log_format(current_format),
                                        parse_success: false,
                                        parse_error: Some(e.to_string()),
                                        parse_time_nanos: i64::try_from(elapsed_nanos).unwrap_or(i64::MAX),
                                    })
                                }
                            }
                        } else {
                            (None, ProtoParseMetadata {
                                detected_format: ProtoLogFormat::PlainText as i32,
                                parse_success: false,
                                parse_error: None,
                                parse_time_nanos: 0,
                            })
                        };

                        let entry = NormalizedLogEntry {
                            container_id: container_id.clone(),
                            timestamp_nanos: log_line.timestamp,
                            log_level: map::convert_log_level(log_line.stream_type),
                            sequence,
                            raw_content: cleaned_bytes.to_vec(),
                            parsed,
                            metadata: Some(metadata),
                            grouped_lines: Vec::new(),
                            line_count: 1,
                            is_grouped: false,
                            swarm_context: None,
                        };

                        // Multiline grouping
                        if let Some(ref mut g) = grouper {
                            for grouped in g.process(entry) {
                                yield Ok(grouped);
                            }
                        } else {
                            yield Ok(entry);
                        }
                    }
                    Err(e) => {
                        if let Some(ref mut g) = grouper {
                            while let Some(pending) = g.flush() {
                                yield Ok(pending);
                            }
                        }
                        yield Err(Status::internal(format!("Stream error: {}", e)));
                        break;
                    }
                }
            }

            // Flush any pending multiline group at end of stream
            if let Some(ref mut g) = grouper {
                while let Some(pending) = g.flush() {
                    yield Ok(pending);
                }
            }
        };

        Ok(Response::new(Box::pin(response_stream)))
    }
}
