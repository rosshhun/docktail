/// ANSI escape code stripping and Docker timestamp handling
/// 
/// Docker logs often contain ANSI color codes from applications using colored
/// terminal output. These need to be stripped before parsing to avoid false
/// negatives in format detection.
/// 
/// Additionally, Docker prepends its own timestamp to log lines which needs
/// to be removed from the content (the timestamp is already extracted separately).

use std::borrow::Cow;

/// Strip Docker timestamp prefix from log content
/// 
/// Docker prepends its own timestamp to log lines when timestamps are enabled:
/// `2026-01-30T03:29:06.691716216Z {"level": "ERROR", ...}`
/// 
/// This timestamp is already extracted and available in the LogLine.timestamp field,
/// so we need to remove it from the content bytes to allow proper format detection.
/// 
/// Format: ISO8601 with nanoseconds and Z suffix
/// Pattern: YYYY-MM-DDTHH:MM:SS.nnnnnnnnnZ (space after)
pub fn strip_docker_timestamp(input: &[u8]) -> &[u8] {
    // Docker/Podman timestamp formats:
    // - Docker: 2026-01-30T03:29:06.691716216Z 
    // - Podman: 2026-01-30T03:29:06Z or 2026-01-30 03:29:06Z (space variant)
    // Minimum length: 20 chars (YYYY-MM-DD HH:MM:SSZ) + space = 21 bytes
    if input.len() < 21 {
        return input;
    }
    
    // Quick check: does it look like an ISO8601 timestamp?
    // YYYY-MM-DD starts with digit, has dashes at positions 4 and 7
    if !input[0].is_ascii_digit() || input[4] != b'-' || input[7] != b'-' {
        return input;
    }
    
    // Check for date/time separator: 'T' (ISO8601) or ' ' (Podman variant)
    let has_separator = input[10] == b'T' || input[10] == b' ';
    if !has_separator || input[13] != b':' || input[16] != b':' {
        return input;
    }
    
    // Look for 'Z' timezone marker (could be at position 19 for no-decimal or later for fractional seconds)
    // Search from position 19 up to 35 to handle various precision levels
    let search_start = 19;
    let search_end = std::cmp::min(input.len(), 35);
    
    if let Some(z_pos) = input[search_start..search_end].iter().position(|&b| b == b'Z') {
        let actual_pos = search_start + z_pos;
        // Check if there's a space after Z
        if actual_pos + 1 < input.len() && input[actual_pos + 1] == b' ' {
            return &input[actual_pos + 2..]; // Skip "Z "
        } else {
            return &input[actual_pos + 1..]; // Skip "Z"
        }
    }
    
    input
}

/// Strip ANSI escape codes from bytes
/// 
/// Handles:
/// - CSI sequences: `\x1b[...m`
/// - OSC sequences: `\x1b]...`
/// - Other escape sequences
/// 
/// Returns Cow::Borrowed if no codes were found (Zero Allocation),
/// or Cow::Owned if stripping occurred.
pub fn strip_ansi_codes(input: &[u8]) -> Cow<'_, [u8]> {
    // Optimization: Quick scan for ESC (0x1b). 
    // If not present, return the original slice immediately.
    if !input.contains(&0x1b) {
        return Cow::Borrowed(input);
    }

    let mut output = Vec::with_capacity(input.len());
    let mut i = 0;
    
    while i < input.len() {
        // Check for ESC character (0x1b or 27)
        if input[i] == 0x1b && i + 1 >= input.len() {
            // Lone trailing ESC byte — strip it (incomplete escape sequence)
            i += 1;
            continue;
        }
        if input[i] == 0x1b {
            // CSI sequence: ESC [ ... (ends with 0x40-0x7E, usually 'm' for colors)
            if input[i + 1] == b'[' {
                i += 2; // Skip ESC [
                // Skip until we find the terminator byte (0x40-0x7E)
                while i < input.len() {
                    let b = input[i];
                    i += 1;
                    if b >= 0x40 && b <= 0x7E {
                        break;
                    }
                }
                continue;
            }
            
            // OSC sequence: ESC ] ... (ends with BEL 0x07 or ESC \)
            // Used for hyperlinks in modern terminals
            if input[i + 1] == b']' {
                i += 2; // Skip ESC ]
                while i < input.len() {
                    if input[i] == 0x07 { // BEL terminator
                        i += 1;
                        break;
                    }
                    if input[i] == 0x1b && i + 1 < input.len() && input[i + 1] == b'\\' {
                        // ST (String Terminator) ESC \
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            
            // Simple Fe sequences (ESC + single char, e.g. ESC N)
            // Range 0x40-0x5F
            if input[i + 1] >= 0x40 && input[i + 1] <= 0x5F {
                i += 2;
                continue;
            }
        }
        
        // Regular character - copy it
        output.push(input[i]);
        i += 1;
    }
    
    Cow::Owned(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_docker_timestamp() {
        let input = b"2026-01-30T03:29:06.691716216Z {\"level\": \"ERROR\"}";
        let output = strip_docker_timestamp(input);
        assert_eq!(output, b"{\"level\": \"ERROR\"}");
    }

    #[test]
    fn test_strip_docker_timestamp_with_tracing() {
        let input = b"2026-01-30T03:33:06.062258424Z 2026-01-30T03:33:06.061778Z  INFO cluster: Starting";
        let output = strip_docker_timestamp(input);
        assert_eq!(output, b"2026-01-30T03:33:06.061778Z  INFO cluster: Starting");
    }

    #[test]
    fn test_no_docker_timestamp() {
        let input = b"{\"level\": \"ERROR\"}";
        let output = strip_docker_timestamp(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_strip_podman_timestamp_no_decimal() {
        // Podman may omit fractional seconds
        let input = b"2026-01-30T03:29:06Z Hello World";
        let output = strip_docker_timestamp(input);
        assert_eq!(output, b"Hello World");
    }

    #[test]
    fn test_strip_podman_timestamp_space_separator() {
        // Podman may use space instead of T
        let input = b"2026-01-30 03:29:06.123Z Hello World";
        let output = strip_docker_timestamp(input);
        assert_eq!(output, b"Hello World");
    }

    #[test]
    fn test_strip_podman_timestamp_microseconds() {
        // Podman may use microseconds instead of nanoseconds
        let input = b"2026-01-30T03:29:06.123456Z Hello World";
        let output = strip_docker_timestamp(input);
        assert_eq!(output, b"Hello World");
    }

    #[test]
    fn test_strip_ansi_cow_optimization() {
        // Ensure no allocation for plain strings
        let input = b"Hello World";
        let output = strip_ansi_codes(input);
        match output {
            Cow::Borrowed(s) => assert_eq!(s, b"Hello World"),
            Cow::Owned(_) => panic!("Should not have allocated"),
        }
    }

    #[test]
    fn test_strip_simple_ansi() {
        let input = b"\x1b[32mHello\x1b[0m World";
        let output = strip_ansi_codes(input);
        assert_eq!(output.as_ref(), b"Hello World");
    }

    #[test]
    fn test_strip_complex_ansi() {
        // Real example from tracing logs
        let input = b"\x1b[2m2026-01-30T03:18:50.827498Z\x1b[0m \x1b[32m INFO\x1b[0m \x1b[2mcluster\x1b[0m\x1b[2m:\x1b[0m Starting Docktail";
        let output = strip_ansi_codes(input);
        assert_eq!(output.as_ref(), b"2026-01-30T03:18:50.827498Z  INFO cluster: Starting Docktail");
    }

    #[test]
    fn test_no_ansi_codes() {
        let input = b"Plain text without ANSI";
        let output = strip_ansi_codes(input);
        assert_eq!(output.as_ref(), input);
    }

    #[test]
    fn test_empty_input() {
        let input = b"";
        let output = strip_ansi_codes(input);
        assert_eq!(output.as_ref(), b"");
    }

    #[test]
    fn test_only_ansi_codes() {
        let input = b"\x1b[0m\x1b[32m\x1b[1m";
        let output = strip_ansi_codes(input);
        assert_eq!(output.as_ref(), b"");
    }

    #[test]
    fn test_json_with_ansi() {
        let input = b"\x1b[32m{\"level\":\"info\",\"msg\":\"test\"}\x1b[0m";
        let output = strip_ansi_codes(input);
        assert_eq!(output.as_ref(), b"{\"level\":\"info\",\"msg\":\"test\"}");
    }

    #[test]
    fn test_osc_hyperlink() {
        // ESC ] 8 ; ; https://example.com BEL Link BEL
        let input = b"\x1b]8;;https://example.com\x07Link\x1b]8;;\x07";
        let output = strip_ansi_codes(input);
        assert_eq!(output.as_ref(), b"Link");
    }

    #[test]
    fn test_performance_cow_optimization() {
        // Verify that Cow::Borrowed is returned for plain text
        // This is the key optimization - no allocation for clean logs
        let plain_logs = vec![
            b"{\"level\":\"info\",\"msg\":\"Application started\"}".as_slice(),
            b"2026-01-30T10:15:30.123Z INFO server listening on :8080".as_slice(),
            b"Processing request for user_id=12345".as_slice(),
        ];
        
        for log in plain_logs {
            let result = strip_ansi_codes(log);
            // Verify it's borrowed (zero allocation)
            assert!(matches!(result, Cow::Borrowed(_)), 
                "Expected Cow::Borrowed for plain log, got Cow::Owned");
            assert_eq!(result.as_ref(), log);
        }
        
        // Verify that Cow::Owned is returned when stripping is needed
        let colored_log = b"\x1b[32mINFO\x1b[0m message";
        let result = strip_ansi_codes(colored_log);
        assert!(matches!(result, Cow::Owned(_)), 
            "Expected Cow::Owned for log with ANSI codes");
        assert_eq!(result.as_ref(), b"INFO message");
    }

    #[test]
    fn test_strip_docker_timestamp_at_very_end() {
        // Timestamp is at the very end of the string without trailing newline or space
        let input = b"2026-01-30T03:29:06.123Z";
        let output = strip_docker_timestamp(input);
        assert_eq!(output, b"");
    }
}
