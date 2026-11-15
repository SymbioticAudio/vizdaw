//! File and folder names made from user text (plan 6b Task 1).

/// The longest file name most file systems take, in bytes.
pub const MAX_NAME_BYTES: usize = 255;

/// What an empty name becomes.
const UNTITLED: &str = "Untitled";

/// `name` as part of a file or folder name: `/`, `:` and control
/// characters become `-`; it is trimmed, a leading `.` becomes `-` (no
/// hidden files), and it is cut at a character boundary to
/// `255 - reserve` bytes, so the rest of the name (`reserve` bytes) still
/// fits. An empty result is `"Untitled"`.
pub fn safe_part(name: &str, reserve: usize) -> String {
    let max = MAX_NAME_BYTES.saturating_sub(reserve);
    let mapped: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == ':' || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let trimmed = mapped.trim();
    let mut out = String::with_capacity(trimmed.len().min(max));
    for (i, c) in trimmed.chars().enumerate() {
        let c = if i == 0 && c == '.' { '-' } else { c };
        if out.len().saturating_add(c.len_utf8()) > max {
            break;
        }
        out.push(c);
    }
    let kept = out.trim_end();
    if kept.is_empty() {
        UNTITLED.to_string()
    } else {
        kept.to_string()
    }
}
