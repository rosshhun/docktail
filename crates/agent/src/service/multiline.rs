use super::proto::NormalizedLogEntry;
use crate::config::MultilineConfig;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

enum GroupAction {
    FlushAndStartNew,
    AddToCurrent,
    StartNew,
}

/// Multiline log grouper for stack traces and error continuations.
///
/// Designed for production Docker log streams:
/// - Groups Java/Rust/Go/Python stack traces with their originating error line
/// - Handles timestamped log prefixes (e.g. `2026-02-05T10:00:00Z ERROR ...`)
/// - Supports per-container configuration via Docker labels
/// - Bypasses grouping entirely for structured formats (JSON, Logfmt)
/// - Proactive timeout flushing via `check_timeout()` for idle streams
/// - Safety limits to prevent unbounded memory growth
pub struct MultilineGrouper {
    pending_group: Option<LogGroup>,
    /// Entries waiting to be emitted (from set_passthrough flush).
    /// Uses a queue so that both deferred and pending-group entries are preserved.
    deferred_queue: VecDeque<NormalizedLogEntry>,
    timeout: Duration,
    last_update: Option<Instant>,
    max_lines: usize,
    require_error_anchor: bool,
    /// When true, all entries pass through ungrouped (JSON, logfmt, etc.)
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
    /// Useful when format detection resolves to JSON/logfmt after construction.
    pub fn set_passthrough(&mut self, passthrough: bool) {
        if passthrough && !self.passthrough {
            // Switching to passthrough: drain ALL pending state into the queue
            // so nothing is lost. Both pending_group and any prior deferred
            // entries are preserved in order.
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
    /// Returns entries that are ready to be emitted.  In passthrough mode this
    /// can be multiple entries (transition drain + the new entry).  In grouping
    /// mode it is at most one.
    ///
    /// Uses `entry.raw_content` for all pattern matching — no separate content
    /// parameter to prevent data mismatch.
    pub fn process(&mut self, entry: NormalizedLogEntry) -> Vec<NormalizedLogEntry> {
        // Passthrough mode: drain queued transition entries, then pass through
        // immediately with zero latency.
        //
        // We collect entries into `emit` so the caller can yield all of them,
        // eliminating the one-behind latency the old single-Option API caused.
        if self.passthrough {
            let mut emit = Vec::new();
            // Drain all transition entries
            while let Some(deferred) = self.deferred_queue.pop_front() {
                emit.push(deferred);
            }
            // The current entry passes through immediately
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

    /// Convenience wrapper: process an entry expecting at most one result.
    /// Use in grouping mode (non-passthrough) tests where the return is 0 or 1 entries.
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
    /// Call this periodically (e.g. every 100ms) from an interval timer
    /// to ensure the last group is emitted even when the container goes quiet.
    ///
    /// Returns `Some(entry)` if a pending group was flushed.
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
        // Deferred entries (from set_passthrough transition) take priority
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
        // Safe u32 conversion — cap at u32::MAX instead of silent wrapping
        let line_count = u32::try_from(1 + continuation_count).unwrap_or(u32::MAX);
        let is_grouped = !self.continuations.is_empty();
        
        // Convert LogLine to proto LogLine
        let proto_lines: Vec<super::proto::LogLine> = self.continuations
            .into_iter()
            .map(|line| super::proto::LogLine {
                content: line.content,
                timestamp_nanos: line.timestamp_nanos,
                sequence: line.sequence,
            })
            .collect();
        
        // Return primary with continuations attached (structure preserved)
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

/// Individual log line within a group
#[derive(Debug, Clone)]
pub struct LogLine {
    pub content: Vec<u8>,
    pub timestamp_nanos: i64,
    pub sequence: u64,
}

#[derive(Debug)]
enum ContinuationPattern {
    StackFrame,
    ErrorIndentation,
    ContinueToken,
}

/// Detect if current line is a continuation of the previous primary line.
fn is_continuation_line(
    current: &[u8],
    previous: &[u8],
    previous_level: i32,
    require_error_anchor: bool,
) -> Option<ContinuationPattern> {

    if current.is_empty() {
        return None;
    }

    if starts_with_any(current, &[b"   at ", b"\tat ", b"\t at "]) {
        return Some(ContinuationPattern::StackFrame);
    }

    if starts_with_any(current, &[b"Caused by:", b"caused by:", b"due to:", b"Suppressed:"]) {
        return Some(ContinuationPattern::StackFrame);
    }

    if starts_with_any(current, &[b"  File \"", b"    raise ", b"Traceback "]) {
        return Some(ContinuationPattern::StackFrame);
    }

    if starts_with_any(current, &[b"goroutine ", b"\tgoroutine "]) {
        if contains_any(previous, &[b"panic", b"runtime error"]) {
            return Some(ContinuationPattern::StackFrame);
        }
    }

    if starts_with_any(current, &[b"   --- ", b"   at System.", b"   at Microsoft."]) {
        return Some(ContinuationPattern::StackFrame);
    }

    if current.len() > 6 && current[0..3] == *b"   " {
        let rest = &current[3..];
        if rest.len() >= 3
            && rest[0].is_ascii_digit()
            && (rest[1] == b':' || (rest[1].is_ascii_digit() && rest[2] == b':'))
        {
            return Some(ContinuationPattern::StackFrame);
        }
    }


    let is_indented = current.starts_with(b"    ") || current.starts_with(b"\t");
    if is_indented {
        if require_error_anchor {

            let is_error_anchor = previous_level >= 4
                || contains_any(
                    previous,
                    &[
                        b"panic", b"ERROR", b"Exception", b"exception",
                        b"error:", b"FATAL", b"fatal", b"PANIC",
                        b"Traceback", b"thread '",
                    ],
                );

            if is_error_anchor {
                return Some(ContinuationPattern::ErrorIndentation);
            }
        } else {
            return Some(ContinuationPattern::ErrorIndentation);
        }
    }

    if starts_with_any(
        current,
        &[
            b"...",
            b"\xe2\x94\x94", // └ in UTF-8
            b"\xe2\x86\xb3", // ↳ in UTF-8
            b"\xe2\x94\x82", // │ in UTF-8
            b"\xe2\x94\x9c", // ├ in UTF-8
        ],
    ) {
        return Some(ContinuationPattern::ContinueToken);
    }

    None
}


fn starts_with_any(haystack: &[u8], needles: &[&[u8]]) -> bool {
    needles.iter().any(|n| haystack.starts_with(n))
}

fn contains_any(haystack: &[u8], needles: &[&[u8]]) -> bool {
    needles.iter().any(|n| {
        haystack
            .windows(n.len())
            .any(|w| w == *n)
    })
}

/// Skip past common log-line prefixes (timestamps, brackets, container IDs)
/// and return the offset where the "real" message content starts.
///
/// Handles patterns like:
/// - `2026-02-05T10:00:00.000Z ERROR ...`
/// - `2026-02-05 10:00:00 [ERROR] ...`
/// - `[2026-02-05T10:00:00Z] ERROR ...`
/// - `Jan  5 10:00:00 hostname app: ERROR ...`
/// - `container_id | ERROR ...`
fn skip_log_prefix(content: &[u8]) -> usize {
    if content.is_empty() {
        return 0;
    }

    let len = content.len();
    let mut pos = 0;

    // Log level keywords — used to avoid eating bracketed levels like [ERROR]
    let level_keywords: &[&[u8]] = &[
        b"ERROR", b"WARN", b"INFO", b"DEBUG", b"TRACE", b"FATAL",
        b"error", b"warn", b"info", b"debug", b"trace", b"fatal",
        b"WARNING", b"CRITICAL", b"NOTICE",
        b"warning", b"critical", b"notice",
    ];

    // Skip leading whitespace
    while pos < len && content[pos].is_ascii_whitespace() {
        pos += 1;
    }

    // Attempt to skip up to 4 "prefix segments" (timestamp + optional hostname/tag)
    for _ in 0..4 {
        if pos >= len {
            return 0; // Consumed everything — no real content found
        }

        // Skip whitespace between segments
        while pos < len && content[pos].is_ascii_whitespace() {
            pos += 1;
        }

        // Bracketed group: [anything]
        // But do NOT consume if the bracket contains a log level keyword (e.g. [ERROR])
        if pos < len && content[pos] == b'[' {
            if let Some(end) = content[pos..].iter().position(|&b| b == b']') {
                let inner = &content[pos + 1..pos + end];
                let is_level = level_keywords
                    .iter()
                    .any(|kw| inner == *kw);
                if is_level {
                    // This bracket contains a level — stop skipping
                    break;
                }
                pos += end + 1;
                continue;
            }
        }

        // Timestamp-like: starts with digit, read until whitespace
        if pos < len && content[pos].is_ascii_digit() {
            let start = pos;
            let mut has_separator = false;
            while pos < len && !content[pos].is_ascii_whitespace() {
                if content[pos] == b'-' || content[pos] == b':' || content[pos] == b'T' {
                    has_separator = true;
                }
                pos += 1;
            }
            if has_separator && (pos - start) >= 8 {
                // Looks like a timestamp (at least 8 chars with date/time separators)
                // Check if a space-separated time component follows (e.g. "2026-02-05 10:00:00.123")
                // Peek ahead: skip whitespace, check if next token is time-like (digit + colon, < 20 chars)
                let saved = pos;
                while pos < len && content[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                if pos < len && content[pos].is_ascii_digit() {
                    let time_start = pos;
                    let mut time_has_colon = false;
                    while pos < len && !content[pos].is_ascii_whitespace() {
                        if content[pos] == b':' {
                            time_has_colon = true;
                        }
                        pos += 1;
                    }
                    let time_len = pos - time_start;
                    if time_has_colon && time_len >= 5 && time_len <= 20 {
                        // Consumed the time part as well (space-separated datetime)
                        continue;
                    }
                    // Not a time component — rewind to just after the date token
                    pos = saved;
                }
                continue;
            } else {
                // Wasn't a timestamp — rewind
                pos = start;
                break;
            }
        }

        // Syslog month prefix: "Jan  5 10:00:00 hostname app:"
        let syslog_months: &[&[u8]] = &[
            b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun",
            b"Jul", b"Aug", b"Sep", b"Oct", b"Nov", b"Dec",
        ];
        if pos + 3 <= len && syslog_months.iter().any(|m| content[pos..].starts_with(m)) {
            let start = pos;
            // Skip month
            pos += 3;
            // Skip spaces + day + space + time
            while pos < len
                && (content[pos].is_ascii_whitespace()
                    || content[pos].is_ascii_digit()
                    || content[pos] == b':')
            {
                pos += 1;
            }
            if pos - start >= 12 {
                // Skip optional hostname (non-space word followed by space)
                while pos < len && content[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                let word_start = pos;
                while pos < len && !content[pos].is_ascii_whitespace() && content[pos] != b':' {
                    pos += 1;
                }
                // If word ends with ':' skip it (syslog tag)
                if pos < len && content[pos] == b':' {
                    pos += 1;
                } else if pos > word_start {
                    // It might be a hostname — skip it and try the next word for tag
                    while pos < len && content[pos].is_ascii_whitespace() {
                        pos += 1;
                    }
                    let tag_start = pos;
                    while pos < len && !content[pos].is_ascii_whitespace() && content[pos] != b':'
                    {
                        pos += 1;
                    }
                    if pos < len && content[pos] == b':' {
                        pos += 1;
                    } else {
                        pos = tag_start;
                    }
                }
                continue;
            } else {
                pos = start;
                break;
            }
        }

        // Pipe separator: "container_name | ERROR ..."
        if let Some(pipe_pos) = content[pos..].iter().position(|&b| b == b'|') {
            let absolute = pos + pipe_pos;
            // Only treat as prefix if pipe is reasonably close (within 80 chars),
            // there's content after, AND the left side doesn't already contain
            // a log level keyword (avoids consuming "ERROR | details" as prefix).
            if pipe_pos < 80 && absolute + 2 < len {
                let left = &content[pos..absolute];
                let has_level_before = level_keywords.iter().any(|kw| {
                    left.windows(kw.len()).any(|w| w == *kw)
                });
                if !has_level_before {
                    pos = absolute + 1;
                    continue;
                }
            }
        }

        // Nothing matched — stop scanning
        break;
    }

    // Skip trailing whitespace after prefix
    while pos < len && content[pos].is_ascii_whitespace() {
        pos += 1;
    }

    // Safety: if we consumed most of the line, it probably wasn't prefix.
    // e.g. "2026-02-05T10:00:00Z 2026-02-05 10:00:00.123 ERROR boom"
    if len > 10 && pos > len * 5 / 6 {
        return 0;
    }

    pos
}

/// Check if a line contains a log level keyword, scanning past timestamp prefixes.
fn has_log_level_prefix(content: &[u8]) -> bool {
    if content.is_empty() {
        return false;
    }

    let levels: &[&[u8]] = &[
        b"ERROR", b"WARN", b"INFO", b"DEBUG", b"TRACE", b"FATAL",
        b"error", b"warn", b"info", b"debug", b"trace", b"fatal",
        b"E ", b"W ", b"I ", // Single-letter levels (Go zap, etc.)
    ];

    if check_level_at(content, 0, levels) {
        return true;
    }

    let offset = skip_log_prefix(content);
    if offset > 0 && offset < content.len() {
        if content[offset] == b'[' {
            let after_bracket = offset + 1;
            if after_bracket < content.len() && check_level_at(content, after_bracket, levels) {
                return true;
            }
        }
        if check_level_at(content, offset, levels) {
            return true;
        }
    }

    false
}

/// Check if a log level keyword appears at exact position `pos` with a word boundary after it.
fn check_level_at(content: &[u8], pos: usize, levels: &[&[u8]]) -> bool {
    let slice = &content[pos..];
    for &level in levels {
        if slice.starts_with(level) {
            let end = pos + level.len();
            if end >= content.len() {
                return true; // Level is at end of line
            }
            let next = content[end];
            // Word boundary: anything non-alphanumeric and not underscore
            if !next.is_ascii_alphanumeric() && next != b'_' {
                return true;
            }
        }
    }
    false
}


#[cfg(test)]
mod tests {
    use super::*;

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

        // Verify structure preserved (not merged)
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

        // No timeout yet
        assert!(grouper.check_timeout().is_none());
        assert!(grouper.has_pending());

        std::thread::sleep(Duration::from_millis(100));

        // Now it should flush
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

    // ─── Log header detection (with timestamp prefixes) ─────────

    #[test]
    fn test_timestamped_error_header() {
        assert!(has_log_level_prefix(
            b"2026-02-05T10:00:00.000Z ERROR something broke"
        ));
        assert!(has_log_level_prefix(
            b"2026-02-05T10:00:00.000Z INFO startup"
        ));
    }

    #[test]
    fn test_bracketed_timestamp_header() {
        assert!(has_log_level_prefix(
            b"[2026-02-05T10:00:00Z] ERROR something"
        ));
        assert!(has_log_level_prefix(
            b"[2026-02-05T10:00:00Z] [ERROR] something"
        ));
    }

    #[test]
    fn test_plain_level_at_start() {
        assert!(has_log_level_prefix(b"ERROR something broke"));
        assert!(has_log_level_prefix(b"WARN low disk"));
        assert!(has_log_level_prefix(b"INFO started"));
        assert!(has_log_level_prefix(b"DEBUG details"));
        assert!(has_log_level_prefix(b"FATAL crash"));
    }

    #[test]
    fn test_level_word_boundary() {
        assert!(!has_log_level_prefix(b"information is key"));
        assert!(!has_log_level_prefix(b"warning_count=5"));
        assert!(!has_log_level_prefix(b"debuggable item"));
        assert!(!has_log_level_prefix(b"traceback at line 5"));
    }

    #[test]
    fn test_level_with_colon() {
        assert!(has_log_level_prefix(b"ERROR: something broke"));
        assert!(has_log_level_prefix(b"WARN: attention"));
    }

    #[test]
    fn test_empty_content_no_header() {
        assert!(!has_log_level_prefix(b""));
    }

    #[test]
    fn test_syslog_style_header() {
        assert!(has_log_level_prefix(
            b"Jan  5 10:00:00 myhost app: ERROR something"
        ));
    }

    #[test]
    fn test_pipe_prefixed_header() {
        assert!(has_log_level_prefix(b"web_1 | ERROR something"));
    }

    #[test]
    fn test_double_timestamp_header() {
        assert!(has_log_level_prefix(
            b"2026-02-05T10:00:00Z 2026-02-05 10:00:00.123 ERROR boom"
        ));
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

        // Switching to passthrough drains pending group into deferred queue
        grouper.set_passthrough(true);
        assert!(grouper.has_pending()); // Flushed group is queued for deferred emission

        // process(line3) with non-empty queue: returns BOTH the queued grouped
        // entry AND line3 in one call (zero-latency passthrough).
        let line3 = create_entry(b"{\"level\":\"info\"}", 3, 3);
        let results = grouper.process(line3);
        assert_eq!(results.len(), 2);
        assert!(results[0].is_grouped);
        assert_eq!(results[0].raw_content, b"ERROR panic");
        assert!(!results[1].is_grouped);
        assert_eq!(results[1].raw_content, b"{\"level\":\"info\"}");

        // Queue is now fully drained — subsequent entries pass through immediately
        let line4 = create_entry(b"{\"level\":\"warn\"}", 4, 4);
        let results4 = grouper.process(line4);
        assert_eq!(results4.len(), 1);
        assert_eq!(results4[0].raw_content, b"{\"level\":\"warn\"}");

        // Nothing pending — flush returns None
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
        assert_eq!(flushed.line_count, 4); // 1 primary + 3 continuations
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
        assert_eq!(flushed.line_count, 2); // primary + 1
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
        // Whitespace-only is indented, and previous was ERROR → groups in conservative mode
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
    fn test_only_level_word() {
        assert!(has_log_level_prefix(b"ERROR"));
        assert!(has_log_level_prefix(b"WARN"));
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

    // ─── skip_log_prefix unit tests ─────────────────────────────

    #[test]
    fn test_skip_prefix_iso_timestamp() {
        let input = b"2026-02-05T10:00:00.000Z ERROR boom";
        let offset = skip_log_prefix(input);
        assert_eq!(&input[offset..], b"ERROR boom");
    }

    #[test]
    fn test_skip_prefix_bracketed_timestamp() {
        let input = b"[2026-02-05T10:00:00Z] ERROR boom";
        let offset = skip_log_prefix(input);
        assert_eq!(&input[offset..], b"ERROR boom");
    }

    #[test]
    fn test_skip_prefix_no_prefix() {
        assert_eq!(skip_log_prefix(b"ERROR boom"), 0);
    }

    #[test]
    fn test_skip_prefix_empty() {
        assert_eq!(skip_log_prefix(b""), 0);
    }

    #[test]
    fn test_skip_prefix_double_timestamp() {
        let input = b"2026-02-05T10:00:00Z 2026-02-05 10:00:00.123 ERROR boom";
        let offset = skip_log_prefix(input);
        assert_eq!(&input[offset..], b"ERROR boom");
    }

    #[test]
    fn test_skip_prefix_pipe() {
        let input = b"web_1 | ERROR crash";
        let offset = skip_log_prefix(input);
        assert_eq!(&input[offset..], b"ERROR crash");
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
