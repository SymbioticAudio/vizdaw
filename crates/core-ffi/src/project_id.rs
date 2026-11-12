//! New project IDs (plan 6a Ruling 16): a version 4 UUID, lowercase and
//! hyphenated, stamped on a project's first save.

use std::io::Read;

/// A fresh version 4 UUID from `/dev/urandom`, or from a hash of the time,
/// the process ID and a counter when that cannot be read.
pub fn fresh() -> String {
    let mut b = [0u8; 16];
    let read = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b));
    if read.is_err() {
        b = fallback();
    }
    format_v4(b)
}

/// 16 bytes from the time, the process ID and a per-process counter.
fn fallback() -> [u8; 16] {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNT.fetch_add(1, Ordering::Relaxed);
    let half = |salt: u64| {
        // `RandomState` is seeded per process: two halves, two seeds.
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(nanos);
        h.write_u32(std::process::id());
        h.write_u64(n);
        h.write_u64(salt);
        h.finish().to_le_bytes()
    };
    let mut out = [0u8; 16];
    for (dst, src) in out.iter_mut().zip(half(0).into_iter().chain(half(1))) {
        *dst = src;
    }
    out
}

/// `b` as a version 4, variant 1 UUID: `xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx`.
fn format_v4(mut b: [u8; 16]) -> String {
    if let Some(v) = b.get_mut(6) {
        *v = (*v & 0x0F) | 0x40;
    }
    if let Some(v) = b.get_mut(8) {
        *v = (*v & 0x3F) | 0x80;
    }
    let mut s = String::with_capacity(36);
    for (i, byte) in b.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            s.push('-');
        }
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_valid_v4_and_differ() {
        let (a, b) = (fresh(), fresh());
        assert!(core_model::document::check_project_id(&a), "{a}");
        assert_ne!(a, b);
        assert_eq!(a.as_bytes()[14], b'4');
        assert!(matches!(a.as_bytes()[19], b'8' | b'9' | b'a' | b'b'), "{a}");
    }

    #[test]
    fn the_fallback_is_a_valid_id_too() {
        let (a, b) = (format_v4(fallback()), format_v4(fallback()));
        assert!(core_model::document::check_project_id(&a), "{a}");
        assert_ne!(a, b);
        assert_eq!(format_v4([0; 16]), "00000000-0000-4000-8000-000000000000");
    }
}
