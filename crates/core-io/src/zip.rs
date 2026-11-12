//! The package ZIP (`.dawproject`, spec §6.5): a reader that refuses hostile
//! archives before they cost memory or time, and a deterministic writer.
//!
//! The reader opens a regular file only, refuses names that leave the
//! package, duplicate names, links, entries whose data overlap and more
//! entries than [`PackageLimits::max_entries`] (from the end record's claim
//! before the directory is walked, and again while walking it). Reading an
//! entry is bounded by a byte limit, an optional packing ratio and the size
//! the entry declares; every inflated byte is counted and its CRC checked.

use crate::limits::{Limits, MAX_ENTRY_NAME_BYTES, PackageLimits};
use crate::read::open_regular;
use crate::{IoError, io_err};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// The refusal for a file that is not a ZIP at all.
pub const NOT_DAWPROJECT: &str = "This isn't a DAWproject file.";
/// The refusal for an entry larger than its reader allows.
pub const TOO_LARGE: &str = "This DAWproject file is too large to open.";

/// The reason fragment for every name rule.
const LEAVES: &str = "an entry name leaves the package";
/// Bytes moved per step when reading, extracting and writing.
const CHUNK: usize = 64 * 1024;

/// How an entry's bytes are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryMethod {
    /// As they are (method 0).
    Stored,
    /// DEFLATE (method 8).
    Deflated,
}

/// One file entry of an opened package, as its central record claims it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    /// The entry's name, checked by [`check_name`].
    pub name: String,
    /// How it is stored; `None` for a method VizDAW can't read.
    pub method: Option<EntryMethod>,
    /// Whether the entry is encrypted.
    pub encrypted: bool,
    /// Its declared unpacked size, in bytes.
    pub size: u64,
    /// Its packed size, in bytes.
    pub compressed: u64,
    crc: u32,
    way: rawzip::ZipArchiveEntryWayfinder,
}

/// An opened package: the archive over the file and its checked entries.
pub struct ZipReader {
    archive: rawzip::ZipArchive<rawzip::FileReader>,
    entries: Vec<EntryInfo>,
    #[cfg(feature = "test-hooks")]
    hooks: Hooks,
}

/// Test hooks (`test-hooks` feature only).
#[cfg(feature = "test-hooks")]
#[derive(Debug, Default)]
struct Hooks {
    cancel_at_chunk: Option<u64>,
    fail_removal: bool,
}

impl std::fmt::Debug for ZipReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZipReader")
            .field("entries", &self.entries)
            .finish_non_exhaustive()
    }
}

/// A whole-file refusal: "This DAWproject file is damaged: <why>."
pub(crate) fn damaged(why: impl std::fmt::Display) -> IoError {
    IoError::Refused(format!("This DAWproject file is damaged: {why}."))
}

/// `name` as a message may show it.
fn shown(name: &str) -> String {
    crate::text::sanitize(name.as_bytes(), &Limits::default()).0
}

/// `n` with a space between each group of three digits ("10 000").
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// Ruling 18's name rules: not empty, at most [`MAX_ENTRY_NAME_BYTES`]
/// bytes, no NUL or `\`, no leading `/`, no `:` in the first component, and
/// no empty, `.` or `..` component. `Err` is the reason fragment.
pub fn check_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty()
        || name.len() > MAX_ENTRY_NAME_BYTES
        || name.contains(['\0', '\\'])
        || name.starts_with('/')
        || name
            .split('/')
            .next()
            .is_some_and(|first| first.contains(':'))
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(LEAVES);
    }
    Ok(())
}

/// True for a central record with a Unix "version made by" whose mode is a
/// symbolic link.
fn is_link(rec: &rawzip::ZipFileHeaderRecord<'_>) -> bool {
    let system = rec.version_made_by().creator_system();
    (system == rawzip::CreatorSystem::UNIX || system == rawzip::CreatorSystem::MACOS)
        && (rec.external_attributes() >> 16) & 0o170_000 == 0o120_000
}

/// An I/O error kind that means the archive's bytes are wrong or short,
/// not that the operating system failed.
fn is_data_error(kind: std::io::ErrorKind) -> bool {
    use std::io::ErrorKind as K;
    matches!(
        kind,
        K::UnexpectedEof | K::InvalidData | K::InvalidInput | K::Other
    )
}

/// A `rawzip` error while finding or listing entries.
fn listing_error(path: &Path, e: rawzip::Error) -> IoError {
    match e.into_kind() {
        rawzip::ErrorKind::MissingEndOfCentralDirectory => {
            IoError::Refused(NOT_DAWPROJECT.to_owned())
        }
        rawzip::ErrorKind::IO(io) if !is_data_error(io.kind()) => io_err(path, "read", &io),
        _ => damaged("its list of files can't be read"),
    }
}

impl ZipReader {
    /// Opens the package at `path` (a regular file, see [`open_regular`])
    /// and checks every central record: names, duplicates, links, overlap
    /// and the entry count. Directory entries are checked, then skipped.
    /// Encrypted entries and unknown methods open; [`ZipReader::read`] and
    /// [`ZipReader::extract`] refuse them.
    pub fn open(path: &Path, limits: &PackageLimits) -> Result<ZipReader, IoError> {
        let file = open_regular(path)?;
        let mut buf = vec![0u8; rawzip::RECOMMENDED_BUFFER_SIZE];
        let archive =
            rawzip::ZipArchive::from_file(file, &mut buf).map_err(|e| listing_error(path, e))?;
        let limit = limits.max_entries;
        let too_many = || damaged(format!("it holds more than {} files", grouped(limit)));
        // The end record's claim alone refuses, before any record is read.
        if archive.entries_hint() > limit as u64 {
            return Err(too_many());
        }
        let mut entries = Vec::new();
        let mut names = HashSet::new();
        let mut headers = Vec::new();
        let mut seen = 0usize;
        let mut records = archive.entries(&mut buf);
        while let Some(rec) = records.next_entry().map_err(|e| listing_error(path, e))? {
            seen = seen.saturating_add(1);
            if seen > limit {
                return Err(too_many());
            }
            let raw = rec.file_path().as_bytes();
            if raw.len() > MAX_ENTRY_NAME_BYTES {
                return Err(damaged(LEAVES));
            }
            let name = std::str::from_utf8(raw).map_err(|_| damaged(LEAVES))?;
            let dir = name.strip_suffix('/');
            check_name(dir.unwrap_or(name)).map_err(damaged)?;
            if is_link(&rec) {
                return Err(damaged("it holds a link"));
            }
            if !names.insert(name.to_owned()) {
                return Err(damaged(format!("two entries are named '{}'", shown(name))));
            }
            if dir.is_some() {
                continue;
            }
            let m = rec.compression_method();
            let method = if m == rawzip::CompressionMethod::STORE {
                Some(EntryMethod::Stored)
            } else if m == rawzip::CompressionMethod::DEFLATE {
                Some(EntryMethod::Deflated)
            } else {
                None
            };
            headers.push(rec.local_header_offset());
            entries.push(EntryInfo {
                name: name.to_owned(),
                method,
                encrypted: rec.flags().is_encrypted() || m == rawzip::CompressionMethod::AES,
                size: rec.uncompressed_size_hint(),
                compressed: rec.compressed_size_hint(),
                crc: rec.crc32(),
                way: rec.wayfinder(),
            });
        }
        // Each entry's local header and data must be its own: two records
        // sharing bytes are how overlapping bombs multiply their output. The
        // central directory is a span too, from its start to the end of all
        // inputs, so no entry's data may reach into or past it: a packed
        // size larger than the bytes really there would make the ratio
        // check meaningless.
        // Each span is (start, end, entry); the directory's has no entry.
        let mut spans: Vec<(u64, u64, Option<&EntryInfo>)> =
            Vec::with_capacity(entries.len().saturating_add(1));
        spans.push((archive.directory_offset(), u64::MAX, None));
        for (e, start) in entries.iter().zip(headers) {
            let found = archive
                .get_entry(e.way)
                .map_err(|_| damaged(format!("{} can't be found", shown(&e.name))))?;
            spans.push((start, found.compressed_data_range().1, Some(e)));
        }
        spans.sort_unstable_by_key(|&(start, end, _)| (start, end));
        for w in spans.windows(2) {
            if let [(_, end, a), (next, _, b)] = w
                && next < end
            {
                return Err(match (a, b) {
                    (Some(e), None) | (None, Some(e)) => {
                        damaged(format!("{} runs past the end of its data", shown(&e.name)))
                    }
                    _ => damaged("two entries share their data"),
                });
            }
        }
        Ok(ZipReader {
            archive,
            entries,
            #[cfg(feature = "test-hooks")]
            hooks: Hooks::default(),
        })
    }

    /// Test hook: once `chunk` chunks (1-based) of an entry have gone to
    /// their destination, sets the caller's cancel flag, as a user's cancel
    /// arriving at that moment would. `None` turns it off.
    #[cfg(feature = "test-hooks")]
    pub fn debug_cancel_at_chunk(&mut self, chunk: Option<u64>) {
        self.hooks.cancel_at_chunk = chunk;
    }

    /// Test hook: with `fail`, removing a failed extract's copy reports an
    /// error and leaves the copy where it is.
    #[cfg(feature = "test-hooks")]
    pub fn debug_fail_removal(&mut self, fail: bool) {
        self.hooks.fail_removal = fail;
    }

    /// Removes a failed extract's copy (or, under the hook, fails to).
    fn remove_copy(&self, to: &Path) -> std::io::Result<()> {
        #[cfg(feature = "test-hooks")]
        if self.hooks.fail_removal {
            return Err(std::io::Error::other("removal failed (test hook)"));
        }
        std::fs::remove_file(to)
    }

    /// The package's file entries, in directory order.
    pub fn entries(&self) -> &[EntryInfo] {
        &self.entries
    }

    /// The file entry named exactly `name`.
    pub fn find(&self, name: &str) -> Option<&EntryInfo> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Inflates `e` (CRC and size verified). Refused with `TOO_LARGE` past `max` bytes (S12), and,
    /// with `ratio`, when declared or actual size / compressed > ratio.
    /// The buffer grows to at most `min(max, declared) + 64 KiB`.
    pub fn read(
        &self,
        e: &EntryInfo,
        max: u64,
        ratio: Option<u64>,
        cancel: &AtomicBool,
    ) -> Result<Vec<u8>, IoError> {
        let cap =
            usize::try_from(max.min(e.size).saturating_add(CHUNK as u64)).unwrap_or(usize::MAX);
        let mut out = Vec::new();
        self.stream(e, Some(max), ratio, cancel, &mut |chunk| {
            let need = out.len().saturating_add(chunk.len());
            if need > out.capacity() {
                let want = out.capacity().saturating_mul(2).clamp(need, cap.max(need));
                out.try_reserve_exact(want.saturating_sub(out.len()))
                    .map_err(|_| IoError::Refused(TOO_LARGE.to_owned()))?;
            }
            out.extend_from_slice(chunk);
            Ok(())
        })?;
        Ok(out)
    }

    /// The first `prefix` bytes of `e` (all of it when shorter), with the
    /// size and ratio checks of [`ZipReader::read`] on the entry as a whole
    /// (refused with `TOO_LARGE` when it declares more than `max`). Unpacking
    /// stops at `prefix`, so the checksum of a longer entry is not checked.
    pub fn read_prefix(
        &self,
        e: &EntryInfo,
        max: u64,
        prefix: usize,
        ratio: Option<u64>,
        cancel: &AtomicBool,
    ) -> Result<Vec<u8>, IoError> {
        let mut out = Vec::new();
        let mut enough = false;
        let got = self.stream(e, Some(max), ratio, cancel, &mut |chunk| {
            let room = prefix.saturating_sub(out.len());
            out.extend_from_slice(chunk.get(..room.min(chunk.len())).unwrap_or_default());
            if out.len() >= prefix {
                // Stops the stream; the error is not passed on.
                enough = true;
                Err(IoError::Cancelled)
            } else {
                Ok(())
            }
        });
        match got {
            Err(_) if enough => Ok(out),
            Ok(_) => Ok(out),
            Err(e) => Err(e),
        }
    }

    /// Streams `e` into a new file `to` (`create_new`); `to` is removed on any error or cancel.
    /// Returns the bytes written, which never pass the size `e` declares.
    /// If `to` cannot be removed after a failure, the error is an `Io` that
    /// names the file left behind, in place of the failure's own.
    pub fn extract(&self, e: &EntryInfo, to: &Path, cancel: &AtomicBool) -> Result<u64, IoError> {
        readable(e)?;
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(to)
        {
            Ok(f) => f,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(IoError::Refused(format!(
                    "{} exists; it was left as it is",
                    core_media::export::display_name(to)
                )));
            }
            Err(err) => return Err(io_err(to, "written", &err)),
        };
        let r = self
            .stream(e, None, None, cancel, &mut |chunk| {
                file.write_all(chunk)
                    .map_err(|err| io_err(to, "written", &err))
            })
            .and_then(|n| {
                file.sync_all().map_err(|err| io_err(to, "written", &err))?;
                Ok(n)
            });
        if let Err(why) = r {
            drop(file);
            return match self.remove_copy(to) {
                Ok(()) => Err(why),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(why),
                Err(err) => Err(IoError::Io(format!(
                    "{} was left behind ({why}) and could not be removed: {err}",
                    core_media::export::display_name(to)
                ))),
            };
        }
        r
    }

    /// Unpacks `e` in chunks into `sink`, counting every byte against the
    /// declared size, `max` and `ratio`, and checking the CRC at the end.
    fn stream(
        &self,
        e: &EntryInfo,
        max: Option<u64>,
        ratio: Option<u64>,
        cancel: &AtomicBool,
        sink: &mut dyn FnMut(&[u8]) -> Result<(), IoError>,
    ) -> Result<u64, IoError> {
        let method = readable(e)?;
        let name = shown(&e.name);
        let too_large = || IoError::Refused(TOO_LARGE.to_owned());
        let packed_too_tight =
            |r: u64| damaged(format!("{name} unpacks to more than {r} times its size"));
        let packed_limit = |r: u64| e.compressed.saturating_mul(r);
        if max.is_some_and(|m| e.size > m) {
            return Err(too_large());
        }
        if let Some(r) = ratio
            && e.size > packed_limit(r)
        {
            return Err(packed_too_tight(r));
        }
        let entry = self
            .archive
            .get_entry(e.way)
            .map_err(|_| damaged(format!("{name} can't be found")))?;
        let raw = entry.reader();
        let mut source: Box<dyn Read + '_> = match method {
            EntryMethod::Stored => Box::new(raw),
            EntryMethod::Deflated => Box::new(flate2::read::DeflateDecoder::new(raw)),
        };
        let mut buf = vec![0u8; CHUNK];
        let mut count: u64 = 0;
        let mut crc = rawzip::Crc32::new();
        #[cfg(feature = "test-hooks")]
        let mut chunks: u64 = 0;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(IoError::Cancelled);
            }
            let n = match source.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(err) if is_data_error(err.kind()) => {
                    return Err(damaged(format!("{name} can't be unpacked")));
                }
                Err(err) => return Err(IoError::Io(err.to_string())),
            };
            let chunk = buf.get(..n).unwrap_or_default();
            count = count.saturating_add(n as u64);
            if count > e.size {
                return Err(damaged(format!("{name} is larger than it says")));
            }
            if max.is_some_and(|m| count > m) {
                return Err(too_large());
            }
            if let Some(r) = ratio
                && count > packed_limit(r)
            {
                return Err(packed_too_tight(r));
            }
            crc.update(chunk);
            sink(chunk)?;
            #[cfg(feature = "test-hooks")]
            {
                chunks = chunks.saturating_add(1);
                if self.hooks.cancel_at_chunk == Some(chunks) {
                    cancel.store(true, Ordering::Relaxed);
                }
            }
        }
        if count < e.size {
            return Err(damaged(format!("{name} is cut short")));
        }
        if crc.checksum() != e.crc {
            return Err(damaged(format!("{name} fails its checksum")));
        }
        Ok(count)
    }
}

/// `e`'s method, or the sentence for an entry VizDAW can't read.
fn readable(e: &EntryInfo) -> Result<EntryMethod, IoError> {
    if e.encrypted {
        return Err(IoError::Refused(format!(
            "{} is encrypted.",
            shown(&e.name)
        )));
    }
    e.method.ok_or_else(|| {
        IoError::Refused(format!(
            "{} uses a compression VizDAW can't read.",
            shown(&e.name)
        ))
    })
}

/// Writes a package. Entries carry no modification time, so equal entries
/// give equal bytes. After an error or a cancel the output is incomplete
/// and must be discarded.
pub struct ZipWriter<W: Write> {
    archive: rawzip::ZipArchiveWriter<W>,
    names: HashSet<String>,
}

impl<W: Write> std::fmt::Debug for ZipWriter<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZipWriter")
            .field("names", &self.names)
            .finish_non_exhaustive()
    }
}

fn write_error(e: impl std::fmt::Display) -> IoError {
    IoError::Io(format!("The DAWproject file could not be written: {e}"))
}

impl<W: Write> ZipWriter<W> {
    /// A writer with no entries yet.
    pub fn new(out: W) -> Self {
        Self {
            archive: rawzip::ZipArchiveWriter::new(out),
            names: HashSet::new(),
        }
    }

    /// Adds the entry `name` holding `bytes`.
    pub fn add(&mut self, name: &str, method: EntryMethod, bytes: &[u8]) -> Result<(), IoError> {
        let mut src = bytes;
        self.add_from(name, method, &mut src, &AtomicBool::new(false))
            .map(|_| ())
    }

    /// Adds the entry `name` holding everything `src` gives, read in 64 KiB
    /// chunks; returns the unpacked size. A name [`check_name`] refuses, or
    /// one already added, is refused before anything is written.
    pub fn add_from(
        &mut self,
        name: &str,
        method: EntryMethod,
        src: &mut dyn Read,
        cancel: &AtomicBool,
    ) -> Result<u64, IoError> {
        check_name(name).map_err(|why| {
            IoError::Refused(format!("'{}' can't be written: {why}", shown(name)))
        })?;
        if self.names.contains(name) {
            return Err(IoError::Refused(format!(
                "Two entries would be named '{}'",
                shown(name)
            )));
        }
        let (mut entry, config) = self
            .archive
            .new_file(name)
            .compression_method(match method {
                EntryMethod::Stored => rawzip::CompressionMethod::STORE,
                EntryMethod::Deflated => rawzip::CompressionMethod::DEFLATE,
            })
            .start()
            .map_err(write_error)?;
        let n = match method {
            EntryMethod::Stored => {
                let mut data = config.wrap(&mut entry);
                let n = copy_chunks(src, &mut data, cancel)?;
                let (_, descriptor) = data.finish().map_err(write_error)?;
                entry.finish(descriptor).map_err(write_error)?;
                n
            }
            EntryMethod::Deflated => {
                let encoder =
                    flate2::write::DeflateEncoder::new(&mut entry, flate2::Compression::default());
                let mut data = config.wrap(encoder);
                let n = copy_chunks(src, &mut data, cancel)?;
                let (encoder, descriptor) = data.finish().map_err(write_error)?;
                encoder.finish().map_err(write_error)?;
                entry.finish(descriptor).map_err(write_error)?;
                n
            }
        };
        self.names.insert(name.to_owned());
        Ok(n)
    }

    /// Writes the central directory and returns the output.
    pub fn finish(self) -> Result<W, IoError> {
        self.archive.finish().map_err(write_error)
    }
}

/// Copies `src` into `dst` in 64 KiB chunks, checking `cancel` before each.
fn copy_chunks(
    src: &mut dyn Read,
    dst: &mut dyn Write,
    cancel: &AtomicBool,
) -> Result<u64, IoError> {
    let mut buf = vec![0u8; CHUNK];
    let mut total: u64 = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(IoError::Cancelled);
        }
        let n = match src.read(&mut buf) {
            Ok(0) => return Ok(total),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(write_error(e)),
        };
        dst.write_all(buf.get(..n).unwrap_or_default())
            .map_err(write_error)?;
        total = total.saturating_add(n as u64);
    }
}
