use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use tokio_stream::Stream;
use crate::docker::client::DockerError;
use crate::filter::engine::{FilterEngine, FilterMode};

// Cooperative yielding budget: prevents executor starvation during heavy filtering.
// Set high (1024) because filter checks are cheap regex matches on single lines;
// a low budget causes tight yield/reschedule loops when many lines are filtered out.
const POLL_BUDGET: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Stdout = 0,
    Stderr = 1,
}

pub struct LogStreamRequest {
    pub container_id: String,
    pub since: Option<i64>,              // Unix timestamp (time-travel start)
    pub until: Option<i64>,              // Unix timestamp (time-travel end)
    pub follow: bool,                    // tail -f mode
    pub filter_pattern: Option<String>,  // Regex pattern for ripgrep
    pub filter_mode: FilterMode,         // Include/Exclude/None
    pub tail_lines: Option<u32>,         // Like "docker logs --tail 100"
}

pub struct LogStreamResponse {
    pub container_id: Arc<str>,          // Zero-copy reference
    pub timestamp: i64,                  // Unix nanoseconds for precision
    pub log_level: LogLevel,             // Stdout or Stderr
    pub content: bytes::Bytes,           // Using Bytes (not Vec<u8>)
    pub sequence: u64,                   // Ensures ordering, detects gaps
}

pub struct LogLine {
    pub timestamp: i64,
    pub stream_type: LogLevel,
    pub content: bytes::Bytes,  // Memory-efficient
}
pub struct LogStream {
    pub container_id: Arc<str>,  // Arc for zero-cost cloning in high-throughput scenarios
    pub inner_stream: Pin<Box<dyn Stream<Item = Result<LogLine, DockerError>> + Send>>,
    pub filter: Option<Arc<FilterEngine>>,
    pub sequence_counter: AtomicU64,  // For generating sequence numbers
}

impl LogStream {
    pub fn new(
        container_id: String,
        inner_stream: impl Stream<Item = Result<LogLine, DockerError>> + Send + 'static,
        filter: Option<Arc<FilterEngine>>,
    ) -> Self {
        Self {
            container_id: container_id.into(),  // Convert String -> Arc<str> once
            inner_stream: Box::pin(inner_stream),
            filter,
            sequence_counter: AtomicU64::new(0),
        }
    }
}

impl Stream for LogStream {
    type Item = Result<LogStreamResponse, DockerError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Manual budget for cooperative multitasking (stable Rust compatible)
        let mut budget = POLL_BUDGET;

        loop {
            // Yield to executor if budget exhausted to prevent starvation
            if budget == 0 {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            budget -= 1;

            // Safe unpinning: LogStream is Unpin (all fields are Unpin)
            let this = self.as_mut().get_mut();

            match this.inner_stream.as_mut().poll_next(cx) {
                Poll::Ready(Some(result)) => {
                    match result {
                        Ok(line) => {
                            // Apply filter - skip non-matching lines
                            if let Some(filter) = &this.filter {
                                if !filter.should_include(&line.content) {
                                    continue;  // Stack-safe: loop iteration, not recursion
                                }
                            }

                            // Generate sequence number for this matching line
                            let seq = this.sequence_counter.fetch_add(1, Ordering::Relaxed);
                            
                            // Build response (only allocates for matching lines)
                            let response = LogStreamResponse {
                                container_id: Arc::clone(&this.container_id), // Atomic increment only
                                timestamp: line.timestamp,
                                log_level: line.stream_type,
                                content: line.content,
                                sequence: seq,
                            };
                            return Poll::Ready(Some(Ok(response)));
                        }
                        Err(e) => return Poll::Ready(Some(Err(e))),
                    }
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}