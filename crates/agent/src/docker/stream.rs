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

#[derive(Debug)]
pub struct LogStreamRequest {
    pub container_id: String,
    pub since: Option<i64>,              // Unix timestamp (time-travel start)
    pub until: Option<i64>,              // Unix timestamp (time-travel end)
    pub follow: bool,                    // tail -f mode
    pub filter_pattern: Option<String>,  // Regex pattern for ripgrep
    pub filter_mode: FilterMode,         // Include/Exclude/None
    pub tail_lines: Option<u32>,         // Like "docker logs --tail 100"
}

#[derive(Debug)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::StreamExt;

    fn make_log_line(content: &str, level: LogLevel) -> LogLine {
        LogLine {
            timestamp: 1000,
            stream_type: level,
            content: bytes::Bytes::from(content.to_string()),
        }
    }

    #[tokio::test]
    async fn test_log_stream_no_filter() {
        let lines = vec![
            Ok(make_log_line("hello", LogLevel::Stdout)),
            Ok(make_log_line("world", LogLevel::Stderr)),
        ];
        let inner = tokio_stream::iter(lines);
        let mut stream = LogStream::new("test-container".to_string(), inner, None);

        let first = stream.next().await.unwrap().unwrap();
        assert_eq!(first.container_id.as_ref(), "test-container");
        assert_eq!(first.content, bytes::Bytes::from("hello"));
        assert_eq!(first.sequence, 0);
        assert!(matches!(first.log_level, LogLevel::Stdout));

        let second = stream.next().await.unwrap().unwrap();
        assert_eq!(second.content, bytes::Bytes::from("world"));
        assert_eq!(second.sequence, 1);
        assert!(matches!(second.log_level, LogLevel::Stderr));

        // Stream should end
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn test_log_stream_with_include_filter() {
        let filter = Arc::new(
            crate::filter::engine::FilterEngine::new("error", false, FilterMode::Include).unwrap()
        );

        let lines = vec![
            Ok(make_log_line("info: all good", LogLevel::Stdout)),
            Ok(make_log_line("error: something failed", LogLevel::Stderr)),
            Ok(make_log_line("debug: trace data", LogLevel::Stdout)),
        ];
        let inner = tokio_stream::iter(lines);
        let mut stream = LogStream::new("test".to_string(), inner, Some(filter));

        // Only the "error" line should come through
        let result = stream.next().await.unwrap().unwrap();
        assert_eq!(result.content, bytes::Bytes::from("error: something failed"));
        assert_eq!(result.sequence, 0);

        // Stream should end (other lines were filtered)
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn test_log_stream_with_exclude_filter() {
        let filter = Arc::new(
            crate::filter::engine::FilterEngine::new("healthcheck", false, FilterMode::Exclude).unwrap()
        );

        let lines = vec![
            Ok(make_log_line("GET /api/users 200", LogLevel::Stdout)),
            Ok(make_log_line("healthcheck: ok", LogLevel::Stdout)),
            Ok(make_log_line("POST /api/orders 201", LogLevel::Stdout)),
        ];
        let inner = tokio_stream::iter(lines);
        let mut stream = LogStream::new("test".to_string(), inner, Some(filter));

        let first = stream.next().await.unwrap().unwrap();
        assert_eq!(first.content, bytes::Bytes::from("GET /api/users 200"));

        let second = stream.next().await.unwrap().unwrap();
        assert_eq!(second.content, bytes::Bytes::from("POST /api/orders 201"));

        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn test_log_stream_error_propagation() {
        let lines: Vec<Result<LogLine, DockerError>> = vec![
            Ok(make_log_line("first", LogLevel::Stdout)),
            Err(DockerError::StreamClosed),
        ];
        let inner = tokio_stream::iter(lines);
        let mut stream = LogStream::new("test".to_string(), inner, None);

        let first = stream.next().await.unwrap().unwrap();
        assert_eq!(first.content, bytes::Bytes::from("first"));

        let err = stream.next().await.unwrap().unwrap_err();
        assert!(matches!(err, DockerError::StreamClosed));
    }

    #[tokio::test]
    async fn test_log_stream_empty() {
        let lines: Vec<Result<LogLine, DockerError>> = vec![];
        let inner = tokio_stream::iter(lines);
        let mut stream = LogStream::new("test".to_string(), inner, None);
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn test_log_stream_sequence_monotonic() {
        let lines: Vec<Result<LogLine, DockerError>> = (0..10)
            .map(|i| Ok(make_log_line(&format!("line {}", i), LogLevel::Stdout)))
            .collect();
        let inner = tokio_stream::iter(lines);
        let mut stream = LogStream::new("test".to_string(), inner, None);

        let mut prev_seq = None;
        while let Some(Ok(resp)) = stream.next().await {
            if let Some(prev) = prev_seq {
                assert_eq!(resp.sequence, prev + 1, "Sequence numbers should be monotonically increasing");
            }
            prev_seq = Some(resp.sequence);
        }
        assert_eq!(prev_seq, Some(9));
    }
}