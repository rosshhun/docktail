// Log GraphQL types - Phase 4

use async_graphql::{ComplexObject, Context, Enum, InputObject, Result, SimpleObject};
use chrono::{DateTime, Utc};

use crate::graphql::types::container::Container;
use crate::agent::client::{LogLevel as ProtoLogLevel, FilterMode as ProtoFilterMode, ContainerInspectRequest};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Type alias for the container lookup map used by the per-request cache.
type ContainerLookupMap = HashMap<(String, String), Option<Container>>;

/// Per-request cache for container lookups to prevent N+1 gRPC calls.
/// All log entries from the same container share a single gRPC call.
pub struct ContainerLookupCache(pub Arc<Mutex<ContainerLookupMap>>);

impl ContainerLookupCache {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }
}

impl Default for ContainerLookupCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Swarm context — identifies which service/task/node produced a log entry
#[derive(Debug, Clone, SimpleObject)]
pub struct SwarmLogContext {
    /// Service ID
    pub service_id: String,
    /// Service name
    pub service_name: String,
    /// Task ID that produced the log
    pub task_id: String,
    /// Task slot (replica index)
    pub task_slot: u64,
    /// Node ID that ran the task
    pub node_id: String,
}

/// Log entry from a container
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct LogEntry {
    /// Container ID this log belongs to
    pub container_id: String,
    
    /// Agent ID where the container runs
    pub agent_id: String,
    
    /// Timestamp when this log was generated
    pub timestamp: DateTime<Utc>,
    
    /// Log level (stdout or stderr)
    pub level: LogLevel,
    
    /// Raw log content (UTF-8 text)
    pub content: String,
    
    /// Sequence number for ordering and gap detection
    pub sequence: u64,
    
    /// Parsed structured log data (if parsing succeeded)
    pub parsed: Option<ParsedLogData>,
    
    /// Detected log format
    pub format: String,
    
    /// Whether parsing succeeded
    pub parse_success: bool,
    
    /// NEW Phase 4: Multiline grouping
    /// Continuation lines (empty if not grouped)
    pub grouped_lines: Vec<LogLine>,
    
    /// Total lines (1 = single line)
    pub line_count: i32,
    
    /// Quick check for grouped logs
    pub is_grouped: bool,
    
    /// Swarm context (populated for service log entries)
    pub swarm_context: Option<SwarmLogContext>,
}

/// Individual log line within a multiline group
#[derive(Debug, Clone, SimpleObject)]
pub struct LogLine {
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub sequence: u64,
}

#[ComplexObject]
impl LogEntry {
    /// Container object (resolved from state).
    /// Uses a per-request cache so that thousands of log entries from the same
    /// container only trigger a single gRPC inspect call instead of N+1.
    async fn container(&self, ctx: &Context<'_>) -> Result<Option<Container>> {
        use crate::state::AppState;
        
        let cache_key = (self.container_id.clone(), self.agent_id.clone());
        
        // Check per-request cache first
        let cache = ctx.data::<ContainerLookupCache>()?;
        {
            let guard = cache.0.lock().await;
            if let Some(cached) = guard.get(&cache_key) {
                return Ok(cached.clone());
            }
        }
        
        let state = ctx.data::<AppState>()?;
        let agent = state.agent_pool.get_agent(&self.agent_id);
        
        let result = if let Some(agent_conn) = agent {
            // Clone-and-Drop: Lock, Clone, Drop
            let mut client = {
                let guard = agent_conn.client.lock().await;
                guard.clone()
            };
            
            let request = ContainerInspectRequest {
                container_id: self.container_id.clone(),
            };
            
            match client.inspect_container(request).await {
                Ok(response) => {
                    if let Some(info) = response.info {
                        let ports = info.ports.into_iter().map(|p| {
                            crate::graphql::types::container::PortMapping {
                                container_port: p.container_port as i32,
                                protocol: p.protocol,
                                host_ip: p.host_ip,
                                host_port: p.host_port.map(|p| p as i32),
                            }
                        }).collect();
                        
                        let ts = chrono::DateTime::from_timestamp(info.created_at, 0);
                        if ts.is_none() {
                            tracing::warn!(
                                container_id = %self.container_id,
                                created_at = info.created_at,
                                "Invalid created_at timestamp, substituting current time"
                            );
                        }
                        
                        Some(Container {
                            id: info.id,
                            agent_id: self.agent_id.clone(),
                            name: info.name,
                            image: info.image,
                            state: crate::graphql::types::container::ContainerState::from(info.state.as_str()),
                            status: info.status,
                            labels_map: info.labels,
                            created_at: ts.unwrap_or_else(chrono::Utc::now),
                            log_driver: info.log_driver,
                            ports,
                            state_info: info.state_info.map(|si| {
                                crate::graphql::types::container::ContainerStateInfoGql {
                                    oom_killed: si.oom_killed,
                                    pid: si.pid,
                                    exit_code: si.exit_code,
                                    started_at: si.started_at,
                                    finished_at: si.finished_at,
                                    restart_count: si.restart_count,
                                }
                            }),
                        })
                    } else {
                        None
                    }
                }
                Err(_) => None,
            }
        } else {
            None
        };
        
        // Store in cache for subsequent log entries from the same container
        {
            let mut guard = cache.0.lock().await;
            guard.insert(cache_key, result.clone());
        }
        
        Ok(result)
    }
}

/// Log level enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum LogLevel {
    /// Standard output stream
    Stdout,
    /// Standard error stream
    Stderr,
}

/// Container source specifying which container on which agent
#[derive(Debug, Clone, InputObject)]
pub struct ContainerSource {
    /// Container ID
    pub container_id: String,
    
    /// Agent ID where the container is running
    pub agent_id: String,
}

/// Options for streaming or querying logs
#[derive(Debug, Clone, InputObject)]
pub struct LogStreamOptions {
    /// Start time for logs (fetch logs after this timestamp)
    pub since: Option<DateTime<Utc>>,
    
    /// End time for logs (fetch logs before this timestamp)
    pub until: Option<DateTime<Utc>>,
    
    /// Number of lines from the end (like tail -n)
    pub tail: Option<i32>,
    
    /// Follow mode - keep streaming new logs (for subscriptions)
    #[graphql(default = false)]
    pub follow: bool,
    
    /// Filter pattern (regex or substring)
    pub filter: Option<String>,
    
    /// Filter mode (include, exclude, or none)
    #[graphql(default)]
    pub filter_mode: FilterMode,
    
    /// Show timestamps in the output
    #[graphql(default = true)]
    pub timestamps: bool,
}

/// Filter mode for log queries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum, Default)]
pub enum FilterMode {
    /// No filtering - return all logs
    #[default]
    None,
    /// Include only logs matching the pattern
    Include,
    /// Exclude logs matching the pattern
    Exclude,
}

/// Parsed structured log data
#[derive(Debug, Clone, SimpleObject)]
pub struct ParsedLogData {
    /// Extracted log level (info, warn, error, debug)
    pub level: Option<String>,
    
    /// Main log message
    pub message: Option<String>,
    
    /// Logger name (e.g., "app.users")
    pub logger: Option<String>,
    
    /// Application timestamp (if different from Docker timestamp)
    pub timestamp: Option<DateTime<Utc>>,
    
    /// HTTP request context
    pub request: Option<RequestContextData>,
    
    /// Error context
    pub error: Option<ErrorContextData>,
    
    /// Additional key-value fields
    pub fields: Vec<KeyValueField>,
}

/// HTTP request context from parsed logs
#[derive(Debug, Clone, SimpleObject)]
pub struct RequestContextData {
    /// HTTP method (GET, POST, etc.)
    pub method: Option<String>,
    
    /// Request path
    pub path: Option<String>,
    
    /// Client IP address
    pub remote_addr: Option<String>,
    
    /// HTTP status code
    pub status_code: Option<i32>,
    
    /// Request duration in milliseconds
    pub duration_ms: Option<i64>,
    
    /// Request/correlation ID
    pub request_id: Option<String>,
}

/// Error context from parsed logs
#[derive(Debug, Clone, SimpleObject)]
pub struct ErrorContextData {
    /// Exception/error type
    pub error_type: Option<String>,
    
    /// Error message
    pub error_message: Option<String>,
    
    /// Stack trace lines
    pub stack_trace: Vec<String>,
    
    /// Source file
    pub file: Option<String>,
    
    /// Line number
    pub line: Option<i32>,
}

/// Key-value field from parsed logs
#[derive(Debug, Clone, SimpleObject)]
pub struct KeyValueField {
    /// Field name
    pub key: String,
    
    /// Field value
    pub value: String,
}

// Conversion functions from proto to GraphQL types

impl From<ProtoLogLevel> for LogLevel {
    fn from(level: ProtoLogLevel) -> Self {
        match level {
            ProtoLogLevel::Stdout => LogLevel::Stdout,
            ProtoLogLevel::Stderr => LogLevel::Stderr,
            _ => LogLevel::Stdout, // Default to stdout for unspecified
        }
    }
}

impl From<FilterMode> for ProtoFilterMode {
    fn from(mode: FilterMode) -> Self {
        match mode {
            FilterMode::None => ProtoFilterMode::None,
            FilterMode::Include => ProtoFilterMode::Include,
            FilterMode::Exclude => ProtoFilterMode::Exclude,
        }
    }
}

impl LogEntry {
    /// Create a LogEntry from a proto NormalizedLogEntry
    pub fn from_proto(
        response: crate::agent::client::NormalizedLogEntry,
        agent_id: String,
    ) -> Result<Self> {
        // Convert timestamp from nanoseconds to DateTime
        let timestamp = DateTime::from_timestamp(
            response.timestamp_nanos / 1_000_000_000,
            (response.timestamp_nanos % 1_000_000_000) as u32,
        );
        if timestamp.is_none() {
            tracing::warn!(
                container_id = %response.container_id,
                timestamp_nanos = response.timestamp_nanos,
                "Invalid log timestamp_nanos, substituting current time"
            );
        }
        let timestamp = timestamp.unwrap_or_else(Utc::now);
        
        // Convert bytes to UTF-8 string (lossy conversion for invalid UTF-8)
        let content = String::from_utf8_lossy(&response.raw_content).to_string();
        
        // Convert log level
        let level = ProtoLogLevel::try_from(response.log_level)
            .unwrap_or(ProtoLogLevel::Stdout)
            .into();
        
        // Convert parsed data if available
        let parsed = response.parsed.map(|p| ParsedLogData {
            level: p.level,
            message: p.message,
            logger: p.logger,
            // Convert protobuf Timestamp to DateTime<Utc>
            timestamp: p.timestamp.and_then(|ts| {
                DateTime::from_timestamp(ts.seconds, ts.nanos as u32)
            }),
            request: p.request.map(|r| RequestContextData {
                method: r.method,
                path: r.path,
                remote_addr: r.remote_addr,
                status_code: r.status_code,
                duration_ms: r.duration_ms,
                request_id: r.request_id,
            }),
            error: p.error.map(|e| ErrorContextData {
                error_type: e.error_type,
                error_message: e.error_message,
                stack_trace: e.stack_trace,
                file: e.file,
                line: e.line,
            }),
            fields: p.fields.into_iter().map(|f| KeyValueField {
                key: f.key,
                value: f.value,
            }).collect(),
        });
        
        // Extract format and parse success from metadata
        let (format, parse_success) = response.metadata.map(|m| {
            let format_str = match crate::agent::client::LogFormat::try_from(m.detected_format) {
                Ok(crate::agent::client::LogFormat::Json) => "JSON",
                Ok(crate::agent::client::LogFormat::Logfmt) => "Logfmt",
                Ok(crate::agent::client::LogFormat::PlainText) => "PlainText",
                Ok(crate::agent::client::LogFormat::Syslog) => "Syslog",
                Ok(crate::agent::client::LogFormat::HttpLog) => "HttpLog",
                _ => "Unknown",
            };
            (format_str.to_string(), m.parse_success)
        }).unwrap_or_else(|| ("Unknown".to_string(), false));
        
        // Convert grouped lines if present
        let grouped_lines: Vec<LogLine> = response.grouped_lines
            .into_iter()
            .map(|line| {
                let ts = DateTime::from_timestamp(
                    line.timestamp_nanos / 1_000_000_000,
                    (line.timestamp_nanos % 1_000_000_000) as u32,
                );
                if ts.is_none() {
                    tracing::warn!(
                        sequence = line.sequence,
                        timestamp_nanos = line.timestamp_nanos,
                        "Invalid grouped line timestamp_nanos, substituting current time"
                    );
                }
                let timestamp = ts.unwrap_or_else(Utc::now);
                
                let content = String::from_utf8_lossy(&line.content).to_string();
                
                LogLine {
                    content,
                    timestamp,
                    sequence: line.sequence,
                }
            })
            .collect();
        
        Ok(Self {
            container_id: response.container_id,
            agent_id,
            timestamp,
            level,
            content,
            sequence: response.sequence,
            parsed,
            format,
            parse_success,
            grouped_lines,
            line_count: response.line_count as i32,
            is_grouped: response.is_grouped,
            swarm_context: response.swarm_context.map(|sc| SwarmLogContext {
                service_id: sc.service_id,
                service_name: sc.service_name,
                task_id: sc.task_id,
                task_slot: sc.task_slot,
                node_id: sc.node_id,
            }),
        })
    }
}
