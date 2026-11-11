//! Text from files: one cleaning for the SMF and Clip File paths.

use crate::limits::Limits;

/// True for a character that is dropped: a control character other than
/// newline, or a bidirectional override or isolate.
fn dropped(c: char) -> bool {
    (c.is_control() && c != '\n') || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Decodes `bytes` as UTF-8 (invalid sequences become U+FFFD), drops control
/// characters other than newline and bidirectional overrides and isolates
/// (U+202A to U+202E, U+2066 to U+2069: they would fail
/// `core_model::rules::check_info`), and cuts the result to
/// `limits.max_text_bytes` without splitting a character. The flag is true
/// if kept text was cut. Work and memory stop at the limit, whatever the
/// input size.
pub fn sanitize(bytes: &[u8], limits: &Limits) -> (String, bool) {
    let mut out = String::new();
    for chunk in bytes.utf8_chunks() {
        let bad = (!chunk.invalid().is_empty()).then_some('\u{FFFD}');
        for c in chunk.valid().chars().chain(bad) {
            if dropped(c) {
                continue;
            }
            if out.len().saturating_add(c.len_utf8()) > limits.max_text_bytes {
                return (out, true);
            }
            out.push(c);
        }
    }
    (out, false)
}
