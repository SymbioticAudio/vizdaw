//! Standard base64 with padding (RFC 4648 §4), for MCP image content.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn symbol(six_bits: u32) -> char {
    // `six_bits` is masked to 0..64, so the lookup always succeeds.
    ALPHABET
        .get((six_bits & 0x3f) as usize)
        .map_or('A', |&b| char::from(b))
}

/// `bytes` as base64, padded with `=`.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = |i: usize| chunk.get(i).copied().map_or(0, u32::from);
        let n = (b(0) << 16) | (b(1) << 8) | b(2);
        out.push(symbol(n >> 18));
        out.push(symbol(n >> 12));
        out.push(if chunk.len() > 1 { symbol(n >> 6) } else { '=' });
        out.push(if chunk.len() > 2 { symbol(n) } else { '=' });
    }
    out
}
