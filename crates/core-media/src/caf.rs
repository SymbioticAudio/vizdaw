//! CAF files (sub-project 5 spec §5.2): little-endian linear PCM, 1–2
//! channels, int16, int24 or float32, as VizDAW writes them. The writer
//! starts the `data` chunk with size −1 ("to end of file") and patches the
//! size on `finish`, so a crash leaves a valid file. The reader accepts
//! only that, reads with positioned reads (no memory map: a file truncated
//! or replaced underneath yields short reads or errors, never SIGBUS), and
//! converts to float.

use crate::MediaError;
use core_model::format::BitDepth;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::FileExt;
use std::path::Path;

/// The most channels a VizDAW CAF holds (wider files are split on import).
pub const MAX_CHANNELS: u16 = 2;

const FLAG_FLOAT: u32 = 1;
const FLAG_LITTLE_ENDIAN: u32 = 2;
const TAG_MONO: u32 = (100 << 16) | 1;
const TAG_STEREO: u32 = (101 << 16) | 2;
const RECORDING_KEY: &[u8] = b"vizdaw-recording";
const INT16_SCALE: f32 = 32_768.0;
const INT24_SCALE: f32 = 8_388_608.0;

/// How a file is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// An ordinary media file.
    Normal,
    /// A recording: an `info` chunk notes `vizdaw-recording` (crash
    /// recovery finds such files whose size was never patched).
    Recording,
}

/// What a CAF file holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CafInfo {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// 1 or 2.
    pub channels: u16,
    /// Sample format.
    pub depth: BitDepth,
    /// Whole frames present (the declared size, cut to the file).
    pub frames: u64,
    /// File offset of the first sample byte.
    pub data_offset: u64,
    /// The `data` size was patched (false: −1, e.g. after a crash).
    pub size_known: bool,
    /// The file carries the recording mark.
    pub recording: bool,
}

impl CafInfo {
    /// Bytes per frame.
    pub fn frame_bytes(&self) -> usize {
        self.depth.bytes() * usize::from(self.channels)
    }
}

/// What a finished writer wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// Frames written.
    pub frames: u64,
    /// BLAKE3 hex of the sample bytes.
    pub content_hash: String,
    /// Samples clipped at full scale (integer depths).
    pub clipped: u64,
}

/// Triangular (TPDF) dither of ±1 LSB from a deterministic xorshift64*.
struct Tpdf(u64);

impl Tpdf {
    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let x = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        // The top 24 bits as a fraction in [0, 1).
        (x >> 40) as f32 / 16_777_216.0
    }

    fn next(&mut self) -> f32 {
        self.unit() + self.unit() - 1.0
    }
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], size: i64) {
    out.extend_from_slice(kind);
    out.extend_from_slice(&size.to_be_bytes());
}

/// Writes a VizDAW CAF file.
pub struct CafWriter {
    file: File,
    channels: u16,
    depth: BitDepth,
    size_at: u64,
    frames: u64,
    clipped: u64,
    hasher: blake3::Hasher,
    /// A write failed: the file may hold bytes the counts leave out.
    poisoned: bool,
    dither: Tpdf,
    bytes: Vec<u8>,
}

impl std::fmt::Debug for CafWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CafWriter")
            .field("channels", &self.channels)
            .field("depth", &self.depth)
            .field("frames", &self.frames)
            .finish_non_exhaustive()
    }
}

impl CafWriter {
    /// Creates `path` (never overwriting) for `channels` (1–2) at `rate`
    /// in `depth`.
    pub fn create(
        path: &Path,
        rate: u32,
        channels: u16,
        depth: BitDepth,
        mode: WriteMode,
    ) -> Result<CafWriter, MediaError> {
        if !(1..=MAX_CHANNELS).contains(&channels) {
            return Err(MediaError::Unsupported(format!(
                "{channels} channels (VizDAW CAF files hold 1 or 2)"
            )));
        }
        if rate == 0 {
            return Err(MediaError::Unsupported("a sample rate of 0".into()));
        }
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let mut h: Vec<u8> = Vec::with_capacity(128);
        h.extend_from_slice(b"caff");
        h.extend_from_slice(&1u16.to_be_bytes());
        h.extend_from_slice(&0u16.to_be_bytes());
        chunk(&mut h, b"desc", 32);
        h.extend_from_slice(&f64::from(rate).to_be_bytes());
        h.extend_from_slice(b"lpcm");
        let float = if depth == BitDepth::Float32 {
            FLAG_FLOAT
        } else {
            0
        };
        h.extend_from_slice(&(FLAG_LITTLE_ENDIAN | float).to_be_bytes());
        let frame_bytes = depth.bytes() as u32 * u32::from(channels);
        h.extend_from_slice(&frame_bytes.to_be_bytes());
        h.extend_from_slice(&1u32.to_be_bytes());
        h.extend_from_slice(&u32::from(channels).to_be_bytes());
        h.extend_from_slice(&depth.bits().to_be_bytes());
        chunk(&mut h, b"chan", 12);
        let tag = if channels == 1 { TAG_MONO } else { TAG_STEREO };
        h.extend_from_slice(&tag.to_be_bytes());
        h.extend_from_slice(&0u32.to_be_bytes());
        h.extend_from_slice(&0u32.to_be_bytes());
        if mode == WriteMode::Recording {
            let mut body = 1u32.to_be_bytes().to_vec();
            body.extend_from_slice(RECORDING_KEY);
            body.push(0);
            body.extend_from_slice(b"1\0");
            chunk(&mut h, b"info", body.len() as i64);
            h.extend_from_slice(&body);
        }
        h.extend_from_slice(b"data");
        let size_at = h.len() as u64;
        h.extend_from_slice(&(-1i64).to_be_bytes());
        // Edit count.
        h.extend_from_slice(&0u32.to_be_bytes());
        file.write_all(&h)?;
        Ok(CafWriter {
            file,
            channels,
            depth,
            size_at,
            frames: 0,
            clipped: 0,
            hasher: blake3::Hasher::new(),
            poisoned: false,
            dither: Tpdf(0x9E37_79B9_7F4A_7C15),
            bytes: Vec::new(),
        })
    }

    /// Appends interleaved float frames (a length that is a whole number
    /// of frames).
    pub fn write(&mut self, interleaved: &[f32]) -> Result<(), MediaError> {
        let ch = usize::from(self.channels);
        if interleaved.len() % ch != 0 {
            return Err(MediaError::Unsupported(format!(
                "{} samples are not whole {ch}-channel frames",
                interleaved.len()
            )));
        }
        self.bytes.clear();
        self.bytes.reserve(interleaved.len() * self.depth.bytes());
        for &x in interleaved {
            let x = if x.is_finite() { x } else { 0.0 };
            match self.depth {
                BitDepth::Float32 => self.bytes.extend_from_slice(&x.to_le_bytes()),
                BitDepth::Int16 => {
                    let v = x * INT16_SCALE;
                    if !(-INT16_SCALE..=INT16_SCALE - 1.0).contains(&v) {
                        self.clipped += 1;
                    }
                    let s = (v + self.dither.next())
                        .round()
                        .clamp(-INT16_SCALE, INT16_SCALE - 1.0) as i16;
                    self.bytes.extend_from_slice(&s.to_le_bytes());
                }
                BitDepth::Int24 => {
                    let v = (x * INT24_SCALE).round();
                    if !(-INT24_SCALE..=INT24_SCALE - 1.0).contains(&v) {
                        self.clipped += 1;
                    }
                    let s = v.clamp(-INT24_SCALE, INT24_SCALE - 1.0) as i32;
                    let b = s.to_le_bytes();
                    self.bytes
                        .extend_from_slice(b.get(..3).unwrap_or(&[0, 0, 0]));
                }
            }
        }
        if self.poisoned {
            return Err(MediaError::Io("an earlier write failed".into()));
        }
        // Hash only what reached the file.
        if let Err(e) = self.file.write_all(&self.bytes) {
            self.poisoned = true;
            return Err(e.into());
        }
        self.hasher.update(&self.bytes);
        self.frames += (interleaved.len() / ch) as u64;
        Ok(())
    }

    /// Frames written so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Test hook: makes every later write fail (the file is swapped for a
    /// read-only handle), as a full disk would.
    #[cfg(test)]
    pub(crate) fn break_file_for_test(&mut self, path: &Path) {
        if let Ok(f) = File::open(path) {
            self.file = f;
        }
    }

    #[cfg(test)]
    pub(crate) fn hash_hex_for_test(&self) -> String {
        self.hasher.finalize().to_hex().to_string()
    }

    /// Samples clipped so far.
    pub fn clipped(&self) -> u64 {
        self.clipped
    }

    /// Flushes what was written to the disk (a recording does this every
    /// 2 s, spec §6.2); the data size stays −1 until [`CafWriter::finish`].
    pub fn sync(&self) -> Result<(), MediaError> {
        self.file.sync_data()?;
        Ok(())
    }

    /// Patches the data size and syncs the file.
    pub fn finish(mut self) -> Result<Written, MediaError> {
        let bytes = self.frames * (self.depth.bytes() as u64) * u64::from(self.channels);
        let size = i64::try_from(bytes.saturating_add(4)).unwrap_or(-1);
        // Data first, so a crash cannot persist a size beyond the data.
        let synced = self.file.sync_data();
        self.file.seek(SeekFrom::Start(self.size_at))?;
        self.file.write_all(&size.to_be_bytes())?;
        self.file.sync_all()?;
        if self.poisoned {
            // The size now covers whole frames written; the take is
            // readable, but its hash and counts are not to be trusted.
            return Err(MediaError::Io(
                "a write failed; the file holds only the frames before it".into(),
            ));
        }
        synced?;
        Ok(Written {
            frames: self.frames,
            content_hash: self.hasher.finalize().to_hex().to_string(),
            clipped: self.clipped,
        })
    }
}

/// Reads a VizDAW CAF file.
#[derive(Debug)]
pub struct CafReader {
    file: File,
    info: CafInfo,
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Reads exactly `buf.len()` bytes at `at`, or fewer at the end of the
/// file; returns how many.
fn read_full(file: &File, buf: &mut [u8], at: u64) -> Result<usize, MediaError> {
    let mut got = 0usize;
    while got < buf.len() {
        let rest = buf.get_mut(got..).unwrap_or(&mut []);
        match file.read_at(rest, at + got as u64) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(got)
}

impl CafReader {
    /// Opens and checks `path`.
    pub fn open(path: &Path) -> Result<CafReader, MediaError> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        let not = |why: &str| MediaError::NotCaf(format!("{}: {why}", path.display()));
        let mut head = [0u8; 8];
        if read_full(&file, &mut head, 0)? < 8 || head.get(..4) != Some(b"caff".as_slice()) {
            return Err(not("no CAF header"));
        }
        let mut pos = 8u64;
        let mut desc: Option<[u8; 32]> = None;
        let mut data: Option<(u64, i64)> = None;
        let mut recording = false;
        while pos + 12 <= len {
            let mut ch = [0u8; 12];
            read_full(&file, &mut ch, pos)?;
            let kind: [u8; 4] = ch
                .get(..4)
                .and_then(|k| k.try_into().ok())
                .unwrap_or([0; 4]);
            let size = i64::from_be_bytes(
                ch.get(4..12)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 8]),
            );
            let body = pos + 12;
            match &kind {
                b"desc" => {
                    let mut d = [0u8; 32];
                    if size != 32 || read_full(&file, &mut d, body)? < 32 {
                        return Err(not("damaged desc chunk"));
                    }
                    desc = Some(d);
                }
                b"info" if size > 0 && size < 4096 => {
                    let mut b = vec![0u8; size as usize];
                    read_full(&file, &mut b, body)?;
                    recording = b.windows(RECORDING_KEY.len()).any(|w| w == RECORDING_KEY);
                }
                b"data" => {
                    data = Some((body + 4, size));
                    break;
                }
                _ => {}
            }
            if size < 0 {
                return Err(not("a chunk of unknown size before the data"));
            }
            pos = body.saturating_add(size as u64);
        }
        let d = desc.ok_or_else(|| not("no desc chunk"))?;
        let (data_offset, size) = data.ok_or_else(|| not("no data chunk"))?;
        let rate = f64::from_be_bytes(d.get(..8).and_then(|x| x.try_into().ok()).unwrap_or([0; 8]));
        let field = |at| be_u32(&d, at).unwrap_or(0);
        let (format, flags, packet, per_packet, channels, bits) = (
            d.get(8..12).unwrap_or(&[]),
            field(12),
            field(16),
            field(20),
            field(24),
            field(28),
        );
        let unsupported =
            |what: String| MediaError::Unsupported(format!("{}: {what}", path.display()));
        if format != b"lpcm" {
            return Err(unsupported("not linear PCM".into()));
        }
        if flags & FLAG_LITTLE_ENDIAN == 0 {
            return Err(unsupported("big-endian samples".into()));
        }
        let depth = match (bits, flags & FLAG_FLOAT != 0) {
            (16, false) => BitDepth::Int16,
            (24, false) => BitDepth::Int24,
            (32, true) => BitDepth::Float32,
            (b, f) => {
                return Err(unsupported(format!(
                    "{b}-bit{}",
                    if f { " float" } else { "" }
                )));
            }
        };
        if !(1..=u32::from(MAX_CHANNELS)).contains(&channels) {
            return Err(unsupported(format!("{channels} channels")));
        }
        let frame_bytes = depth.bytes() as u64 * u64::from(channels);
        if per_packet != 1 || u64::from(packet) != frame_bytes {
            return Err(unsupported("not packed, one frame per packet".into()));
        }
        if !((1.0..=768_000.0).contains(&rate) && rate.fract() == 0.0) {
            return Err(unsupported(format!("a sample rate of {rate}")));
        }
        let present = len.saturating_sub(data_offset) / frame_bytes;
        let frames = if size >= 4 {
            ((size as u64 - 4) / frame_bytes).min(present)
        } else {
            present
        };
        Ok(CafReader {
            file,
            info: CafInfo {
                sample_rate: rate as u32,
                channels: channels as u16,
                depth,
                frames,
                data_offset,
                size_known: size >= 0,
                recording,
            },
        })
    }

    /// What the file holds.
    pub fn info(&self) -> &CafInfo {
        &self.info
    }

    /// Reads frames from `start` into `out` (interleaved float, as many
    /// whole frames as fit); returns the frames read, fewer at the end or
    /// when the file is shorter than it was. `scratch` is reused; on
    /// return its first `frames read × frame_bytes()` bytes are those
    /// frames' raw sample bytes (overviews hash them, plan 5b Task 2).
    pub fn read(
        &self,
        start: u64,
        out: &mut [f32],
        scratch: &mut Vec<u8>,
    ) -> Result<usize, MediaError> {
        let i = &self.info;
        let ch = usize::from(i.channels);
        let want = (out.len() / ch) as u64;
        let n = want.min(i.frames.saturating_sub(start)) as usize;
        if n == 0 {
            return Ok(0);
        }
        let fb = i.frame_bytes();
        scratch.resize(n * fb, 0);
        let at = i.data_offset + start * fb as u64;
        let got = read_full(&self.file, scratch, at)? / fb;
        let sb = i.depth.bytes();
        for (k, o) in out.iter_mut().take(got * ch).enumerate() {
            let b = scratch.get(k * sb..k * sb + sb).unwrap_or(&[]);
            *o = match (i.depth, b) {
                (BitDepth::Float32, [a, b, c, d]) => f32::from_le_bytes([*a, *b, *c, *d]),
                (BitDepth::Int16, [a, b]) => f32::from(i16::from_le_bytes([*a, *b])) / INT16_SCALE,
                (BitDepth::Int24, [a, b, c]) => {
                    // Sign-extend via the top byte.
                    (i32::from_le_bytes([0, *a, *b, *c]) >> 8) as f32 / INT24_SCALE
                }
                _ => 0.0,
            };
        }
        Ok(got)
    }
}

/// BLAKE3 hex of `path`'s sample bytes (the hash the writer reported).
pub fn hash_file(path: &Path) -> Result<String, MediaError> {
    let r = CafReader::open(path)?;
    let total = r.info.frames * r.info.frame_bytes() as u64;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    while done < total {
        let n = (total - done).min(buf.len() as u64) as usize;
        let chunk = buf.get_mut(..n).unwrap_or(&mut []);
        let got = read_full(&r.file, chunk, r.info.data_offset + done)?;
        h.update(chunk.get(..got).unwrap_or(&[]));
        if got < n {
            break;
        }
        done += n as u64;
    }
    Ok(h.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(frames: usize, channels: usize) -> Vec<f32> {
        (0..frames * channels)
            .map(|k| {
                let f = (k / channels) as f32;
                0.8 * (f * 0.01 + (k % channels) as f32).sin()
            })
            .collect()
    }

    fn write(path: &Path, depth: BitDepth, data: &[f32], ch: u16) -> Written {
        let mut w = CafWriter::create(path, 48_000, ch, depth, WriteMode::Normal).unwrap();
        w.write(data).unwrap();
        w.finish().unwrap()
    }

    fn read_all(path: &Path) -> (CafInfo, Vec<f32>) {
        let r = CafReader::open(path).unwrap();
        let info = r.info().clone();
        let mut out = vec![0.0; info.frames as usize * usize::from(info.channels)];
        let n = r.read(0, &mut out, &mut Vec::new()).unwrap();
        assert_eq!(n as u64, info.frames);
        (info, out)
    }

    #[test]
    fn round_trip_in_each_depth_mono_and_stereo() {
        let dir = tempfile::tempdir().unwrap();
        for (depth, tol) in [
            (BitDepth::Float32, 0.0),
            (BitDepth::Int24, 1.0 / INT24_SCALE),
            // Rounding plus up to ±1 LSB of dither.
            (BitDepth::Int16, 2.0 / INT16_SCALE),
        ] {
            for ch in [1u16, 2] {
                let p = dir.path().join(format!("{depth:?}-{ch}.caf"));
                let data = sine(1000, usize::from(ch));
                let w = write(&p, depth, &data, ch);
                assert_eq!((w.frames, w.clipped), (1000, 0));
                let (info, back) = read_all(&p);
                assert_eq!(
                    (info.sample_rate, info.channels, info.depth, info.size_known),
                    (48_000, ch, depth, true)
                );
                for (a, b) in data.iter().zip(&back) {
                    assert!((a - b).abs() <= tol, "{depth:?} {a} {b}");
                }
            }
        }
    }

    #[test]
    fn dither_only_at_16_bit() {
        let dir = tempfile::tempdir().unwrap();
        let silence = vec![0.0f32; 4096];
        for (depth, dithered) in [(BitDepth::Int16, true), (BitDepth::Int24, false)] {
            let p = dir.path().join(format!("{depth:?}.caf"));
            write(&p, depth, &silence, 1);
            let (_, back) = read_all(&p);
            assert_eq!(back.iter().any(|x| *x != 0.0), dithered, "{depth:?}");
            assert!(back.iter().all(|x| x.abs() <= 1.0 / INT16_SCALE));
        }
    }

    #[test]
    fn integer_depths_clip_and_count() {
        let dir = tempfile::tempdir().unwrap();
        let loud = [1.5f32, -2.0, 0.5, f32::NAN];
        let w = write(&dir.path().join("i24.caf"), BitDepth::Int24, &loud, 1);
        assert_eq!(w.clipped, 2);
        let (_, back) = read_all(&dir.path().join("i24.caf"));
        assert_eq!(
            back,
            vec![(INT24_SCALE - 1.0) / INT24_SCALE, -1.0, 0.5, 0.0]
        );
        let w = write(&dir.path().join("f.caf"), BitDepth::Float32, &loud, 1);
        assert_eq!(w.clipped, 0);
        let (_, back) = read_all(&dir.path().join("f.caf"));
        assert_eq!(back, vec![1.5, -2.0, 0.5, 0.0]);
    }

    #[test]
    fn an_unfinished_file_reads_to_its_end() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("crash.caf");
        let mut w =
            CafWriter::create(&p, 44_100, 2, BitDepth::Int24, WriteMode::Recording).unwrap();
        w.write(&sine(300, 2)).unwrap();
        // A crash: the writer goes without `finish`.
        drop(w);
        let r = CafReader::open(&p).unwrap();
        assert_eq!(
            (
                r.info().frames,
                r.info().size_known,
                r.info().recording,
                r.info().sample_rate
            ),
            (300, false, true, 44_100)
        );
    }

    #[test]
    fn truncated_file_reads_what_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.caf");
        write(&p, BitDepth::Int16, &sine(1000, 2), 2);
        let off = CafReader::open(&p).unwrap().info().data_offset;
        let f = OpenOptions::new().write(true).open(&p).unwrap();
        // 500 whole frames and half of the next.
        f.set_len(off + 500 * 4 + 2).unwrap();
        let r = CafReader::open(&p).unwrap();
        assert_eq!(r.info().frames, 500);
        let mut out = vec![0.0; 2000];
        assert_eq!(r.read(0, &mut out, &mut Vec::new()).unwrap(), 500);
        assert_eq!(r.read(499, &mut out, &mut Vec::new()).unwrap(), 1);
        assert_eq!(r.read(500, &mut out, &mut Vec::new()).unwrap(), 0);
        // Truncated under an open reader: short reads, no error.
        let r = CafReader::open(&dir.path().join("t.caf")).unwrap();
        f.set_len(off + 100 * 4).unwrap();
        assert_eq!(r.read(0, &mut out, &mut Vec::new()).unwrap(), 100);
    }

    #[test]
    fn a_failed_write_leaves_no_hash_over_missing_bytes_and_finish_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("full.caf");
        let mut w =
            CafWriter::create(&p, 48_000, 1, BitDepth::Int16, WriteMode::Recording).unwrap();
        w.write(&[0.25; 100]).unwrap();
        w.break_file_for_test(&p);
        assert!(w.write(&[0.5; 100]).is_err());
        assert_eq!(w.frames(), 100);
        let mut good = blake3::Hasher::new();
        good.update(&std::fs::read(&p).unwrap()[w_data_offset(&p)..]);
        assert_eq!(w.hash_hex_for_test(), good.finalize().to_hex().to_string());
        assert!(w.finish().is_err(), "a poisoned writer does not finish Ok");
    }

    fn w_data_offset(p: &Path) -> usize {
        CafReader::open(p).unwrap().info().data_offset as usize
    }

    #[test]
    fn other_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.wav");
        std::fs::write(&p, b"RIFF\0\0\0\0WAVEfmt this is not caf").unwrap();
        assert!(matches!(CafReader::open(&p), Err(MediaError::NotCaf(_))));
        std::fs::write(&p, b"").unwrap();
        assert!(matches!(CafReader::open(&p), Err(MediaError::NotCaf(_))));
        // A big-endian CAF: flip the little-endian flag of a good file.
        let good = dir.path().join("g.caf");
        write(&good, BitDepth::Int16, &sine(10, 1), 1);
        let mut bytes = std::fs::read(&good).unwrap();
        // desc body starts at 8 + 12; flags at +12, big-endian u32.
        bytes[20 + 12 + 3] &= !(FLAG_LITTLE_ENDIAN as u8);
        std::fs::write(&p, &bytes).unwrap();
        assert!(matches!(
            CafReader::open(&p),
            Err(MediaError::Unsupported(_))
        ));
        assert!(matches!(
            CafWriter::create(
                &dir.path().join("3.caf"),
                48_000,
                3,
                BitDepth::Int16,
                WriteMode::Normal
            ),
            Err(MediaError::Unsupported(_))
        ));
    }

    #[test]
    fn create_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.caf");
        write(&p, BitDepth::Int16, &sine(10, 1), 1);
        assert!(matches!(
            CafWriter::create(&p, 48_000, 1, BitDepth::Int16, WriteMode::Normal),
            Err(MediaError::Io(_))
        ));
    }

    #[test]
    fn the_hash_is_of_the_samples() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.caf");
        let b = dir.path().join("b.caf");
        let w = write(&a, BitDepth::Float32, &sine(100, 2), 2);
        assert_eq!(hash_file(&a).unwrap(), w.content_hash);
        assert_eq!(w.content_hash.len(), 64);
        let mut other = sine(100, 2);
        other[7] = 0.123;
        let w2 = write(&b, BitDepth::Float32, &other, 2);
        assert_ne!(w.content_hash, w2.content_hash);
    }
}
