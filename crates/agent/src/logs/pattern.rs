//! Continuation-line detection and log-prefix skipping.
//!
//! Pure byte-level helpers used by [`super::group::MultilineGrouper`] to decide
//! whether a new log line is a continuation of the previous entry (stack frame,
//! indented detail, "Caused by:" chain, etc.) or the start of a new log group.

/// Continuation pattern kinds (used for tracing diagnostics).
#[derive(Debug)]
pub(crate) enum ContinuationPattern {
    StackFrame,
    ErrorIndentation,
    ContinueToken,
}

/// Detect if `current` is a continuation of the previous primary line.
pub(crate) fn is_continuation_line(
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

    // Rust backtrace numbering: "   0:", "   1:", "  10:", etc.
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
pub(crate) fn skip_log_prefix(content: &[u8]) -> usize {
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
                // Looks like a timestamp — check for space-separated time component
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
                        continue;
                    }
                    pos = saved;
                }
                continue;
            } else {
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
            pos += 3;
            while pos < len
                && (content[pos].is_ascii_whitespace()
                    || content[pos].is_ascii_digit()
                    || content[pos] == b':')
            {
                pos += 1;
            }
            if pos - start >= 12 {
                // Skip optional hostname
                while pos < len && content[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                let word_start = pos;
                while pos < len && !content[pos].is_ascii_whitespace() && content[pos] != b':' {
                    pos += 1;
                }
                if pos < len && content[pos] == b':' {
                    pos += 1;
                } else if pos > word_start {
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

        break;
    }

    // Skip trailing whitespace after prefix
    while pos < len && content[pos].is_ascii_whitespace() {
        pos += 1;
    }

    // Safety: if we consumed most of the line, it probably wasn't prefix.
    if len > 10 && pos > len * 5 / 6 {
        return 0;
    }

    pos
}

/// Check if a line contains a log level keyword, scanning past timestamp prefixes.
pub(crate) fn has_log_level_prefix(content: &[u8]) -> bool {
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
                return true;
            }
            let next = content[end];
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

    #[test]
    fn test_only_level_word() {
        assert!(has_log_level_prefix(b"ERROR"));
        assert!(has_log_level_prefix(b"WARN"));
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
}
