//! MIDI message framing for surface input (spec §12.2).

/// The number of data bytes after a status byte that starts a message
/// other than SysEx, or `None` for a byte that starts nothing.
fn data_len(status: u8) -> Option<usize> {
    match status {
        0x80..=0xBF | 0xE0..=0xEF | 0xF2 => Some(2),
        0xC0..=0xDF | 0xF1 | 0xF3 => Some(1),
        0xF6 => Some(0),
        _ => None,
    }
}

/// A System Real-Time byte: a one-byte message legal anywhere.
fn real_time(b: u8) -> bool {
    b >= 0xF8
}

/// Splits `bytes` into whole messages: channel messages by length, SysEx from F0 to F7.
/// System Real-Time bytes (F8–FF) are one-byte messages legal anywhere, even inside
/// another message or a SysEx: they are skipped and neither returned nor reported.
/// A byte that starts nothing (F4, F5, a lone F7), a cut message, a data byte above
/// 0x7F or running status are reported as `Err(len)` of the bytes skipped.
///
/// A status byte inside a SysEx cuts it: the SysEx so far is reported and the
/// status byte starts the next message.
pub fn split(bytes: &[u8]) -> Vec<Result<Vec<u8>, usize>> {
    let mut out = Vec::new();
    let mut rest = bytes.iter().copied().filter(|&b| !real_time(b)).peekable();
    while let Some(status) = rest.next() {
        if status == 0xF0 {
            let mut msg = vec![status];
            let mut done = false;
            while let Some(&b) = rest.peek() {
                if b == 0xF7 {
                    msg.push(b);
                    rest.next();
                    done = true;
                    break;
                }
                if b >= 0x80 {
                    break;
                }
                msg.push(b);
                rest.next();
            }
            out.push(if done { Ok(msg) } else { Err(msg.len()) });
            continue;
        }
        let Some(n) = data_len(status) else {
            out.push(Err(1));
            continue;
        };
        let mut msg = vec![status];
        msg.extend(rest.by_ref().take(n));
        if msg.len() == n.saturating_add(1) && msg.iter().skip(1).all(|&b| b < 0x80) {
            out.push(Ok(msg));
        } else {
            out.push(Err(msg.len()));
        }
    }
    out
}

/// Bytes as lower-case hex pairs separated by spaces ("90 68 7f").
pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
