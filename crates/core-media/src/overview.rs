//! Waveform overviews (sub-project 5 spec §10.1; plan 5b Rulings 1–3, 5).
//!
//! An overview holds min/max peaks of a media file at 256, 4096 and
//! 65 536 frames per peak, per channel, as `i16` scaled by the file's
//! largest absolute sample (at least full scale), the min rounded down and
//! the max rounded up so the envelope always contains the samples. It
//! lives in `Media/.overviews/<content hash>.vzov` with the `(length,
//! mtime)` stamps of the files it was built from.
//!
//! [`build`] reads a CAF once and hashes its sample bytes on the way, so a
//! build also tells whether the file still holds the audio a project
//! expects. Overviews are a cache, not media: they are written to a temp
//! file and renamed into place, and may be replaced or deleted. Nothing
//! here runs on (or near) the real-time thread.

use crate::MediaError;
use crate::caf::CafReader;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Where overviews live inside a package.
pub const DIR: &str = "Media/.overviews";
/// Frames per peak of each level, finest first.
pub const LEVELS: [u32; 3] = [256, 4096, 65_536];
/// Stamps an overview keeps (newest last).
pub const MAX_STAMPS: usize = 8;
const MAGIC: &[u8; 4] = b"VZOV";
const VERSION: u32 = 1;
const HASH_BYTES: usize = 64;
const STAMP_BYTES: usize = 24;
/// The fixed header: magic, version, channels, level count, scale,
/// frames, hash, stamp count, stamp slots.
pub const HEADER_BYTES: usize = 4 + 4 + 2 + 2 + 4 + 8 + HASH_BYTES + 4 + MAX_STAMPS * STAMP_BYTES;
/// Frames per peak of the finest level.
const L0: u64 = 256;
/// Each level is this many peaks of the one below.
const STEP: usize = 16;
const CHUNK_FRAMES: usize = 65_536;
const FULL: f64 = 32_767.0;
/// More finest-level peaks than this is a damaged header (24 hours at
/// 768 kHz is about 2^28).
const MAX_PEAKS: u64 = 1 << 28;
/// The most bins one [`Bins::read`] makes.
const MAX_BINS: u64 = 1 << 22;

/// A media file's identity on disk: its length and modification time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    /// File length in bytes.
    pub len: u64,
    /// Modification time, seconds since the epoch.
    pub mtime_s: i64,
    /// Modification time, nanoseconds within the second.
    pub mtime_ns: u32,
}

impl Stamp {
    /// The stamp of the file at `path`.
    pub fn of(path: &Path) -> Result<Stamp, MediaError> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(path)?;
        Ok(Stamp {
            len: m.len(),
            mtime_s: m.mtime(),
            mtime_ns: u32::try_from(m.mtime_nsec()).unwrap_or(0),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Level {
    frames_per_peak: u32,
    /// Per peak, per channel: min, then max.
    peaks: Vec<i16>,
}

/// One media file's overview.
#[derive(Debug, Clone, PartialEq)]
pub struct Overview {
    /// 1 or 2.
    pub channels: u16,
    /// Frames the file held when built.
    pub frames: u64,
    /// What full scale (32 767) stands for: the file's largest absolute
    /// sample, at least 1.
    pub scale: f32,
    /// BLAKE3 hex of the file's sample bytes.
    pub hash: String,
    /// Files this overview was built from or checked against.
    pub stamps: Vec<Stamp>,
    levels: Vec<Level>,
}

fn quantize(v: f32, scale: f32, up: bool) -> i16 {
    if !v.is_finite() {
        return 0;
    }
    let x = f64::from(v) / f64::from(scale) * FULL;
    let x = if up { x.ceil() } else { x.floor() };
    x.clamp(-32_768.0, 32_767.0) as i16
}

fn decode(q: i16, scale: f32) -> f32 {
    (f64::from(q) / FULL * f64::from(scale)) as f32
}

/// Min and max per peak of `lo`/`hi` (per peak, per channel), `by` peaks
/// to one.
fn coarser(lo: &[f32], hi: &[f32], ch: usize, by: usize) -> (Vec<f32>, Vec<f32>) {
    let n = lo.len() / ch.max(1);
    let m = n.div_ceil(by);
    let mut l = vec![f32::INFINITY; m * ch];
    let mut h = vec![f32::NEG_INFINITY; m * ch];
    for p in 0..n {
        let q = p / by;
        for c in 0..ch {
            if let (Some(a), Some(x)) = (lo.get(p * ch + c), l.get_mut(q * ch + c)) {
                *x = x.min(*a);
            }
            if let (Some(a), Some(x)) = (hi.get(p * ch + c), h.get_mut(q * ch + c)) {
                *x = x.max(*a);
            }
        }
    }
    (l, h)
}

/// Builds the overview of the VizDAW CAF at `caf` in one read, hashing its
/// sample bytes on the way (so `hash` equals [`crate::caf::hash_file`]'s).
/// A file shorter than its header says gives the overview of what is
/// there. `cancel` stops it with `Cancelled`. No stamps are set.
pub fn build(caf: &Path, cancel: &AtomicBool) -> Result<Overview, MediaError> {
    let r = CafReader::open(caf)?;
    let info = r.info().clone();
    let ch = usize::from(info.channels);
    let fb = info.frame_bytes();
    let peaks0 = info.frames.div_ceil(L0);
    if peaks0 > MAX_PEAKS {
        return Err(MediaError::Unsupported(format!(
            "{}: too long for an overview",
            caf.display()
        )));
    }
    let n0 = peaks0 as usize;
    let mut lo = vec![f32::INFINITY; n0 * ch];
    let mut hi = vec![f32::NEG_INFINITY; n0 * ch];
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0.0f32; CHUNK_FRAMES * ch];
    let mut scratch = Vec::new();
    let mut at = 0u64;
    let mut peak = 0.0f32;
    while at < info.frames {
        if cancel.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        let n = r.read(at, &mut buf, &mut scratch)?;
        if n == 0 {
            break;
        }
        hasher.update(scratch.get(..n * fb).unwrap_or(&[]));
        for (k, frame) in buf.chunks_exact(ch).take(n).enumerate() {
            let p = ((at + k as u64) / L0) as usize;
            for (c, v) in frame.iter().enumerate() {
                let v = if v.is_finite() { *v } else { 0.0 };
                peak = peak.max(v.abs());
                if let Some(x) = lo.get_mut(p * ch + c) {
                    *x = x.min(v);
                }
                if let Some(x) = hi.get_mut(p * ch + c) {
                    *x = x.max(v);
                }
            }
        }
        at += n as u64;
    }
    let used = at.div_ceil(L0) as usize * ch;
    lo.truncate(used);
    hi.truncate(used);
    let scale = peak.max(1.0);
    let mut levels = Vec::with_capacity(LEVELS.len());
    for (i, fpp) in LEVELS.iter().enumerate() {
        if i > 0 {
            (lo, hi) = coarser(&lo, &hi, ch, STEP);
        }
        let peaks = lo
            .iter()
            .zip(&hi)
            .flat_map(|(l, h)| [quantize(*l, scale, false), quantize(*h, scale, true)])
            .collect();
        levels.push(Level {
            frames_per_peak: *fpp,
            peaks,
        });
    }
    Ok(Overview {
        channels: info.channels,
        frames: at,
        scale,
        hash: hasher.finalize().to_hex().to_string(),
        stamps: Vec::new(),
        levels,
    })
}

/// The largest absolute sample of `reader` in frames `[from, to)`.
pub fn exact_peak(reader: &CafReader, from: u64, to: u64) -> Result<f32, MediaError> {
    let ch = usize::from(reader.info().channels);
    let to = to.min(reader.info().frames);
    let mut buf = vec![0.0f32; CHUNK_FRAMES * ch];
    let mut scratch = Vec::new();
    let mut at = from;
    let mut p = 0.0f32;
    while at < to {
        let want = ((to - at) as usize).min(CHUNK_FRAMES);
        let slice = buf.get_mut(..want * ch).unwrap_or(&mut []);
        let n = reader.read(at, slice, &mut scratch)?;
        if n == 0 {
            break;
        }
        for v in slice.iter().take(n * ch) {
            if v.is_finite() {
                p = p.max(v.abs());
            }
        }
        at += n as u64;
    }
    Ok(p)
}

/// Exact sample min/max in `step`-frame bins over `[from, to)`: what a
/// view zoomed in below 256 frames per column draws (Ruling 3).
#[derive(Debug, Clone, PartialEq)]
pub struct Bins {
    /// First frame.
    pub from: u64,
    /// End frame (exclusive; cut to the file).
    pub to: u64,
    /// Frames per bin (≥ 1).
    pub step: u64,
    /// Channels.
    pub channels: u16,
    /// Per bin, per channel: min, then max.
    values: Vec<f32>,
}

impl Bins {
    /// Reads `[from, to)` of `reader` into `step`-frame bins.
    pub fn read(reader: &CafReader, from: u64, to: u64, step: u64) -> Result<Bins, MediaError> {
        let info = reader.info();
        let step = step.max(1);
        let to = to.min(info.frames);
        let ch = usize::from(info.channels);
        if from >= to {
            return Ok(Bins {
                from,
                to: from,
                step,
                channels: info.channels,
                values: Vec::new(),
            });
        }
        let bins = (to - from).div_ceil(step);
        if bins > MAX_BINS {
            return Err(MediaError::Unsupported(format!(
                "{bins} bins is too many for one read"
            )));
        }
        let mut values = Vec::with_capacity(bins as usize * ch * 2);
        for _ in 0..bins as usize * ch {
            values.push(f32::INFINITY);
            values.push(f32::NEG_INFINITY);
        }
        let mut buf = vec![0.0f32; CHUNK_FRAMES * ch];
        let mut scratch = Vec::new();
        let mut at = from;
        while at < to {
            let want = ((to - at) as usize).min(CHUNK_FRAMES);
            let slice = buf.get_mut(..want * ch).unwrap_or(&mut []);
            let n = reader.read(at, slice, &mut scratch)?;
            if n == 0 {
                break;
            }
            for (k, frame) in slice.chunks_exact(ch).take(n).enumerate() {
                let b = ((at + k as u64 - from) / step) as usize;
                for (c, v) in frame.iter().enumerate() {
                    let v = if v.is_finite() { *v } else { 0.0 };
                    let i = (b * ch + c) * 2;
                    if let Some(x) = values.get_mut(i) {
                        *x = x.min(v);
                    }
                    if let Some(x) = values.get_mut(i + 1) {
                        *x = x.max(v);
                    }
                }
            }
            at += n as u64;
        }
        for v in &mut values {
            if !v.is_finite() {
                *v = 0.0;
            }
        }
        Ok(Bins {
            from,
            to: at.min(to),
            step,
            channels: info.channels,
            values,
        })
    }

    /// Min and max of `channel` over frames `[from, to)` (whole bins
    /// that overlap it); `None` outside the bins.
    pub fn range(&self, channel: u16, from: u64, to: u64) -> Option<(f32, f32)> {
        let (from, to) = (from.max(self.from), to.min(self.to));
        if from >= to || channel >= self.channels {
            return None;
        }
        let ch = usize::from(self.channels);
        let c = usize::from(channel);
        let (b0, b1) = (
            (from - self.from) / self.step,
            (to - self.from).div_ceil(self.step),
        );
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for b in b0..b1 {
            let i = (b as usize * ch + c) * 2;
            if let (Some(a), Some(z)) = (self.values.get(i), self.values.get(i + 1)) {
                lo = lo.min(*a);
                hi = hi.max(*z);
            }
        }
        (lo <= hi).then_some((lo, hi))
    }
}

/// A little-endian reader over a byte slice.
struct Cur<'a> {
    b: &'a [u8],
    at: usize,
}

impl Cur<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let s = self.b.get(self.at..self.at.checked_add(N)?)?;
        self.at += N;
        s.try_into().ok()
    }
    fn u16(&mut self) -> Option<u16> {
        self.take().map(u16::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.take().map(u64::from_le_bytes)
    }
    fn i64(&mut self) -> Option<i64> {
        self.take().map(i64::from_le_bytes)
    }
    fn f32(&mut self) -> Option<f32> {
        self.take().map(f32::from_le_bytes)
    }
}

/// What the header holds.
struct Header {
    channels: u16,
    frames: u64,
    scale: f32,
    hash: String,
    stamps: Vec<Stamp>,
}

fn parse_header(b: &[u8]) -> Result<Header, &'static str> {
    let mut c = Cur { b, at: 0 };
    if c.take::<4>().as_ref() != Some(MAGIC) {
        return Err("not an overview");
    }
    if c.u32() != Some(VERSION) {
        return Err("another version");
    }
    let channels = c.u16().ok_or("short")?;
    if !(1..=2).contains(&channels) {
        return Err("channels");
    }
    if c.u16() != Some(LEVELS.len() as u16) {
        return Err("levels");
    }
    let scale = c.f32().ok_or("short")?;
    if !(scale.is_finite() && scale >= 1.0) {
        return Err("scale");
    }
    let frames = c.u64().ok_or("short")?;
    if frames.div_ceil(L0) > MAX_PEAKS {
        return Err("frames");
    }
    let raw: [u8; HASH_BYTES] = c.take().ok_or("short")?;
    let end = raw.iter().position(|b| *b == 0).unwrap_or(HASH_BYTES);
    let hash = std::str::from_utf8(raw.get(..end).unwrap_or(&[]))
        .map_err(|_| "hash")?
        .to_string();
    if !Overview::valid_hash(&hash) {
        return Err("hash");
    }
    let n = c.u32().ok_or("short")? as usize;
    if n > MAX_STAMPS {
        return Err("stamps");
    }
    let mut stamps = Vec::with_capacity(n);
    for i in 0..MAX_STAMPS {
        let (len, s, ns, _) = (
            c.u64().ok_or("short")?,
            c.i64().ok_or("short")?,
            c.u32().ok_or("short")?,
            c.u32().ok_or("short")?,
        );
        if i < n {
            stamps.push(Stamp {
                len,
                mtime_s: s,
                mtime_ns: ns,
            });
        }
    }
    Ok(Header {
        channels,
        frames,
        scale,
        hash,
        stamps,
    })
}

/// The body size a header implies (`None` on overflow).
fn body_bytes(channels: u16, frames: u64) -> Option<u64> {
    let mut total = 0u64;
    for fpp in LEVELS {
        let peaks = frames.div_ceil(u64::from(fpp));
        total = total
            .checked_add(12)?
            .checked_add(peaks.checked_mul(u64::from(channels))?.checked_mul(4)?)?;
    }
    Some(total)
}

impl Overview {
    /// Whether `hash` may name an overview file: 1–64 lowercase hex
    /// characters (a project's hash never becomes a path otherwise).
    pub fn valid_hash(hash: &str) -> bool {
        !hash.is_empty()
            && hash.len() <= HASH_BYTES
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    /// The overview file for `hash` in `package`; `None` for a hash that
    /// is not [`Overview::valid_hash`].
    pub fn path(package: &Path, hash: &str) -> Option<PathBuf> {
        Self::valid_hash(hash).then(|| package.join(DIR).join(format!("{hash}.vzov")))
    }

    /// Adds `s` (newest last), keeping at most [`MAX_STAMPS`].
    pub fn with_stamp(mut self, s: Stamp) -> Overview {
        self.stamps.retain(|x| *x != s);
        self.stamps.push(s);
        while self.stamps.len() > MAX_STAMPS {
            self.stamps.remove(0);
        }
        self
    }

    /// Whether a file with stamp `s` is known to hold this audio.
    pub fn has_stamp(&self, s: &Stamp) -> bool {
        self.stamps.contains(s)
    }

    /// Min and max of `channel` over frames `[from, to)`, from the
    /// coarsest level with at least four peaks in the range (else the
    /// finest); the peaks at both ends are taken whole, so the result
    /// contains every sample of the range. `None` when the range is empty.
    pub fn range(&self, channel: u16, from: u64, to: u64) -> Option<(f32, f32)> {
        let to = to.min(self.frames);
        if from >= to || channel >= self.channels {
            return None;
        }
        let span = to - from;
        let level = self
            .levels
            .iter()
            .rev()
            .find(|l| u64::from(l.frames_per_peak) * 4 <= span)
            .or_else(|| self.levels.first())?;
        let fpp = u64::from(level.frames_per_peak).max(1);
        let ch = usize::from(self.channels);
        let c = usize::from(channel);
        let (mut lo, mut hi) = (i16::MAX, i16::MIN);
        for p in from / fpp..to.div_ceil(fpp) {
            let at = (p as usize * ch + c) * 2;
            if let (Some(a), Some(b)) = (level.peaks.get(at), level.peaks.get(at + 1)) {
                lo = lo.min(*a);
                hi = hi.max(*b);
            }
        }
        (lo <= hi).then(|| (decode(lo, self.scale), decode(hi, self.scale)))
    }

    /// The largest absolute sample over frames `[from, to)`: whole
    /// 256-frame blocks from the finest level, the partial blocks at both
    /// ends from `exact(from, to)` (samples outside the range never count;
    /// Ruling 5).
    pub fn peak(
        &self,
        from: u64,
        to: u64,
        exact: &mut dyn FnMut(u64, u64) -> Result<f32, MediaError>,
    ) -> Result<f32, MediaError> {
        let to = to.min(self.frames);
        if from >= to {
            return Ok(0.0);
        }
        let a = from.div_ceil(L0) * L0;
        let b = to / L0 * L0;
        if a >= b {
            return exact(from, to);
        }
        let mut p = 0.0f32;
        if from < a {
            p = p.max(exact(from, a)?);
        }
        if b < to {
            p = p.max(exact(b, to)?);
        }
        if let Some(level) = self.levels.first() {
            let ch = usize::from(self.channels);
            let (i0, i1) = ((a / L0) as usize * ch * 2, (b / L0) as usize * ch * 2);
            for v in level.peaks.get(i0..i1).unwrap_or(&[]) {
                p = p.max(decode(*v, self.scale).abs());
            }
        }
        Ok(p)
    }

    /// Writes the overview to `path` through a temp file beside it and a
    /// rename (an overview is a cache: replacing one is fine).
    pub fn write(&self, path: &Path) -> Result<(), MediaError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = path
            .parent()
            .ok_or_else(|| MediaError::Io(format!("{}: no folder", path.display())))?;
        std::fs::create_dir_all(dir)?;
        let ch = usize::from(self.channels).max(1);
        let mut out = Vec::with_capacity(
            HEADER_BYTES
                + self
                    .levels
                    .iter()
                    .map(|l| 12 + 2 * l.peaks.len())
                    .sum::<usize>(),
        );
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&self.channels.to_le_bytes());
        out.extend_from_slice(&(self.levels.len() as u16).to_le_bytes());
        out.extend_from_slice(&self.scale.to_le_bytes());
        out.extend_from_slice(&self.frames.to_le_bytes());
        let mut h = [0u8; HASH_BYTES];
        for (d, s) in h.iter_mut().zip(self.hash.bytes()) {
            *d = s;
        }
        out.extend_from_slice(&h);
        let n = self.stamps.len().min(MAX_STAMPS);
        out.extend_from_slice(&(n as u32).to_le_bytes());
        for i in 0..MAX_STAMPS {
            let s = self.stamps.get(i).copied().unwrap_or(Stamp {
                len: 0,
                mtime_s: 0,
                mtime_ns: 0,
            });
            out.extend_from_slice(&s.len.to_le_bytes());
            out.extend_from_slice(&s.mtime_s.to_le_bytes());
            out.extend_from_slice(&s.mtime_ns.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
        }
        for l in &self.levels {
            out.extend_from_slice(&l.frames_per_peak.to_le_bytes());
            out.extend_from_slice(&((l.peaks.len() / (2 * ch)) as u64).to_le_bytes());
            for v in &l.peaks {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let tmp = dir.join(format!(
            ".{name}.{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // Flush the bytes before the rename, so a crash never leaves a
        // valid header over a zeroed body.
        let r = (|| {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&out)?;
            f.sync_all()?;
            std::fs::rename(&tmp, path)
        })();
        if let Err(e) = r {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        // Best effort: make the rename itself durable.
        if let Ok(d) = std::fs::File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(())
    }

    /// Reads and checks the overview at `path`: its size must be exactly
    /// what its header implies, every level as [`LEVELS`] says; anything
    /// else is `Unsupported` ("damaged overview"), never a panic.
    pub fn read(path: &Path) -> Result<Overview, MediaError> {
        let bad = |why: &str| {
            MediaError::Unsupported(format!("{}: damaged overview ({why})", path.display()))
        };
        let mut f = std::fs::File::open(path)?;
        let len = f.metadata()?.len();
        let mut head = [0u8; HEADER_BYTES];
        f.read_exact(&mut head).map_err(|_| bad("short"))?;
        let h = parse_header(&head).map_err(bad)?;
        let body = body_bytes(h.channels, h.frames).ok_or_else(|| bad("size"))?;
        if len != HEADER_BYTES as u64 + body {
            return Err(bad("size"));
        }
        let mut rest = Vec::with_capacity(body as usize);
        f.read_to_end(&mut rest)?;
        let mut c = Cur { b: &rest, at: 0 };
        let ch = usize::from(h.channels);
        let mut levels = Vec::with_capacity(LEVELS.len());
        for fpp in LEVELS {
            if c.u32() != Some(fpp) {
                return Err(bad("level"));
            }
            let count = c.u64().ok_or_else(|| bad("short"))?;
            if count != h.frames.div_ceil(u64::from(fpp)) {
                return Err(bad("peak count"));
            }
            let n = count as usize * ch * 2;
            let mut peaks = Vec::with_capacity(n);
            for _ in 0..n {
                peaks.push(i16::from_le_bytes(c.take().ok_or_else(|| bad("short"))?));
            }
            levels.push(Level {
                frames_per_peak: fpp,
                peaks,
            });
        }
        Ok(Overview {
            channels: h.channels,
            frames: h.frames,
            scale: h.scale,
            hash: h.hash,
            stamps: h.stamps,
            levels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caf::{CafWriter, WriteMode, hash_file};
    use core_model::format::BitDepth;

    /// A stereo float CAF of `frames` frames: channel 0 a slow ramp,
    /// channel 1 a faster one at half level, with `spikes` set.
    fn file(
        dir: &Path,
        name: &str,
        frames: usize,
        spikes: &[(usize, usize, f32)],
    ) -> (PathBuf, Vec<f32>) {
        let mut s = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            s.push(((f % 1000) as f32 / 1000.0 - 0.5) * 0.8);
            s.push(((f % 77) as f32 / 77.0 - 0.5) * 0.5);
        }
        for (f, c, v) in spikes {
            s[f * 2 + c] = *v;
        }
        let p = dir.join(name);
        let mut w = CafWriter::create(&p, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
        w.write(&s).unwrap();
        w.finish().unwrap();
        (p, s)
    }

    fn brute(s: &[f32], c: usize, from: usize, to: usize) -> (f32, f32) {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for f in from..to {
            lo = lo.min(s[f * 2 + c]);
            hi = hi.max(s[f * 2 + c]);
        }
        (lo, hi)
    }

    fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    #[test]
    fn levels_match_a_brute_force_min_max() {
        let dir = tempfile::tempdir().unwrap();
        // 70 001 frames: every level ends in a partial peak.
        let (p, s) = file(dir.path(), "a.caf", 70_001, &[]);
        let o = build(&p, &no_cancel()).unwrap();
        assert_eq!((o.channels, o.frames, o.scale), (2, 70_001, 1.0));
        assert_eq!(o.hash, hash_file(&p).unwrap());
        assert_eq!(o.levels.len(), 3);
        for l in &o.levels {
            let fpp = l.frames_per_peak as usize;
            let n = 70_001usize.div_ceil(fpp);
            assert_eq!(l.peaks.len(), n * 2 * 2, "level {fpp}");
            for p in 0..n {
                for c in 0..2 {
                    let (lo, hi) = brute(&s, c, p * fpp, ((p + 1) * fpp).min(70_001));
                    let at = (p * 2 + c) * 2;
                    assert_eq!(
                        l.peaks[at],
                        quantize(lo, 1.0, false),
                        "level {fpp} peak {p}"
                    );
                    assert_eq!(
                        l.peaks[at + 1],
                        quantize(hi, 1.0, true),
                        "level {fpp} peak {p}"
                    );
                }
            }
        }
    }

    #[test]
    fn float_overs_scale_the_peaks() {
        let dir = tempfile::tempdir().unwrap();
        let (p, _) = file(
            dir.path(),
            "loud.caf",
            5000,
            &[(1234, 1, 2.0), (4000, 0, -1.5)],
        );
        let o = build(&p, &no_cancel()).unwrap();
        assert_eq!(o.scale, 2.0);
        let (lo, hi) = o.range(1, 1200, 1300).unwrap();
        assert!(hi >= 2.0 && hi - 2.0 < 1e-4, "{hi}");
        assert!(lo <= -0.25);
        let (lo, _) = o.range(0, 3900, 4100).unwrap();
        assert!(lo <= -1.5 && lo > -1.5 - 1e-4, "{lo}");
    }

    #[test]
    fn write_then_read_is_identical_and_stamps_keep_the_newest_eight() {
        let dir = tempfile::tempdir().unwrap();
        let (p, _) = file(dir.path(), "b.caf", 9000, &[]);
        let mut o = build(&p, &no_cancel()).unwrap();
        for i in 0..10 {
            o = o.with_stamp(Stamp {
                len: i,
                mtime_s: i as i64,
                mtime_ns: 0,
            });
        }
        assert_eq!(o.stamps.len(), MAX_STAMPS);
        assert_eq!(o.stamps.first().unwrap().len, 2, "the oldest went");
        let at = Overview::path(dir.path(), &o.hash).unwrap();
        o.write(&at).unwrap();
        assert_eq!(Overview::read(&at).unwrap(), o);
        // Rewriting replaces it; no temp file is left.
        o.write(&at).unwrap();
        let left: Vec<_> = std::fs::read_dir(at.parent().unwrap()).unwrap().collect();
        assert_eq!(left.len(), 1);
        let st = Stamp::of(&p).unwrap();
        assert!(!o.has_stamp(&st));
        assert!(o.clone().with_stamp(st).has_stamp(&st));
    }

    /// Review Focus 3: a damaged or foreign overview is refused, never
    /// trusted and never a panic.
    #[test]
    fn damaged_overviews_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (p, _) = file(dir.path(), "c.caf", 3000, &[]);
        let o = build(&p, &no_cancel()).unwrap();
        let good = dir.path().join("good.vzov");
        o.write(&good).unwrap();
        let bytes = std::fs::read(&good).unwrap();
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("empty", vec![]),
            ("truncated", bytes[..bytes.len() - 1].to_vec()),
            ("extended", [bytes.clone(), vec![0]].concat()),
            ("header only", bytes[..HEADER_BYTES].to_vec()),
            ("magic", {
                let mut b = bytes.clone();
                b[0] = b'X';
                b
            }),
            ("version", {
                let mut b = bytes.clone();
                b[4] = 2;
                b
            }),
            ("channels", {
                let mut b = bytes.clone();
                b[8] = 3;
                b
            }),
            ("frames", {
                // 3000 → 7096 frames: the peak counts no longer fit the size.
                let mut b = bytes.clone();
                b[17] ^= 0x10;
                b
            }),
            ("hash with a slash", {
                let mut b = bytes.clone();
                b[24] = b'/';
                b
            }),
            ("uppercase hash", {
                let mut b = bytes.clone();
                b[24] = b'A';
                b
            }),
            ("stamp count", {
                let mut b = bytes.clone();
                b[88] = 9;
                b
            }),
            ("level", {
                let mut b = bytes.clone();
                b[HEADER_BYTES] = 1;
                b
            }),
        ];
        for (why, b) in cases {
            let at = dir.path().join(format!("{why}.vzov"));
            std::fs::write(&at, &b).unwrap();
            assert!(Overview::read(&at).is_err(), "{why} was accepted");
        }
        // Random bytes of random lengths never panic.
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for i in 0..2000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let len = (x % 700) as usize;
            let mut b: Vec<u8> = (0..len).map(|k| (x >> (k % 56)) as u8).collect();
            if i % 2 == 0 && b.len() >= 8 {
                b[..8].copy_from_slice(&bytes[..8]);
            }
            let at = dir.path().join("random.vzov");
            std::fs::write(&at, &b).unwrap();
            let _ = Overview::read(&at);
        }
    }

    #[test]
    fn hashes_name_files_only_when_lowercase_hex() {
        let pkg = Path::new("/p.vizdaw");
        assert_eq!(
            Overview::path(pkg, "0af9"),
            Some(PathBuf::from("/p.vizdaw/Media/.overviews/0af9.vzov"))
        );
        let long = "a".repeat(65);
        for bad in ["", "../../evil", "AB12", "12 3", long.as_str(), "x.vzov"] {
            assert_eq!(Overview::path(pkg, bad), None, "{bad:?}");
        }
    }

    #[test]
    fn ranges_contain_their_samples_and_bins_are_exact() {
        let dir = tempfile::tempdir().unwrap();
        let (p, s) = file(dir.path(), "d.caf", 300_000, &[]);
        let o = build(&p, &no_cancel()).unwrap();
        let r = CafReader::open(&p).unwrap();
        let mut x: u64 = 7;
        for _ in 0..100 {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let from = (x >> 33) % 299_000;
            let len = 1 + (x >> 7) % 100_000;
            let to = (from + len).min(300_000);
            for c in 0..2u16 {
                let (blo, bhi) = brute(&s, c as usize, from as usize, to as usize);
                let (lo, hi) = o.range(c, from, to).unwrap();
                assert!(lo <= blo + 1e-6 && hi >= bhi - 1e-6, "{from}..{to}");
            }
            let step = 1 + (x >> 40) % 300;
            let short_to = (from + 600).min(300_000);
            let b = Bins::read(&r, from, short_to, step).unwrap();
            for k in 0..2u16 {
                let mut f = from;
                while f < short_to {
                    let e = (f + step).min(short_to);
                    assert_eq!(
                        b.range(k, f, e),
                        Some(brute(&s, k as usize, f as usize, e as usize))
                    );
                    f = e;
                }
            }
        }
        assert_eq!(o.range(0, 300_000, 300_100), None);
        assert_eq!(o.range(2, 0, 10), None);
    }

    #[test]
    fn the_peak_counts_only_its_span() {
        let dir = tempfile::tempdir().unwrap();
        // A loud spike at frame 50 lies outside [100, 1100); inside, the
        // loudest sample is 0.6 at frame 600.
        let (p, _) = file(dir.path(), "e.caf", 5000, &[(50, 0, 0.95), (600, 1, -0.6)]);
        let o = build(&p, &no_cancel()).unwrap();
        let r = CafReader::open(&p).unwrap();
        let mut exact = |a: u64, b: u64| exact_peak(&r, a, b);
        let got = o.peak(100, 1100, &mut exact).unwrap();
        assert!(got >= 0.6 && got - 0.6 < 1e-4, "{got}");
        // A span inside one block is read exactly.
        assert_eq!(o.peak(590, 610, &mut exact).unwrap(), 0.6);
        assert_eq!(o.peak(10, 10, &mut exact).unwrap(), 0.0);
        assert!(o.peak(0, 5000, &mut exact).unwrap() >= 0.95);
    }

    #[test]
    fn a_build_stops_on_cancel_and_survives_a_cut_file() {
        let dir = tempfile::tempdir().unwrap();
        let (p, _) = file(dir.path(), "f.caf", 100_000, &[]);
        assert_eq!(
            build(&p, &AtomicBool::new(true)).unwrap_err(),
            MediaError::Cancelled
        );
        // Cut the file mid-data: the overview covers what is there.
        let len = std::fs::metadata(&p).unwrap().len();
        let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
        f.set_len(len - 40_003).unwrap();
        let o = build(&p, &no_cancel()).unwrap();
        assert!(o.frames < 100_000 && o.frames > 90_000, "{}", o.frames);
        assert_eq!(o.hash, hash_file(&p).unwrap());
    }
}
