//! The `.vizdaw` package: `project.json`, `Media/` and `Autosave/`.
//!
//! `project.json` is `{ "format": "vizdaw-project", "version": 2,
//! "document": { … } }`. Loading migrates older versions step by step,
//! refuses newer ones, rejects unknown fields and runs every integrity
//! check. Saving writes a temporary file, syncs it, renames it over the old
//! file and syncs the directory.

use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::history::Timestamp;
use crate::ids::PluginInstanceId;
use crate::mixer::{PluginRef, PluginSlot};
use crate::rules::validate;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The `format` tag.
pub const FORMAT: &str = "vizdaw-project";
/// The version this build writes.
pub const VERSION: u64 = 2;
/// The oldest version this build can migrate from.
pub const FIRST_VERSION: u64 = 1;
/// The project file name inside a package.
pub const PROJECT_FILE: &str = "project.json";

/// A migration from version N to N+1, on raw JSON.
pub type Migration = fn(Value) -> Result<Value>;

/// Migrations indexed by `from - FIRST_VERSION`.
pub const MIGRATIONS: &[Migration] = &[migrate_v1_to_v2];

/// Version 1 → 2: each plugin instrument's `plugin_ref` string becomes a
/// `slot` holding `PluginRef::from_legacy(string)` with instance IDs 0, 1, …
/// in destination order; `counters` gains `strip` (0) and `plugin`. The
/// mixer is left out; loading derives it (Plan Ruling 6).
pub fn migrate_v1_to_v2(mut v: Value) -> Result<Value> {
    let doc = v
        .get_mut("document")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| corrupt("missing document"))?;
    let mut next = 0u64;
    if let Some(Value::Array(insts)) = doc.get_mut("instruments") {
        for d in insts.iter_mut() {
            let Some(p) = d.pointer_mut("/kind/plugin_instrument") else {
                continue;
            };
            let name = p
                .get("plugin_ref")
                .and_then(Value::as_str)
                .ok_or_else(|| corrupt("plugin_instrument without plugin_ref"))?
                .to_string();
            let slot = PluginSlot::new(PluginInstanceId(next), PluginRef::from_legacy(&name));
            *p = json!({ "slot": serde_json::to_value(slot).map_err(corrupt)? });
            next += 1;
        }
    }
    let counters = doc
        .get_mut("counters")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| corrupt("missing counters"))?;
    counters.insert("strip".into(), Value::from(0u64));
    counters.insert("plugin".into(), Value::from(next));
    Ok(v)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectFile {
    format: String,
    version: u64,
    document: Document,
}

#[derive(Serialize)]
struct ProjectFileRef<'a> {
    format: &'static str,
    version: u64,
    document: &'a Document,
}

fn corrupt(p: impl std::fmt::Display) -> ModelError {
    ModelError::Corrupt {
        problems: vec![p.to_string()],
    }
}

/// Serializes a document as pretty `project.json` text.
pub fn to_json(doc: &Document) -> Result<String> {
    serde_json::to_string_pretty(&ProjectFileRef {
        format: FORMAT,
        version: VERSION,
        document: doc,
    })
    .map_err(corrupt)
}

/// Runs `steps` to bring `value` from version `found` to `target`.
/// `steps[i]` migrates version `first + i` to `first + i + 1`.
pub fn run_migrations(
    mut value: Value,
    found: u64,
    target: u64,
    first: u64,
    steps: &[Migration],
) -> Result<Value> {
    if found < first {
        return Err(corrupt(format!("unsupported format version {found}")));
    }
    for v in found..target {
        let idx = usize::try_from(v - first).map_err(corrupt)?;
        let step = steps
            .get(idx)
            .ok_or_else(|| corrupt(format!("no migration from version {v}")))?;
        value = step(value)?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("version".into(), Value::from(v + 1));
        }
    }
    Ok(value)
}

/// Just the header, read without building the document.
#[derive(Deserialize)]
struct Header {
    format: Option<String>,
    version: Option<u64>,
}

/// Parses, migrates and validates `project.json` text.
pub fn from_json(text: &str) -> Result<Document> {
    // Fast path: a current-version file deserializes in one pass.
    let file = match serde_json::from_str::<ProjectFile>(text) {
        Ok(f) if f.format == FORMAT && f.version == VERSION => f,
        _ => slow_path(text)?,
    };
    let problems = validate(&file.document);
    if !problems.is_empty() {
        return Err(ModelError::Corrupt { problems });
    }
    Ok(file.document)
}

/// Checks the header, refuses newer versions and migrates older ones.
fn slow_path(text: &str) -> Result<ProjectFile> {
    let header: Header = serde_json::from_str(text).map_err(corrupt)?;
    if header.format.as_deref() != Some(FORMAT) {
        return Err(corrupt("not a vizdaw-project file"));
    }
    let found = header.version.ok_or_else(|| corrupt("missing version"))?;
    if found > VERSION {
        return Err(ModelError::NewerFormat { found });
    }
    let value: Value = serde_json::from_str(text).map_err(corrupt)?;
    let value = run_migrations(value, found, VERSION, FIRST_VERSION, MIGRATIONS)?;
    serde_json::from_value(value).map_err(corrupt)
}

/// Writes `bytes` to `path` atomically: temp file, fsync, rename, fsync dir.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| ModelError::invalid("path has no parent directory"))?;
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}

/// Saves `doc` into the package directory (created if missing).
pub fn save(doc: &Document, package: &Path) -> Result<()> {
    fs::create_dir_all(package.join("Media"))?;
    fs::create_dir_all(package.join("Autosave"))?;
    write_atomic(&package.join(PROJECT_FILE), to_json(doc)?.as_bytes())
}

/// Loads the package's `project.json`.
pub fn load(package: &Path) -> Result<Document> {
    from_json(&fs::read_to_string(package.join(PROJECT_FILE))?)
}

/// Autosave copies in the package, newest first.
pub fn list_autosaves(package: &Path) -> Result<Vec<PathBuf>> {
    let dir = package.join("Autosave");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut v: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("project-") && n.ends_with(".json"))
        })
        .collect();
    v.sort();
    v.reverse();
    Ok(v)
}

/// Writes a timestamped copy to `Autosave/` and keeps the newest
/// `settings.autosave_keep` copies. Returns the new file's path.
pub fn autosave(doc: &Document, package: &Path, at: Timestamp) -> Result<PathBuf> {
    let dir = package.join("Autosave");
    fs::create_dir_all(&dir)?;
    let mut ms = at.0;
    let mut path = dir.join(format!("project-{ms:020}.json"));
    while path.exists() {
        ms = ms.saturating_add(1);
        path = dir.join(format!("project-{ms:020}.json"));
    }
    write_atomic(&path, to_json(doc)?.as_bytes())?;
    let keep = usize::try_from(doc.settings().autosave_keep.max(1)).unwrap_or(usize::MAX);
    for old in list_autosaves(package)?.into_iter().skip(keep) {
        fs::remove_file(old)?;
    }
    Ok(path)
}
