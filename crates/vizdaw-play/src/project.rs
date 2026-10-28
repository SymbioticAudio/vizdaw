//! Loading a project and choosing the sequence to play.

use core_engine::engine::find_sequence;
use core_model::document::Document;
use core_model::ids::SeqId;
use core_model::query::{seq_length, unused_sequences};
use std::path::Path;

/// Why no sequence could be chosen.
#[derive(Debug, PartialEq, Eq)]
pub enum Choice {
    /// Play this one.
    Play(SeqId),
    /// `--seq` named no sequence.
    Unknown(String),
    /// Several (or no) top-level sequences; these are the candidates.
    Ambiguous(Vec<String>),
}

/// The package's media for the engine's streamer (5a Ruling 24): each
/// item's file under `package`. The engine opens them lazily; one that is
/// not a readable CAF fails once and plays as silence.
pub fn media_files(doc: &Document, package: &Path) -> Vec<core_media::stream::MediaFile> {
    doc.media()
        .values()
        .map(|m| core_media::stream::MediaFile {
            id: m.id,
            path: package.join(&m.path),
        })
        .collect()
}

/// Loads `project.json` from a package.
pub fn load(path: &Path) -> Result<Document, String> {
    core_model::file::load(path).map_err(|e| format!("cannot load {}: {e}", path.display()))
}

/// `--seq NAME`, else the only top-level sequence.
pub fn choose(doc: &Document, name: Option<&str>) -> Choice {
    if let Some(n) = name {
        return match find_sequence(doc, n) {
            Ok(id) => Choice::Play(id),
            Err(_) => Choice::Unknown(n.to_string()),
        };
    }
    let top = unused_sequences(doc);
    if let [only] = top.as_slice() {
        return Choice::Play(*only);
    }
    let candidates: Vec<SeqId> = if top.is_empty() {
        doc.sequences().keys().copied().collect()
    } else {
        top
    };
    Choice::Ambiguous(
        candidates
            .iter()
            .filter_map(|s| doc.sequence(*s).ok())
            .map(|s| s.name.clone())
            .collect(),
    )
}

/// One line per sequence: name, length in bars, top-level marker.
pub fn describe_sequences(doc: &Document) -> Vec<String> {
    let top = unused_sequences(doc);
    doc.sequences()
        .values()
        .map(|s| {
            let bar = s
                .meter
                .entry_at(core_model::time::Tick::ZERO)
                .meter
                .bar_ticks()
                .max(1);
            let len = seq_length(doc, s.id).map_or(0, |t| t.0 / bar);
            let mark = if top.contains(&s.id) {
                "  (top level)"
            } else {
                ""
            };
            format!("{}  {} bar(s){mark}", s.name, len)
        })
        .collect()
}
