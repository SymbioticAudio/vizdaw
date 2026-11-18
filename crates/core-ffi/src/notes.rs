//! The song's notes file and Co-Producer's one entry per request, dated
//! export folders, and the move of an untitled song's notes and copies
//! into its package on the first save (plan 8b Rulings 4–7).
//!
//! The notes are the composer's: Co-Producer only ever appends, and never
//! replaces, rewrites or deletes a file. Every file read here (the notes,
//! the copies, a remembered folder) is hostile input.

use crate::error::VizError;
use core_control::request::ExportWhat;
use core_model::diff::{self, ChangeItem};
use core_model::file::COPIES_DIR;
use core_model::state::STATE_DIR;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The notes file's name in a package or an untitled song's stand-in.
pub const NOTES_FILE: &str = "Notes.md";
/// The most bytes [`read`] gives back.
pub const MAX_READ: usize = 1 << 20;
/// Ruling 4's refusal of a `Notes.md` that is not a regular file.
pub const NOT_PLAIN: &str = "Notes.md is not a plain file, so Co-Producer leaves it alone.";
/// Ruling 6's answer for an untitled song with no remembered folder.
pub const ASK_FOR_FOLDER: &str =
    "Ask the composer where exports should go, then call export_folder with parent.";
/// The line that remembers a song's export folder (Ruling 6).
const EXPORT_LINE: &str = "- Export folder: ";
/// What heads an untitled song's notes appended to a package's own.
const BEFORE_FIRST_SAVE: &str = "## Notes from before the first save\n\n";
/// Items on one line of an entry, before "; and N more".
const PER_LINE: usize = 20;
/// The song's part of an export folder's name, in characters and bytes.
const SONG_CHARS: usize = 100;
const SONG_BYTES: usize = 180;
/// The last number an export folder's name may take.
const MAX_NUMBER: u32 = 999;

fn io(message: impl Into<String>) -> VizError {
    VizError::Io {
        message: message.into(),
    }
}

fn model(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

/// The notes as read: at most [`MAX_READ`] bytes, lossy UTF-8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notes {
    /// The text.
    pub text: String,
    /// The file is longer than what was read.
    pub truncated: bool,
    /// The file's path.
    pub path: PathBuf,
}

/// `text` on one line, as the entry shows it: every character
/// `diff::shown_name` would blank (controls, bidi controls and marks,
/// line and paragraph separators) is a space, so no text can start a line
/// of its own. Not shortened.
pub fn line_text(text: &str) -> String {
    let mut b = [0u8; 4];
    text.chars()
        .map(|c| diff::shown_name(c.encode_utf8(&mut b)))
        .collect()
}

/// Whether `path` is a regular file (`Ok(false)` when nothing is there);
/// a link, a folder or anything else is Ruling 4's refusal.
fn plain(path: &Path) -> Result<bool, VizError> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_file() => Ok(true),
        Ok(_) => Err(model(NOT_PLAIN)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io(format!("Notes.md cannot be checked ({e})."))),
    }
}

/// Makes a new file `path` holding `bytes`, whole or not at all, never
/// replacing one: a temporary file beside it is written and synced, then
/// linked into place (`AlreadyExists` when something is there). On a
/// volume without hard links it falls back to `create_new` and one write.
fn create_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("the path has no folder"))?;
    let name = path
        .file_name()
        .map_or_else(|| "file".to_string(), |n| n.to_string_lossy().into_owned());
    let tmp = dir.join(format!(
        ".{name}.{}-{}.new",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let write = |p: &Path| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(p)?;
        f.write_all(bytes)?;
        f.sync_all()
    };
    if let Err(e) = write(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    let linked = std::fs::hard_link(&tmp, path);
    let _ = std::fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Err(e),
        Err(_) => write(path),
    }
}

/// Ruling 4: the notes file in `home`, made holding `"# <song>\n"` when
/// absent (never replacing one); a link, a folder or any other non-regular
/// file is refused.
pub fn ensure(home: &Path, song: &str) -> Result<PathBuf, VizError> {
    let path = home.join(NOTES_FILE);
    if plain(&path)? {
        return Ok(path);
    }
    std::fs::create_dir_all(home)
        .map_err(|e| io(format!("The notes' folder could not be made ({e}).")))?;
    match create_new(&path, format!("# {}\n", line_text(song)).as_bytes()) {
        Ok(()) => {}
        // Another writer made it first: it is checked again below.
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
        Err(e) => return Err(io(format!("Notes.md could not be made ({e})."))),
    }
    if plain(&path)? {
        Ok(path)
    } else {
        Err(io("Notes.md could not be made."))
    }
}

/// Ruling 4: the notes in `home` (made first when absent): the first
/// [`MAX_READ`] bytes, decoded as lossy UTF-8.
pub fn read(home: &Path, song: &str) -> Result<Notes, VizError> {
    let path = ensure(home, song)?;
    let f =
        std::fs::File::open(&path).map_err(|e| io(format!("Notes.md could not be read ({e}).")))?;
    if !f.metadata().is_ok_and(|m| m.is_file()) {
        return Err(model(NOT_PLAIN));
    }
    let mut buf = Vec::new();
    f.take(MAX_READ as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| io(format!("Notes.md could not be read ({e}).")))?;
    let truncated = buf.len() > MAX_READ;
    buf.truncate(MAX_READ);
    Ok(Notes {
        text: String::from_utf8_lossy(&buf).into_owned(),
        truncated,
        path,
    })
}

/// Ruling 4's append of `text` to the notes in `home` (made first when
/// absent): after a newline when the file does not end in one, then a
/// blank line.
pub fn append(home: &Path, song: &str, text: &str) -> Result<(), VizError> {
    let path = ensure(home, song)?;
    append_to(&path, text.as_bytes())
}

/// Appends `text` to the regular file `path` by `O_APPEND`, so it never
/// truncates or rewrites whatever another editor wrote: a newline when
/// the file does not end in one, a blank line (none in an empty file),
/// `text`, and a final newline when `text` lacks one, in one write.
fn append_to(path: &Path, text: &[u8]) -> Result<(), VizError> {
    if !plain(path)? {
        return Err(io("Notes.md is gone."));
    }
    let failed = |e: std::io::Error| io(format!("Notes.md could not be written ({e})."));
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .append(true)
        .open(path)
        .map_err(failed)?;
    let m = f.metadata().map_err(failed)?;
    if !m.is_file() {
        return Err(model(NOT_PLAIN));
    }
    let mut block = Vec::with_capacity(text.len() + 3);
    if m.len() > 0 {
        f.seek(SeekFrom::End(-1)).map_err(failed)?;
        let mut last = [0u8; 1];
        f.read_exact(&mut last).map_err(failed)?;
        if last != *b"\n" {
            block.push(b'\n');
        }
        block.push(b'\n');
    }
    block.extend_from_slice(text);
    if !text.ends_with(b"\n") {
        block.push(b'\n');
    }
    f.write_all(&block).map_err(failed)?;
    f.sync_all().map_err(failed)
}

/// What one Co-Producer request did, kept from its begin to its end
/// (Ruling 5).
#[derive(Debug, Clone, Default)]
pub struct RequestLog {
    /// The first enter's stamp, "YYYY-MM-DD HH-MM"; `None` until the
    /// request enters.
    pub stamp: Option<String>,
    /// The stamp the app gave at begin or at the first note, file or
    /// export folder, for a request that never enters (fix round 1).
    pub begun: Option<String>,
    /// What the composer asked, in their words.
    pub asked: Option<String>,
    /// What Co-Producer's calls changed; `true` after a yes in chat.
    pub items: Vec<(ChangeItem, bool)>,
    /// The undo steps the request made.
    pub labels: Vec<String>,
    /// The files and folders it wrote.
    pub files: Vec<String>,
    /// Its notes.
    pub notes: Vec<String>,
    /// The export folder given, remembered for the song.
    pub export_folder: Option<String>,
    /// Its saved copy, relative to the package or stand-in.
    pub copy: Option<String>,
}

/// `items` joined by "; ", at most [`PER_LINE`], then "; and N more".
fn joined(items: &[String]) -> String {
    let mut out = items
        .iter()
        .take(PER_LINE)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    if items.len() > PER_LINE {
        out.push_str(&format!("; and {} more", items.len() - PER_LINE));
    }
    out
}

/// Ruling 5's entry for `log` of the request titled `title`, ending in a
/// newline. A request that entered is headed by its first enter's stamp.
/// One that never entered still writes an entry when it has notes, files
/// or an export folder (fix round 1 ruling), headed by `begun` (or by the
/// title alone when the app gave none), and has no "Saved copy:" line
/// unless a copy was written. `None` for a request that only read. Item lines are Added
/// (change "added"), Deleted (change starting "deleted") and Changed,
/// those after a yes on their own "(yes in chat)" line, in the order each
/// line's first item came. Every text is on one line ([`line_text`]), so
/// no line but the heading starts with `#`.
pub fn entry(log: &RequestLog, title: &str) -> Option<String> {
    let logged = !log.notes.is_empty() || !log.files.is_empty() || log.export_folder.is_some();
    let stamp = match log.stamp.as_deref() {
        Some(s) => Some(s),
        None if logged => log.begun.as_deref(),
        None => return None,
    };
    let mut out = match stamp {
        Some(stamp) => {
            let shown = match stamp.split_once(' ') {
                Some((date, time)) => format!("{date} {}", time.replacen('-', ":", 1)),
                None => stamp.to_string(),
            };
            format!("## {} — {}\n", line_text(&shown), line_text(title))
        }
        None => format!("## {}\n", line_text(title)),
    };
    if let Some(a) = &log.asked {
        out.push_str(&format!("- Asked: \"{}\"\n", line_text(a)));
    }
    let mut lines: Vec<(&str, bool, Vec<String>)> = Vec::new();
    for (item, yes) in &log.items {
        let kind = if item.change == "added" {
            "Added"
        } else if item.change.starts_with("deleted") {
            "Deleted"
        } else {
            "Changed"
        };
        let text = format!("{}: {}", line_text(&item.thing), line_text(&item.change));
        match lines.iter_mut().find(|l| l.0 == kind && l.1 == *yes) {
            Some(l) => l.2.push(text),
            None => lines.push((kind, *yes, vec![text])),
        }
    }
    for (kind, yes, items) in &lines {
        let yes = if *yes { " (yes in chat)" } else { "" };
        out.push_str(&format!("- {kind}{yes}: {}\n", joined(items)));
    }
    if log.items.is_empty() && log.files.is_empty() && log.notes.is_empty() {
        out.push_str("- Nothing changed.\n");
    }
    if !log.files.is_empty() {
        let files: Vec<String> = log.files.iter().map(|f| line_text(f)).collect();
        out.push_str(&format!("- Files: {}\n", joined(&files)));
    }
    if let Some(f) = &log.export_folder {
        out.push_str(&format!("{EXPORT_LINE}{}\n", line_text(f)));
    }
    for n in &log.notes {
        // One "- Note:" line for each non-empty line of the note.
        for line in n.split('\n').map(str::trim).filter(|l| !l.is_empty()) {
            out.push_str(&format!("- Note: {}\n", line_text(line)));
        }
    }
    if !log.labels.is_empty() {
        let labels: Vec<String> = log
            .labels
            .iter()
            .map(|l| format!("\"{}\"", line_text(l)))
            .collect();
        let s = if labels.len() == 1 { "" } else { "s" };
        out.push_str(&format!("- Undo step{s}: {}\n", labels.join("; ")));
    }
    if let Some(c) = &log.copy {
        out.push_str(&format!("- Saved copy: {}\n", line_text(c)));
    }
    Some(out)
}

/// Ruling 6's memory: the path of the last line that reads exactly
/// `- Export folder: <absolute path>`.
pub fn remembered_folder(text: &str) -> Option<PathBuf> {
    text.lines()
        .rev()
        .filter_map(|l| l.strip_prefix(EXPORT_LINE))
        .map(Path::new)
        .find(|p| p.is_absolute())
        .map(Path::to_path_buf)
}

/// True when `stamp` is "YYYY-MM-DD HH-MM" (digits checked).
pub fn check_stamp(stamp: &str) -> bool {
    stamp.len() == 16
        && stamp.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 | 13 => b == b'-',
            10 => b == b' ',
            _ => b.is_ascii_digit(),
        })
}

/// True when `date` is "YYYY-MM-DD" (digits checked).
pub fn check_date(date: &str) -> bool {
    date.len() == 10
        && date.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 => b == b'-',
            _ => b.is_ascii_digit(),
        })
}

/// The song's part of an export folder's name (Ruling 6): `/` and `:` as
/// `-`, controls removed, leading dots dropped, cut to [`SONG_CHARS`]
/// characters (and [`SONG_BYTES`] bytes, so the name fits a file name);
/// "Untitled" when nothing is left.
pub(crate) fn clean_song(song: &str) -> String {
    let mut out = String::new();
    let chars = song
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| if matches!(c, '/' | ':') { '-' } else { c })
        .skip_while(|c| *c == '.');
    for (i, c) in chars.enumerate() {
        if i >= SONG_CHARS || out.len() + c.len_utf8() > SONG_BYTES {
            break;
        }
        out.push(c);
    }
    if out.trim().is_empty() {
        "Untitled".to_string()
    } else {
        out
    }
}

/// An export folder's first name, "<song> — <what> — <date>" (Ruling 6).
pub(crate) fn export_folder_name(song: &str, what: ExportWhat, date: &str) -> String {
    format!("{} — {} — {date}", clean_song(song), what.label())
}

/// Ruling 6: a new folder "<song> — <what> — <date>" inside `parent`,
/// with " 2", " 3"… up to 999 when the name is taken (`create_dir` never
/// replaces). `date` must be "YYYY-MM-DD". `parent` is checked by the
/// caller.
pub fn make_export_folder(
    parent: &Path,
    song: &str,
    what: ExportWhat,
    date: &str,
) -> Result<PathBuf, VizError> {
    if !check_date(date) {
        return Err(model("An export folder's date is \"YYYY-MM-DD\"."));
    }
    let base = export_folder_name(song, what, date);
    for n in 1..=MAX_NUMBER {
        let name = if n == 1 {
            base.clone()
        } else {
            format!("{base} {n}")
        };
        let path = parent.join(name);
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io(format!("The export folder could not be made ({e})."))),
        }
    }
    Err(io(
        "The export folder could not be made: every name is taken.",
    ))
}

/// Copies the regular files and folders under `from` into the new folder
/// `to`; links are left out.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let t = e.file_type()?;
        let dest = to.join(e.file_name());
        if t.is_dir() {
            copy_tree(&e.path(), &dest)?;
        } else if t.is_file() {
            std::fs::copy(e.path(), &dest)?;
        }
    }
    Ok(())
}

/// Moves the copy folder `src` to `<dir>/<name>` (Ruling 7, step 2):
/// by `rename` when `try_rename`; else, or across volumes, by a copy into
/// `.<name>.moving` beside it, a rename into place, then removing `src`.
/// A name already taken leaves `src` where it is. Nothing half-moved is
/// left on failure.
fn move_copy(src: &Path, dir: &Path, name: &str, try_rename: bool) -> Result<(), String> {
    const TAKEN: &str = "the song already has a copy of that name";
    let dst = dir.join(name);
    if std::fs::symlink_metadata(&dst).is_ok() {
        return Err(TAKEN.into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if try_rename {
        match std::fs::rename(src, &dst) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == ErrorKind::CrossesDevices => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    let tmp = dir.join(format!(".{name}.moving"));
    // One an earlier move left is this code's own.
    if std::fs::symlink_metadata(&tmp).is_ok() {
        std::fs::remove_dir_all(&tmp).map_err(|e| e.to_string())?;
    }
    let placed = copy_tree(src, &tmp)
        .map_err(|e| e.to_string())
        .and_then(|()| {
            if std::fs::symlink_metadata(&dst).is_ok() {
                return Err(TAKEN.to_string());
            }
            std::fs::rename(&tmp, &dst).map_err(|e| e.to_string())
        });
    if let Err(e) = placed {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    // The copy is whole in the package: the source may go. One that
    // cannot is found again by the next save, whose name check keeps it.
    let _ = std::fs::remove_dir_all(src);
    Ok(())
}

/// Ruling 7, step 1: each state blob of the stand-in copied into the
/// package's when absent. The blobs it holds the same bytes of; `Err`
/// when one there has other bytes or one cannot be copied.
fn copy_blobs(stand_in: &Path, package: &Path) -> Result<Vec<PathBuf>, String> {
    let from = stand_in.join(STATE_DIR);
    let Ok(rd) = std::fs::read_dir(&from) else {
        return Ok(Vec::new());
    };
    let to = package.join(STATE_DIR);
    let mut same = Vec::new();
    for e in rd {
        let e = e.map_err(|e| e.to_string())?;
        if !e.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let bytes = std::fs::read(e.path()).map_err(|e| e.to_string())?;
        let dst = to.join(e.file_name());
        match std::fs::symlink_metadata(&dst) {
            Ok(_) => {
                if std::fs::read(&dst).ok().as_deref() != Some(bytes.as_slice()) {
                    return Err("a plug-in state there differs from the song's".into());
                }
            }
            Err(_) => {
                std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
                match create_new(&dst, &bytes) {
                    Ok(()) => {}
                    Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                        if std::fs::read(&dst).ok().as_deref() != Some(bytes.as_slice()) {
                            return Err("a plug-in state there differs from the song's".into());
                        }
                    }
                    Err(e) => return Err(e.to_string()),
                }
            }
        }
        same.push(e.path());
    }
    Ok(same)
}

/// Ruling 7, step 3: the stand-in's notes moved, or appended to the
/// package's own after [`BEFORE_FIRST_SAVE`]; the source goes only after.
fn move_notes(stand_in: &Path, package: &Path) -> Result<(), String> {
    let src = stand_in.join(NOTES_FILE);
    if !plain(&src).map_err(|e| e.to_string())? {
        return Ok(());
    }
    let bytes = std::fs::read(&src).map_err(|e| e.to_string())?;
    let dst = package.join(NOTES_FILE);
    let appended = |bytes: &[u8]| {
        let mut block = BEFORE_FIRST_SAVE.as_bytes().to_vec();
        block.extend_from_slice(bytes);
        append_to(&dst, &block).map_err(|e| e.to_string())
    };
    if plain(&dst).map_err(|e| e.to_string())? {
        appended(&bytes)?;
    } else {
        match create_new(&dst, &bytes) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => appended(&bytes)?,
            Err(e) => return Err(e.to_string()),
        }
    }
    std::fs::remove_file(&src).map_err(|e| e.to_string())
}

/// Ruling 7: an untitled song's notes, copies and the state blobs they use
/// moved from its stand-in into `package`, never replacing a file and
/// removing a source only once its copy is whole; empty stand-in folders
/// are removed. Each thing left behind gives one problem line.
pub fn move_in(stand_in: &Path, package: &Path) -> Vec<String> {
    move_in_with(stand_in, package, true)
}

fn move_in_with(stand_in: &Path, package: &Path, try_rename: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if !std::fs::symlink_metadata(stand_in).is_ok_and(|m| m.is_dir()) {
        return lines;
    }
    let left = |what: &str, why: &str| {
        format!(
            "Co-Producer's {what} from before the first save stayed in {}: {}.",
            stand_in.display(),
            why.trim_end_matches('.')
        )
    };
    let copies = stand_in.join(COPIES_DIR);
    match copy_blobs(stand_in, package) {
        Ok(blobs) => {
            match core_model::file::list_copies(stand_in) {
                Ok(list) => {
                    for pj in list {
                        let Some(src) = pj.parent() else { continue };
                        let Some(name) = src.file_name().and_then(|n| n.to_str()) else {
                            continue;
                        };
                        if let Err(why) =
                            move_copy(src, &package.join(COPIES_DIR), name, try_rename)
                        {
                            let what = format!("copy \"{}\"", diff::shown_name(name));
                            lines.push(left(&what, &why));
                        }
                    }
                }
                Err(e) => lines.push(left("copies", &e.to_string())),
            }
            // The blobs stay while a copy that may use them does.
            if core_model::file::list_copies(stand_in).is_ok_and(|l| l.is_empty()) {
                for b in blobs {
                    let _ = std::fs::remove_file(b);
                }
            }
        }
        Err(why) => lines.push(left("copies", &why)),
    }
    if let Err(why) = move_notes(stand_in, package) {
        lines.push(left("notes", &why));
    }
    for d in [
        copies.clone(),
        copies.parent().map(Path::to_path_buf).unwrap_or_default(),
        stand_in.join(STATE_DIR),
        stand_in.to_path_buf(),
    ] {
        let _ = std::fs::remove_dir(d);
    }
    lines
}

/// One copy with about what removing it frees (Ruling 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopySize {
    /// Its folder's name.
    pub name: String,
    /// Its folder.
    pub path: PathBuf,
    /// Its own files, plus the package's media and state files only it
    /// names.
    pub bytes: u64,
}

/// The package-relative files `doc` names: its media and its state blobs.
fn names_of(doc: &core_model::document::Document) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = doc.media().values().map(|m| m.path.clone()).collect();
    out.extend(
        doc.referenced_states()
            .into_iter()
            .map(|b| format!("{STATE_DIR}/{}", core_model::state::blob_file_name(b))),
    );
    out
}

fn doc_at(project: &Path) -> Option<core_model::document::Document> {
    let text = std::fs::read_to_string(project).ok()?;
    core_model::file::from_json(&text).ok()
}

/// Bytes of the regular files under `dir` (links not followed), at most
/// 64 folders deep.
fn tree_bytes(dir: &Path, depth: u32) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    rd.flatten()
        .map(|e| match std::fs::symlink_metadata(e.path()) {
            Ok(m) if m.is_file() => m.len(),
            Ok(m) if m.is_dir() && depth < 64 => tree_bytes(&e.path(), depth + 1),
            _ => 0,
        })
        .sum()
}

/// Ruling 8: the copies in `home`, oldest first (their names start with
/// their stamp), each with its own folder's bytes plus the files of
/// `home` it names that `present`, the autosaves and no other copy name.
/// An estimate: a copy or autosave that cannot be read names nothing.
pub fn copy_sizes(
    home: &Path,
    present: &core_model::document::Document,
) -> Result<Vec<CopySize>, VizError> {
    let list = core_model::file::list_copies(home).map_err(VizError::from)?;
    let mut kept = names_of(present);
    for a in core_model::file::list_autosaves(home).unwrap_or_default() {
        if let Some(d) = doc_at(&a) {
            kept.extend(names_of(&d));
        }
    }
    let copies: Vec<(String, PathBuf, BTreeSet<String>)> = list
        .iter()
        .filter_map(|pj| {
            let dir = pj.parent()?;
            let name = dir.file_name()?.to_str()?.to_string();
            let names = doc_at(pj).map(|d| names_of(&d)).unwrap_or_default();
            Some((name, dir.to_path_buf(), names))
        })
        .collect();
    let mut uses: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, _, names) in &copies {
        for n in names {
            *uses.entry(n.as_str()).or_default() += 1;
        }
    }
    Ok(copies
        .iter()
        .map(|(name, dir, names)| {
            let own = format!("{COPIES_DIR}/{name}/");
            let only: u64 = names
                .iter()
                .filter(|n| {
                    !n.starts_with(&own)
                        && !kept.contains(n.as_str())
                        && uses.get(n.as_str()).copied() == Some(1)
                })
                .filter_map(|n| std::fs::symlink_metadata(home.join(n)).ok())
                .filter(std::fs::Metadata::is_file)
                .map(|m| m.len())
                .sum();
            CopySize {
                name: name.clone(),
                path: dir.clone(),
                bytes: tree_bytes(dir, 0) + only,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The copy path used across volumes moves a copy whole.
    #[test]
    fn a_copy_moves_by_copying() {
        let dir = tempfile::tempdir().unwrap();
        let stand_in = dir.path().join("s");
        let pkg = dir.path().join("P.vizdaw");
        let src = stand_in.join(COPIES_DIR).join("C.vizdaw");
        std::fs::create_dir_all(src.join("Media")).unwrap();
        std::fs::write(src.join("project.json"), "{}").unwrap();
        std::fs::write(src.join("Media/a.caf"), "aa").unwrap();
        let lines = move_in_with(&stand_in, &pkg, false);
        assert!(lines.is_empty(), "{lines:?}");
        let moved = pkg.join(COPIES_DIR).join("C.vizdaw");
        assert_eq!(std::fs::read(moved.join("Media/a.caf")).unwrap(), b"aa");
        assert!(!pkg.join(COPIES_DIR).join(".C.vizdaw.moving").exists());
        assert!(!stand_in.exists());
    }

    #[test]
    fn songs_in_folder_names() {
        assert_eq!(clean_song("a/b:.c"), "a-b-.c");
        assert_eq!(clean_song("..x\u{7}y"), "xy");
        assert_eq!(clean_song("..."), "Untitled");
        // Plan 8c Task 3: the leading dots go after the `/` becomes `-`.
        assert_eq!(clean_song("../Mix"), "-Mix");
        assert_eq!(clean_song("a/b"), "a-b");
        assert_eq!(clean_song(".hidden"), "hidden");
        assert_eq!(clean_song(&"é".repeat(150)).chars().count(), 90);
        assert_eq!(clean_song(&"a".repeat(150)).chars().count(), 100);
    }
}
