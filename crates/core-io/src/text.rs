//! Text from files: one cleaning for the SMF and Clip File paths.

use crate::limits::Limits;

/// Decodes `bytes` as UTF-8 (invalid sequences become U+FFFD), drops control
/// characters other than newline and bidirectional overrides and isolates,
/// and cuts the result to `limits.max_text_bytes` without splitting a
/// character. The flag is true if it was cut.
pub fn sanitize(bytes: &[u8], limits: &Limits) -> (String, bool) {
    // Decoding a bounded prefix keeps work bounded; one extra byte-run is
    // enough to know whether anything was left.
    let cleaned: String = String::from_utf8_lossy(bytes)
        .chars()
        .filter(|&c| {
            c == '\n'
                || !(c.is_control()
                    || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        })
        .collect();
    if cleaned.len() <= limits.max_text_bytes {
        return (cleaned, false);
    }
    let mut end = limits.max_text_bytes;
    while !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    (cleaned[..end].to_string(), true)
}
