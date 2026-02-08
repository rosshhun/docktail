//! Multiline log grouper for stack traces and error continuations.
//!
//! Designed for production Docker log streams:
//! - Groups Java/Rust/Go/Python stack traces with their originating error line
//! - Handles timestamped log prefixes (e.g. `2026-02-05T10:00:00Z ERROR ...`)
//! - Supports per-container configuration via Docker labels
//! - Bypasses grouping entirely for structured formats (JSON, Logfmt)
//! - Proactive timeout flushing via `check_timeout()` for idle streams
//! - Safety limits to prevent unbounded memory growth

use crate::config::MultilineConfig;
use crate::proto::NormalizedLogEntry;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::pattern::{has_log_level_prefix, is_continuation_line};

enum GroupAction {
    FlushAndStartNew,
    AddToCurrent,
    StartNew,
}

pub struct MultilineGrouper {
    pending_group: Option<LogGroup>,
    deferred_queue: VecDeque<NormalizedLogEntry>,
    timeout: Duration,
    last_update: Option<Instant>,
    max_lines: usize,
    require_error_anchor: bool,
    passthrough: bool,
}

impl MultilineGrouper {
    pub fn new(config: &MultilineConfig) -> Self {
        Self {
            pending_group: None,
            deferred_queue: VecDeque::new(),
            timeout: Duration::from_millis(config.timeout_ms),
            last_update: None,
            max_lines: config.max_lines,
            require_error_anchor: config.require_error_anchor,
            passthrough: false,
        }
    }

    /// Create a grouper that passes everything through without grouping.
    /// Use for structured log formats (JSON, logfmt) where each line is self-contained.
    pub fn new_passthrough() -> Self {
        Self {
            pending_group: None,
            deferred_queue: VecDeque::new(),
            timeout: Duration::from_millis(0),
            last_update: None,
            max_lines: 0,
            require_error_anchor: false,
            passthrough: true,
        }
    }

    /// Set passthrough mode. When enabled, all entries pass through ungrouped.
    pub fn set_passthrough(&mut self, passthrough: bool) {
        if passthrough && !self.passthrough {
            if let Some(group) = self.pending_group.take() {
                self.last_update = None;
                self.deferred_queue.push_back(group.into_entry());
            }
        }
        self.passthrough = passthrough;
    }

    /// Returns true if the grouper is in passthrough mode.
    pub fn is_passthrough(&self) -> bool {
        self.passthrough
    }

    /// Process a log entry, possibly grouping it with previous lines.
    /// Returns entries that are ready to be emitted.
    pub fn process(&mut self, entry: NormalizedLogEntry) -> Vec<NormalizedLogEntry> {
        if self.passthrough {
            let mut emit = Vec::new();
            while let Some(deferred) = self.deferred_queue.pop_front() {
                emit.push(deferred);
            }
            emit.push(entry);
            return emit;
        }

        let content = &entry.raw_content;

        // Check timeout on arrival of new line
        if let Some(last) = self.last_update {
            if last.elapsed() > self.timeout {
                tracing::debug!(
                    timeout_ms = self.timeout.as_millis() as u64,
                    elapsed_ms = last.elapsed().as_millis() as u64,
                    "multiline: timeout expired, flushing pending group"
                );
                let flushed = self.flush();
                self.start_new_group(entry);
                return flushed.into_iter().collect();
            }
        }

        // Determine action based on current state and new line
        let action = if let Some(ref group) = self.pending_group {
            if has_log_level_prefix(content) {
                tracing::trace!("multiline: new log-level header detected, flushing");
                GroupAction::FlushAndStartNew
            } else if group.continuations.len() >= self.max_lines {
                tracing::debug!(
                    max_lines = self.max_lines,
                    "multiline: max_lines limit reached, flushing"
                );
                GroupAction::FlushAndStartNew
            } else {
                let pattern = is_continuation_line(
                    content,
                    &group.primary.raw_content,
                    group.primary.log_level,
                    self.require_error_anchor,
                );

                if let Some(ref p) = pattern {
                    tracing::trace!(pattern = ?p, "multiline: continuation detected");
                    GroupAction::AddToCurrent
                } else {
                    GroupAction::FlushAndStartNew
                }
            }
        } else {
            GroupAction::StartNew
        };

        match action {
            GroupAction::FlushAndStartNew => {
                let complete = self.flush();
                self.start_new_group(entry);
                complete.into_iter().collect()
            }
            GroupAction::AddToCurrent => {
                if let Some(ref mut group) = self.pending_group {
                    group.add_continuation(entry);
                    self.last_update = Some(Instant::now());
                }
                Vec::new()
            }
            GroupAction::StartNew => {
                self.start_new_group(entry);
                Vec::new()
            }
        }
    }

    /// Convenience wrapper for tests expecting at most one result.
    #[cfg(test)]
    fn process_one(&mut self, entry: NormalizedLogEntry) -> Option<NormalizedLogEntry> {
        let mut results = self.process(entry);
        if results.len() <= 1 {
            results.pop()
        } else {
            panic!("process_one() called but {} entries were returned", results.len());
        }
    }

    /// Proactively check timeout and flush if expired.
    pub fn check_timeout(&mut self) -> Option<NormalizedLogEntry> {
        if let Some(last) = self.last_update {
            if last.elapsed() > self.timeout {
                tracing::debug!(
                    elapsed_ms = last.elapsed().as_millis() as u64,
                    "multiline: proactive timeout flush"
                );
                return self.flush();
            }
        }
        None
    }

    /// Returns true if there is a pending group that hasn't been emitted yet.
    pub fn has_pending(&self) -> bool {
        self.pending_group.is_some() || !self.deferred_queue.is_empty()
    }

    /// Flush pending group (call at stream end or timeout).
    pub fn flush(&mut self) -> Option<NormalizedLogEntry> {
        if let Some(deferred) = self.deferred_queue.pop_front() {
            return Some(deferred);
        }
        if let Some(group) = self.pending_group.take() {
            self.last_update = None;
            Some(group.into_entry())
        } else {
            None
        }
    }

    fn start_new_group(&mut self, entry: NormalizedLogEntry) {
        self.pending_group = Some(LogGroup::new(entry));
        self.last_update = Some(Instant::now());
    }
}

struct LogGroup {
    primary: NormalizedLogEntry,
    continuations: Vec<LogLine>,
}

impl LogGroup {
    fn new(primary: NormalizedLogEntry) -> Self {
        Self {
            primary,
            continuations: Vec::new(),
        }
    }

    fn add_continuation(&mut self, entry: NormalizedLogEntry) {
        self.continuations.push(LogLine {
            content: entry.raw_content,
            timestamp_nanos: entry.timestamp_nanos,
            sequence: entry.sequence,
        });
    }

    fn into_entry(self) -> NormalizedLogEntry {
        let continuation_count = self.continuations.len();
        let line_count = u32::try_from(1 + continuation_count).unwrap_or(u32::MAX);
        let is_grouped = !self.continuations.is_empty();

        let proto_lines: Vec<crate::proto::LogLine> = self.continuations
            .into_iter()
            .map(|line| crate::proto::LogLine {
                content: line.content,
                timestamp_nanos: line.timestamp_nanos,
                sequence: line.sequence,
            })
            .collect();

        NormalizedLogEntry {
            grouped_lines: proto_lines,
            line_count,
            is_grouped,
            container_id: self.primary.container_id,
            timestamp_nanos: self.primary.timestamp_nanos,
            log_level: self.primary.log_level,
            sequence: self.primary.sequence,
            raw_content: self.primary.raw_content,
            parsed: self.primary.parsed,
            metadata: self.primary.metadata,
            swarm_context: self.primary.swarm_context,
        }
    }
}

/// Individual log line within a group.
#[derive(Debug, Clone)]
pub struct LogLine {
    pub content: Vec<u8>,
    pub timestamp_nanos: i64,
    pub sequence: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn create_entry(content: &[u8], level: i32, sequence: u64) -> NormalizedLogEntry {
        NormalizedLogEntry {
            container_id: "test".to_string(),
            timestamp_nanos: 1_000_000_000,
            log_level: level,
            sequence,
            raw_content: content.to_vec(),
            parsed: None,
            metadata: None,
            grouped_lines: Vec::new(),
            line_count: 1,
            is_grouped: false,
            swarm_context: None,
        }
    }

    fn default_test_config() -> MultilineConfig {
        MultilineConfig {
            enabled: true,
            timeout_ms: 300,
            max_lines: 50,
            require_error_anchor: true,
            container_overrides: std::collections::HashMap::new(),
        }
    }

    // ─── Basic grouping ─────────────────────────────────────────

    #[test]
    fn test_stack_trace_grouping() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR panic at main.rs:10", 5, 1);
        let line2 = create_entry(b"    at std::panic::catch_unwind", 0, 2);
        let line3 = create_entry(b"    at tokio::runtime::block_on", 0, 3);

        assert!(grouper.process_one(line1).is_none());
        assert!(grouper.process_one(line2).is_none());
        assert!(grouper.process_one(line3).is_none());

        let grouped = grouper.flush().unwrap();
        assert_eq!(grouped.line_count, 3);
        assert_eq!(grouped.grouped_lines.len(), 2);
        assert!(grouped.is_grouped);

        assert_eq!(grouped.raw_content, b"ERROR panic at main.rs:10");
        assert_eq!(
            grouped.grouped_lines[0].content,
            b"    at std::panic::catch_unwind"
        );
    }

    #[test]
    fn test_single_line_not_grouped() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"INFO request completed", 3, 1);
        let line2 = create_entry(b"INFO another request", 3, 2);

        assert!(grouper.process_one(line1).is_none());
        let flushed = grouper.process_one(line2).unwrap();

        assert!(!flushed.is_grouped);
        assert_eq!(flushed.line_count, 1);
        assert_eq!(flushed.raw_content, b"INFO request completed");
    }

    // ─── Timeout handling ───────────────────────────────────────

    #[test]
    fn test_timeout_flush_on_next_line() {
        let mut config = default_test_config();
        config.timeout_ms = 50;
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR start", 5, 1);
        grouper.process_one(line1);

        std::thread::sleep(Duration::from_millis(100));

        let line2 = create_entry(b"INFO new log", 3, 2);
        let flushed = grouper.process_one(line2).unwrap();

        assert_eq!(flushed.raw_content, b"ERROR start");
        assert!(!flushed.is_grouped);
    }

    #[test]
    fn test_proactive_timeout_check() {
        let mut config = default_test_config();
        config.timeout_ms = 50;
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR panic at main.rs", 5, 1);
        let line2 = create_entry(b"    at std::panic::catch", 0, 2);

        grouper.process_one(line1);
        grouper.process_one(line2);

        assert!(grouper.check_timeout().is_none());
        assert!(grouper.has_pending());

        std::thread::sleep(Duration::from_millis(100));

        let flushed = grouper.check_timeout().unwrap();
        assert!(flushed.is_grouped);
        assert_eq!(flushed.line_count, 2);
        assert!(!grouper.has_pending());
    }

    #[test]
    fn test_check_timeout_no_pending() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);
        assert!(grouper.check_timeout().is_none());
    }

    // ─── Continuation patterns ──────────────────────────────────

    #[test]
    fn test_java_stack_trace() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let lines = vec![
            create_entry(
                b"ERROR Exception in thread main java.lang.NullPointerException",
                5, 1,
            ),
            create_entry(b"\tat com.example.App.main(App.java:15)", 0, 2),
            create_entry(b"\tat com.example.Util.run(Util.java:42)", 0, 3),
            create_entry(b"Caused by: java.io.IOException: file not found", 0, 4),
            create_entry(b"\tat java.io.FileInputStream.open(Native Method)", 0, 5),
        ];

        for line in lines {
            grouper.process_one(line);
        }

        let grouped = grouper.flush().unwrap();
        assert_eq!(grouped.line_count, 5);
        assert!(grouped.is_grouped);
    }

    #[test]
    fn test_python_traceback() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR Unhandled exception", 5, 1);
        let line2 = create_entry(b"Traceback (most recent call last):", 0, 2);
        let line3 = create_entry(b"  File \"/app/main.py\", line 42, in run", 0, 3);
        let line4 = create_entry(b"    raise ValueError(\"bad input\")", 0, 4);

        grouper.process_one(line1);
        grouper.process_one(line2);
        grouper.process_one(line3);
        grouper.process_one(line4);

        let grouped = grouper.flush().unwrap();
        assert_eq!(grouped.line_count, 4);
        assert!(grouped.is_grouped);
    }

    #[test]
    fn test_rust_backtrace() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR thread 'main' panicked at 'index out of bounds'", 5, 1);
        let line2 = create_entry(b"   0: std::panicking::begin_panic", 0, 2);
        let line3 = create_entry(b"   1: myapp::process", 0, 3);
        let line4 = create_entry(b"   2: myapp::main", 0, 4);

        grouper.process_one(line1);
        grouper.process_one(line2);
        grouper.process_one(line3);
        grouper.process_one(line4);

        let grouped = grouper.flush().unwrap();
        assert_eq!(grouped.line_count, 4);
        assert!(grouped.is_grouped);
    }

    #[test]
    fn test_dotnet_stack_trace() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR System.InvalidOperationException: failed", 5, 1);
        let line2 = create_entry(b"   at System.Collections.List.Add(Object item)", 0, 2);
        let line3 = create_entry(b"   --- End of stack trace ---", 0, 3);
        let line4 = create_entry(b"   at Microsoft.AspNetCore.Hosting.Start()", 0, 4);

        grouper.process_one(line1);
        grouper.process_one(line2);
        grouper.process_one(line3);
        grouper.process_one(line4);

        let grouped = grouper.flush().unwrap();
        assert_eq!(grouped.line_count, 4);
        assert!(grouped.is_grouped);
    }

    #[test]
    fn test_caused_by_chain() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR connection failed", 5, 1);
        let line2 = create_entry(b"Caused by: java.net.ConnectException: refused", 0, 2);
        let line3 = create_entry(b"Caused by: java.io.IOException: broken pipe", 0, 3);

        grouper.process_one(line1);
        grouper.process_one(line2);
        grouper.process_one(line3);

        let grouped = grouper.flush().unwrap();
        assert_eq!(grouped.line_count, 3);
        assert!(grouped.is_grouped);
    }

    #[test]
    fn test_suppressed_exception() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR main exception", 5, 1);
        let line2 = create_entry(b"Suppressed: java.io.IOException", 0, 2);
        let line3 = create_entry(b"\tat cleanup(Resource.java:55)", 0, 3);

        grouper.process_one(line1);
        grouper.process_one(line2);
        grouper.process_one(line3);

        let grouped = grouper.flush().unwrap();
        assert_eq!(grouped.line_count, 3);
        assert!(grouped.is_grouped);
    }

    #[test]
    fn test_continue_tokens() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR query failed", 5, 1);
        let line2 = create_entry(b"... 5 more", 0, 2);

        grouper.process_one(line1);
        grouper.process_one(line2);

        let grouped = grouper.flush().unwrap();
        assert!(grouped.is_grouped);
    }

    #[test]
    fn test_unicode_tree_chars() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR dependency tree", 5, 1);
        let line2 = create_entry("├── child1".as_bytes(), 0, 2);
        let line3 = create_entry("└── child2".as_bytes(), 0, 3);

        grouper.process_one(line1);
        grouper.process_one(line2);
        grouper.process_one(line3);

        let grouped = grouper.flush().unwrap();
        assert!(grouped.is_grouped);
        assert_eq!(grouped.line_count, 3);
    }

    // ─── Error anchor behavior ──────────────────────────────────

    #[test]
    fn test_dont_group_yaml_in_conservative_mode() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"INFO config:", 3, 1);
        let line2 = create_entry(b"    database:", 0, 2);

        grouper.process_one(line1);
        let flushed = grouper.process_one(line2).unwrap();

        assert!(!flushed.is_grouped);
    }

    #[test]
    fn test_group_indented_after_error() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR validation failed", 5, 1);
        let line2 = create_entry(b"    field: username", 0, 2);
        let line3 = create_entry(b"    reason: too short", 0, 3);

        grouper.process_one(line1);
        grouper.process_one(line2);
        grouper.process_one(line3);

        let grouped = grouper.flush().unwrap();
        assert!(grouped.is_grouped);
        assert_eq!(grouped.line_count, 3);
    }

    #[test]
    fn test_aggressive_mode_groups_any_indent() {
        let mut config = default_test_config();
        config.require_error_anchor = false;
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"INFO config:", 3, 1);
        let line2 = create_entry(b"    database: postgres", 0, 2);

        grouper.process_one(line1);
        grouper.process_one(line2);

        let grouped = grouper.flush().unwrap();
        assert!(grouped.is_grouped);
    }

    // ─── New log level flushes ──────────────────────────────────

    #[test]
    fn test_new_level_flushes() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR panic", 5, 1);
        let line2 = create_entry(b"    at main", 0, 2);
        let line3 = create_entry(b"WARN different log", 4, 3);

        grouper.process_one(line1);
        grouper.process_one(line2);

        let flushed = grouper.process_one(line3).unwrap();
        assert_eq!(flushed.line_count, 2);
        assert!(flushed.is_grouped);
    }

    #[test]
    fn test_timestamped_level_flushes_group() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"2026-02-05T10:00:00Z ERROR panic happened", 5, 1);
        let line2 = create_entry(b"    at main::run", 0, 2);
        let line3 = create_entry(b"2026-02-05T10:00:01Z INFO recovered", 3, 3);

        grouper.process_one(line1);
        grouper.process_one(line2);

        let flushed = grouper.process_one(line3).unwrap();
        assert!(flushed.is_grouped);
        assert_eq!(flushed.line_count, 2);
    }

    // ─── Passthrough / JSON bypass ──────────────────────────────

    #[test]
    fn test_passthrough_mode() {
        let mut grouper = MultilineGrouper::new_passthrough();

        let line1 = create_entry(b"{\"level\":\"error\",\"msg\":\"oops\"}", 5, 1);
        let line2 = create_entry(b"{\"level\":\"info\",\"msg\":\"ok\"}", 3, 2);

        let result1 = grouper.process_one(line1).unwrap();
        let result2 = grouper.process_one(line2).unwrap();

        assert!(!result1.is_grouped);
        assert!(!result2.is_grouped);
        assert!(!grouper.has_pending());
    }

    #[test]
    fn test_set_passthrough_flushes_pending() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR panic", 5, 1);
        let line2 = create_entry(b"    at main", 0, 2);

        grouper.process_one(line1);
        grouper.process_one(line2);

        assert!(grouper.has_pending());

        grouper.set_passthrough(true);
        assert!(grouper.has_pending());

        let line3 = create_entry(b"{\"level\":\"info\"}", 3, 3);
        let results = grouper.process(line3);
        assert_eq!(results.len(), 2);
        assert!(results[0].is_grouped);
        assert_eq!(results[0].raw_content, b"ERROR panic");
        assert!(!results[1].is_grouped);
        assert_eq!(results[1].raw_content, b"{\"level\":\"info\"}");

        let line4 = create_entry(b"{\"level\":\"warn\"}", 4, 4);
        let results4 = grouper.process(line4);
        assert_eq!(results4.len(), 1);
        assert_eq!(results4[0].raw_content, b"{\"level\":\"warn\"}");

        assert!(!grouper.has_pending());
        assert!(grouper.flush().is_none());
    }

    #[test]
    fn test_is_passthrough() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);
        assert!(!grouper.is_passthrough());

        grouper.set_passthrough(true);
        assert!(grouper.is_passthrough());
    }

    // ─── Max lines limit ────────────────────────────────────────

    #[test]
    fn test_max_lines_limit() {
        let mut config = default_test_config();
        config.max_lines = 3;
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR start", 5, 1);
        let line2 = create_entry(b"    continuation 1", 0, 2);
        let line3 = create_entry(b"    continuation 2", 0, 3);
        let line4 = create_entry(b"    continuation 3", 0, 4);
        let line5 = create_entry(b"    continuation 4", 0, 5);

        grouper.process_one(line1);
        grouper.process_one(line2);
        grouper.process_one(line3);
        grouper.process_one(line4);

        let flushed = grouper.process_one(line5).unwrap();
        assert_eq!(flushed.line_count, 4);
    }

    #[test]
    fn test_max_lines_one() {
        let mut config = default_test_config();
        config.max_lines = 1;
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR start", 5, 1);
        let line2 = create_entry(b"    at main", 0, 2);
        let line3 = create_entry(b"    at lib", 0, 3);

        grouper.process_one(line1);
        grouper.process_one(line2);

        let flushed = grouper.process_one(line3).unwrap();
        assert_eq!(flushed.line_count, 2);
    }

    // ─── Edge cases ─────────────────────────────────────────────

    #[test]
    fn test_empty_content() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR something", 5, 1);
        let line2 = create_entry(b"", 0, 2);

        grouper.process_one(line1);
        let flushed = grouper.process_one(line2).unwrap();

        assert!(!flushed.is_grouped);
        assert_eq!(flushed.raw_content, b"ERROR something");
    }

    #[test]
    fn test_whitespace_only_content() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR something", 5, 1);
        let line2 = create_entry(b"    ", 0, 2);

        grouper.process_one(line1);
        grouper.process_one(line2);

        let grouped = grouper.flush().unwrap();
        assert!(grouped.is_grouped);
    }

    #[test]
    fn test_very_long_line() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let long_content = vec![b'A'; 100_000];
        let line1 = create_entry(&long_content, 3, 1);
        let line2 = create_entry(b"INFO next", 3, 2);

        grouper.process_one(line1);
        let flushed = grouper.process_one(line2).unwrap();

        assert!(!flushed.is_grouped);
        assert_eq!(flushed.raw_content.len(), 100_000);
    }

    #[test]
    fn test_flush_empty_grouper() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);
        assert!(grouper.flush().is_none());
    }

    #[test]
    fn test_multiple_groups_in_sequence() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR first panic", 5, 1);
        let line2 = create_entry(b"    at main::run", 0, 2);
        let line3 = create_entry(b"ERROR second panic", 5, 3);
        let line4 = create_entry(b"    at lib::process", 0, 4);

        grouper.process_one(line1);
        grouper.process_one(line2);

        let first = grouper.process_one(line3).unwrap();
        assert!(first.is_grouped);
        assert_eq!(first.raw_content, b"ERROR first panic");
        assert_eq!(first.line_count, 2);

        grouper.process_one(line4);

        let second = grouper.flush().unwrap();
        assert!(second.is_grouped);
        assert_eq!(second.raw_content, b"ERROR second panic");
        assert_eq!(second.line_count, 2);
    }

    #[test]
    fn test_unindented_at_does_not_group() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"Log message", 3, 1);
        let line2 = create_entry(b"at the moment", 0, 2);

        grouper.process_one(line1);
        let flushed = grouper.process_one(line2).unwrap();

        assert_eq!(flushed.raw_content, b"Log message");
        assert!(!flushed.is_grouped);
    }

    #[test]
    fn test_tab_indented_at_groups() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR exception occurred", 5, 1);
        let line2 = create_entry(b"\tat com.example.Main.run(Main.java:42)", 0, 2);

        grouper.process_one(line1);
        grouper.process_one(line2);

        let grouped = grouper.flush().unwrap();
        assert!(grouped.is_grouped);
    }

    #[test]
    fn test_sequence_preserved() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR start", 5, 100);
        let line2 = create_entry(b"    at foo", 0, 101);
        let line3 = create_entry(b"    at bar", 0, 102);

        grouper.process_one(line1);
        grouper.process_one(line2);
        grouper.process_one(line3);

        let grouped = grouper.flush().unwrap();
        assert_eq!(grouped.sequence, 100);
        assert_eq!(grouped.grouped_lines[0].sequence, 101);
        assert_eq!(grouped.grouped_lines[1].sequence, 102);
    }

    #[test]
    fn test_container_id_preserved() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let mut entry = create_entry(b"ERROR start", 5, 1);
        entry.container_id = "abc123".to_string();

        grouper.process_one(entry);
        let flushed = grouper.flush().unwrap();
        assert_eq!(flushed.container_id, "abc123");
    }

    // ─── Interaction: timestamp prefix + grouping ────────────────

    #[test]
    fn test_timestamped_lines_group_correctly() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"2026-02-05T10:00:00Z ERROR NullPointerException", 5, 1);
        let line2 = create_entry(b"\tat com.example.App.run(App.java:42)", 0, 2);
        let line3 = create_entry(b"\tat com.example.Main.main(Main.java:10)", 0, 3);
        let line4 = create_entry(b"2026-02-05T10:00:01Z INFO Server started", 3, 4);

        grouper.process_one(line1);
        grouper.process_one(line2);
        grouper.process_one(line3);

        let flushed = grouper.process_one(line4).unwrap();
        assert!(flushed.is_grouped);
        assert_eq!(flushed.line_count, 3);
        assert_eq!(
            flushed.raw_content,
            b"2026-02-05T10:00:00Z ERROR NullPointerException"
        );
    }

    // ─── Regression: no false grouping of unrelated lines ───────

    #[test]
    fn test_normal_info_lines_not_grouped() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let entries: Vec<_> = (1..=5)
            .map(|i| {
                create_entry(
                    format!("INFO request {} completed in 42ms", i).as_bytes(),
                    3,
                    i,
                )
            })
            .collect();

        let mut outputs = Vec::new();
        for entry in entries {
            if let Some(out) = grouper.process_one(entry) {
                outputs.push(out);
            }
        }
        if let Some(out) = grouper.flush() {
            outputs.push(out);
        }

        assert_eq!(outputs.len(), 5);
        for out in &outputs {
            assert!(!out.is_grouped);
            assert_eq!(out.line_count, 1);
        }
    }

    #[test]
    fn test_alternating_levels_each_separate() {
        let config = default_test_config();
        let mut grouper = MultilineGrouper::new(&config);

        let line1 = create_entry(b"ERROR first", 5, 1);
        let line2 = create_entry(b"WARN second", 4, 2);
        let line3 = create_entry(b"INFO third", 3, 3);

        grouper.process_one(line1);
        let f1 = grouper.process_one(line2).unwrap();
        let f2 = grouper.process_one(line3).unwrap();
        let f3 = grouper.flush().unwrap();

        assert!(!f1.is_grouped);
        assert!(!f2.is_grouped);
        assert!(!f3.is_grouped);
    }
}
