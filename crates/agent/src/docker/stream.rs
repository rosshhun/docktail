use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use tokio_stream::Stream;
use crate::docker::client::DockerError;
use crate::filter::engine::{FilterEngine, FilterMode};

// prevents executor starvation during heavy filtering.
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
    pub container_id: Arc<str>,          
    pub timestamp: i64,                  
    pub log_level: LogLevel,             
    pub content: bytes::Bytes,          
    pub sequence: u64,                   
}

pub struct LogLine {
    pub timestamp: i64,
    pub stream_type: LogLevel,
    pub content: bytes::Bytes,
}
pub struct LogStream {
    pub container_id: Arc<str>,  
    pub inner_stream: Pin<Box<dyn Stream<Item = Result<LogLine, DockerError>> + Send>>,
    pub filter: Option<Arc<FilterEngine>>,
    pub sequence_counter: AtomicU64,  
}

impl LogStream {
    pub fn new(
        container_id: String,
        inner_stream: impl Stream<Item = Result<LogLine, DockerError>> + Send + 'static,
        filter: Option<Arc<FilterEngine>>,
    ) -> Self {
        Self {
            container_id: container_id.into(),  
            inner_stream: Box::pin(inner_stream),
            filter,
            sequence_counter: AtomicU64::new(0),
        }
    }
}

impl Stream for LogStream {
    type Item = Result<LogStreamResponse, DockerError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
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
                                container_id: Arc::clone(&this.container_id), 
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