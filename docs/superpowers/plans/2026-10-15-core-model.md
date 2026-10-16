# VizDAW Core Model (Sub-project 2a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `crates/core-model`, a pure-Rust crate that holds a VizDAW project as immutable values and expresses every model edit from the parent spec §5 as a validated, undoable command, with queries (flatten, time conversion, usage), a versioned project file, property tests, golden files and benchmarks.

**Architecture:** A `Document` is an immutable value built from `im` persistent collections; its fields are private and change only through the pure `command::apply_in(doc, ctx, cmd) -> Result<Applied>`, which clones (cheaply), mutates the clone through index-aware helpers and returns the new document plus a `Report`. A derived event index (`EventId → location`) lives in memory only. `History` wraps `apply_in` with versions, transactions, the single selection and "Modify Again"; `query::*` are read-only functions; `file` serializes a stable JSON representation (arrays ordered by ID, events by `(tick, id)`).

**Tech Stack:** Rust stable (1.99.1 installed), edition 2024, Cargo workspace (resolver 3); runtime `serde 1` (+`derive`, `rc`), `serde_json 1`, `im 15` (+`serde`), `thiserror 2`; dev `proptest 1`, `criterion 0.5`, `insta 1` (+`json`).

**Spec:** `docs/superpowers/specs/2026-10-15-core-model-design.md` (binding); parent `docs/superpowers/specs/2026-10-15-vizdaw-design.md` §4–5, §8, §12 for context. Executors read both.

## Global Constraints

- Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` (Rust lives in `~/.cargo/bin`; Xcode's license is not accepted, so `/usr/bin/git`, `cc` and the linker only work with the Command Line Tools developer dir). Every bash block below starts with that line.
- Work in `/Users/lattawb/workspace/vizdaw` on branch `overnight`. Commit at the end of every task; never push. If your harness requires an attribution trailer on commits, append it to the messages given.
- Workspace root `Cargo.toml`: `[workspace] members = ["crates/*"]`, `resolver = "3"`, `edition = "2024"`; the crate is `crates/core-model` (package `core-model`, import as `core_model`).
- Runtime dependencies are limited to `serde`, `serde_json`, `im`, `thiserror`; dev-only `proptest`, `criterion`, `insta`. No other crates (tests make temp dirs with `std::env::temp_dir()`, not `tempfile`).
- No platform-specific code; must compile on macOS and Linux.
- "No panics on any input": `clippy::unwrap_used`, `expect_used`, `indexing_slicing` and `panic` are denied in library code via `#![cfg_attr(not(test), deny(...))]` in `lib.rs`; integration tests, benches and `#[cfg(test)]` code may use `unwrap`. All integer arithmetic on user-supplied values saturates or uses checked conversion (`Tick` arithmetic saturates).
- `#![deny(missing_docs)]`: every public item has a doc comment.
- Time: `Tick(i64)` at **15,360 ticks per quarter note**; tempo is fixed-point milli-BPM within **5–999 BPM**; meter changes fall on barlines.
- IDs are per-document counters, never reused; serialized as plain integers. `project.json` is `{ "format": "vizdaw-project", "version": 1, "document": { … } }`; events within a take serialize as an array ordered by `(tick, id)`; unknown fields are rejected everywhere (`#[serde(deny_unknown_fields)]`).
- One error enum, `ModelError` (spec §8 variants plus `Invalid { reason }`).
- Each task ends green: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Code in this plan is already `rustfmt`-formatted for edition 2024; run `cargo fmt --all` anyway before checking.

## Review Focus

The five failure modes most likely to bite a user, each pinned by a named test in its owning task:

1. **Reference cycles introduced by re-pointing** (not only by inserting) — a re-point that would make a sequence reach itself must be refused with the full cycle path, and a harmless re-point must still work. Pinned: Task 12 `re_point_refuses_cycles`; generated sequences in Task 17 `no_command_sequence_creates_a_cycle`.
2. **Loop count combined with a per-use length shorter than the sequence** — each pass plays only the first `length` ticks, passes repeat at `length`, and notes crossing the pass end are cut. Pinned: Task 11 `loop_count_with_length_shorter_than_sequence`.
3. **Tick ↔ seconds across linear tempo ramps** — the closed-form ramp integral and its inverse must round-trip every tick exactly. Pinned: Task 2 `linear_ramp_matches_closed_form` and `tick_seconds_round_trip` (proptest).
4. **JSON round-trip of `im` collections and `Arc<[u8]>` SysEx data** — every event kind, 8-bit SysEx bytes, generator params and the ID-keyed maps survive save/load unchanged. Pinned: Task 4 `json_round_trip_preserves_every_event_kind`; Task 17 `save_then_load_is_identical`.
5. **Undo after a failed command inside a transaction** — the failed command must leave no trace, commit must still collapse the transaction into one step, and undo/redo must land exactly before/after it. Pinned: Task 15 `undo_after_failed_command_in_transaction`.

## Spec Rulings

Ambiguities in the spec resolved by this plan (each is implemented and tested as stated):

1. **Pickups:** tick 0 is bar 1 beat 1 (beats are 1-based); `start_offset ≥ 0` is the pickup length and content plays from `-start_offset`; bar 0 is the pickup bar.
2. **Tempo/meter maps:** the first tempo point and first meter entry sit at tick 0 and also govern negative ticks; a ramp is linear in BPM per tick; key signatures share meter entries, so key changes also fall on barlines.
3. **References:** `Count(n)` plays the per-use length `n` times; `Fill` repeats the sequence's own length until the per-use length is filled (last pass cut); the pickup plays only before the first pass; a reference that starts before its window still plays the part inside it.
4. **Derived length:** longest lane (its own length, else the end of its active take's content — notes, reference spans, audio frames converted with the owning sequence's tempo) rounded up to a barline, minimum one bar. Inactive takes do not count.
5. **Lane length and loops:** a lane's own length stops its playback (later events silent, longer notes and references cut); looped passes start at the loop point, so negative ticks never play in a looped lane.
6. **Takes:** cycle checks and use counts consider every take (a take switch can never create a cycle).
7. **Flatten command** inlines recursively (all nesting levels, matching the flatten query), makes destinations explicit, warns about events that had no destination, and locks the owner's length if inlining would shorten it.
8. **Transpose** clamps at 0/127 rather than dropping notes.
9. **Selection-dependent commands** use `apply_in(doc, Context { selection, filter }, cmd)`; `apply(doc, cmd)` is `apply_in` with an empty selection; `Applied` gains `selection: Option<Selection>`.
10. **Document immutability:** `Document` fields are private (read accessors only); a derived `EventId → location` index is kept in memory and rebuilt on load, never serialized.
11. **Errors:** `ModelError` adds `Invalid { reason }` for structural refusals; `NotOnBarline` carries the tick; `OutOfRange.value` is a `String`; `Io` holds the message.
12. **Drum rows** match notes by exact `(pitch, event dest override)`; Move Row moves active-take notes and pins the source lane's destination when the lanes differ; Remove Row deletes the row's notes.
13. **Make Reference from Selection** moves active-take events inside the (optionally bar-rounded) range into new sequence(s) with a locked length equal to the range; the loop count is only set on the new reference.
14. **Keyboard slots** are unique within the project; clashes with command key bindings are checked by the app (2c).
15. **Unused sequences** are those with zero users (top-level songs included; the caller decides which to keep).
16. **History:** selection-only select-and-modify is not an undo step; inside a transaction only the transaction's own steps can be undone; the depth limit is read from the document's settings.
17. **Versioning:** version 1 is the first format, so `MIGRATIONS` is empty; the generic `run_migrations` is tested with fake steps. `project.json` is pretty-printed (diffable); loading uses a one-pass fast path for the current version.
18. **View filters** hide event kinds and lanes.

## File Structure

```
Cargo.toml                         workspace (members = crates/*, resolver 3, edition 2024)
.github/workflows/ci.yml           fmt + clippy + test on macos-14 and ubuntu-latest
crates/core-model/
  Cargo.toml                       deps + [[bench]] model
  src/lib.rs                       crate docs, lint gates, module list, re-exports
  src/error.rs                     ModelError, Result
  src/ids.rs                       typed IDs, AnyId, Counters
  src/time.rs                      Tick, PPQ, BarBeatUnit
  src/tempo.rs                     Bpm, TempoPoint, TempoMap (tick <-> seconds)
  src/meter.rs                     Meter, KeySig, MeterEntry, MeterMap (tick <-> bar.beat.unit)
  src/event.rs                     Event, EventSpec, EventBody and value types
  src/lane.rs                      Lane, Take, QuantizeSpec, DrumLayout, DrumRow
  src/sequence.rs                  Sequence, SeqKind, KeySlot, Marker, SeqLength
  src/document.rs                  Document (+ event index), DocumentRepr, Settings, Destination, OutputPath, MediaItem
  src/rules.rs                     cycle checks, referential integrity, ranges, validate()
  src/selection.rs                 Selection, ViewFilter
  src/rule.rs                      Rule, SelectMode, events_matching
  src/modify.rs                    Modify and friends, ModifyTemplate, apply_modify
  src/history.rs                   History, Version, Timestamp, transactions
  src/file.rs                      project.json, atomic save, autosave, migrations
  src/command/mod.rs               Command, apply/apply_in, Report, Applied, Context
  src/command/{sequence,lane,event,drum,reference,audio,routing,settings,select}.rs
  src/query/mod.rs                 re-exports
  src/query/{length,routing,usage,flatten}.rs
  tests/*.rs                       one integration-test file per task
  tests/fixtures/*.json            golden project files
  tests/snapshots/*.snap           insta snapshots of the golden summaries
  benches/model.rs                 criterion benchmarks
```

Each task below lists the exact files it creates or modifies. Tests are integration tests (`tests/<name>.rs`) so they exercise only the public API; crate-internal helpers are tested through the commands that use them.

---

### Task 1: Workspace scaffold, typed IDs and the error type

**Goal:** Create the Cargo workspace and the `core-model` crate with lint gates, typed ID newtypes, per-document counters and `ModelError`; prove the pinned dependency versions resolve.

**Files:**
- Create: `Cargo.toml`
- Create: `crates/core-model/Cargo.toml`
- Create: `crates/core-model/src/error.rs`
- Create: `crates/core-model/src/ids.rs`
- Create: `crates/core-model/src/lib.rs`
- Create (test): `crates/core-model/tests/ids.rs`

**Interfaces:**
- Consumes: Nothing.
- Produces:
  - `core_model::ids::{SeqId, LaneId, EventId, DestId, OutputPathId, MediaId, TakeId, DrumRowId}` — `pub struct X(pub u64)`, `Copy + Ord + Hash + Serialize` (as a plain integer), `Display` (`seq3`, `ev9`, …).
  - `core_model::ids::AnyId` enum (one variant per ID type) with `From<each ID>`.
  - `core_model::ids::Counters { seq, lane, event, dest, output, media, take, drum_row: u64 }` with `alloc_seq(&mut self) -> SeqId` … `alloc_drum_row(&mut self) -> DrumRowId` (saturating, never reused).
  - `core_model::error::ModelError` variants `Cycle { path: Vec<SeqId> }`, `UnknownId(AnyId)`, `InUse { users: Vec<AnyId> }`, `OutOfRange { field: &'static str, value: String }`, `NotOnBarline { tick: i64 }`, `NewerFormat { found: u64 }`, `Corrupt { problems: Vec<String> }`, `Invalid { reason: String }`, `Io(String)`; helpers `ModelError::invalid(reason)`, `ModelError::range(field, value)`; `From<std::io::Error>`.
  - `core_model::{ModelError, Result}` re-exports (`Result<T> = std::result::Result<T, ModelError>`).

- [ ] **Step 1: Create the workspace manifests**

`Cargo.toml`:

```toml
[workspace]
members = ["crates/*"]
resolver = "3"

[workspace.package]
edition = "2024"
license = "GPL-3.0-only"
rust-version = "1.85"
```

`crates/core-model/Cargo.toml`:

```toml
[package]
name = "core-model"
version = "0.1.0"
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
serde = { version = "1", features = ["derive", "rc"] }
serde_json = "1"
im = { version = "15", features = ["serde"] }
thiserror = "2"

[dev-dependencies]
proptest = "1"
criterion = "0.5"
insta = { version = "1", features = ["json"] }
```

Also create a placeholder `crates/core-model/src/lib.rs` containing the single line `//! core-model` so the crate builds, then validate dependency resolution:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo build
```

Expected: `Finished` (Cargo.lock is created; resolved versions include serde 1.0.x, serde_json 1.0.x, im 15.1.x, thiserror 2.0.x, proptest 1.x, criterion 0.5.x, insta 1.x). If resolution fails, pin the newest version cargo offers within the same major (e.g. `im = "=15.1.0"`) and note it in the commit message.

- [ ] **Step 2: Write the failing test**

`crates/core-model/tests/ids.rs`:

```rust
use core_model::ModelError;
use core_model::ids::{AnyId, Counters, EventId, SeqId};

#[test]
fn counters_allocate_increasing_ids_per_type() {
    let mut c = Counters::default();
    assert_eq!(c.alloc_seq(), SeqId(0));
    assert_eq!(c.alloc_seq(), SeqId(1));
    assert_eq!(c.alloc_event(), EventId(0));
    assert_eq!(c.seq, 2);
    assert_eq!(c.event, 1);
}

#[test]
fn counters_saturate_instead_of_overflowing() {
    let mut c = Counters {
        event: u64::MAX,
        ..Counters::default()
    };
    assert_eq!(c.alloc_event(), EventId(u64::MAX));
    assert_eq!(c.alloc_event(), EventId(u64::MAX));
}

#[test]
fn ids_serialize_as_plain_integers() {
    assert_eq!(serde_json::to_string(&SeqId(7)).unwrap(), "7");
    let back: EventId = serde_json::from_str("42").unwrap();
    assert_eq!(back, EventId(42));
}

#[test]
fn any_id_displays_with_prefix() {
    assert_eq!(AnyId::from(SeqId(3)).to_string(), "seq3");
    let e = ModelError::UnknownId(EventId(9).into());
    assert_eq!(e.to_string(), "unknown id ev9");
}

#[test]
fn io_errors_convert() {
    let e: ModelError = std::io::Error::other("boom").into();
    assert!(matches!(e, ModelError::Io(ref m) if m.contains("boom")));
}
```


- [ ] **Step 3: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test ids
```

Expected: FAIL to compile — `error[E0432]: unresolved import `core_model::ids` (the crate has no modules yet)`.

- [ ] **Step 4: Implement**

Create `crates/core-model/src/error.rs`:

```rust
//! The single error type of the crate.

use crate::ids::{AnyId, SeqId};

/// Every failure the model can report.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    /// A reference would make a sequence reach itself. `path` starts and ends
    /// with the same sequence.
    #[error("reference cycle through sequences {path:?}")]
    Cycle {
        /// The cycle, first element repeated at the end.
        path: Vec<SeqId>,
    },
    /// An ID that does not exist in the document.
    #[error("unknown id {0}")]
    UnknownId(AnyId),
    /// The item is still used by the listed items.
    #[error("still in use by {users:?}")]
    InUse {
        /// The users (events, lanes or drum rows) that block the change.
        users: Vec<AnyId>,
    },
    /// A value outside its allowed range.
    #[error("{field} out of range: {value}")]
    OutOfRange {
        /// The field name.
        field: &'static str,
        /// The offending value, formatted.
        value: String,
    },
    /// A meter change that does not fall on a barline.
    #[error("meter change at tick {tick} is not on a barline")]
    NotOnBarline {
        /// The tick of the offending meter change.
        tick: i64,
    },
    /// The project file was written by a newer VizDAW.
    #[error("project format version {found} is newer than this build supports")]
    NewerFormat {
        /// The version found in the file.
        found: u64,
    },
    /// The project file failed validation; every problem is listed.
    #[error("corrupt project: {problems:?}")]
    Corrupt {
        /// Human-readable problem descriptions.
        problems: Vec<String>,
    },
    /// A structurally invalid request that is not an ID or range problem
    /// (for example committing with no open transaction).
    #[error("invalid: {reason}")]
    Invalid {
        /// Why the request was refused.
        reason: String,
    },
    /// A filesystem error, formatted.
    #[error("i/o error: {0}")]
    Io(String),
}

impl From<std::io::Error> for ModelError {
    fn from(e: std::io::Error) -> Self {
        ModelError::Io(e.to_string())
    }
}

impl ModelError {
    /// Shorthand for [`ModelError::Invalid`].
    pub fn invalid(reason: impl Into<String>) -> Self {
        ModelError::Invalid {
            reason: reason.into(),
        }
    }

    /// Shorthand for [`ModelError::OutOfRange`].
    pub fn range(field: &'static str, value: impl std::fmt::Display) -> Self {
        ModelError::OutOfRange {
            field,
            value: value.to_string(),
        }
    }
}

/// Crate-wide result alias.
pub type Result<T> = std::result::Result<T, ModelError>;
```

Create `crates/core-model/src/ids.rs`:

```rust
//! Typed ID newtypes and the per-document counters that allocate them.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident, $prefix:literal) => {
        $(#[$m])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}{}", $prefix, self.0)
            }
        }
    };
}

id_type!(
    /// Identifies a sequence.
    SeqId,
    "seq"
);
id_type!(
    /// Identifies a lane (track) within a sequence.
    LaneId,
    "lane"
);
id_type!(
    /// Identifies an event; unique across the whole document.
    EventId,
    "ev"
);
id_type!(
    /// Identifies an instrument-list destination.
    DestId,
    "dest"
);
id_type!(
    /// Identifies an audio output path.
    OutputPathId,
    "out"
);
id_type!(
    /// Identifies a media-pool item.
    MediaId,
    "media"
);
id_type!(
    /// Identifies a take within a lane.
    TakeId,
    "take"
);
id_type!(
    /// Identifies a drum row within a drum lane.
    DrumRowId,
    "row"
);

/// Any ID, used in errors and reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AnyId {
    /// A sequence.
    Seq(SeqId),
    /// A lane.
    Lane(LaneId),
    /// An event.
    Event(EventId),
    /// A destination.
    Dest(DestId),
    /// An output path.
    Output(OutputPathId),
    /// A media item.
    Media(MediaId),
    /// A take.
    Take(TakeId),
    /// A drum row.
    DrumRow(DrumRowId),
}

impl fmt::Display for AnyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnyId::Seq(i) => i.fmt(f),
            AnyId::Lane(i) => i.fmt(f),
            AnyId::Event(i) => i.fmt(f),
            AnyId::Dest(i) => i.fmt(f),
            AnyId::Output(i) => i.fmt(f),
            AnyId::Media(i) => i.fmt(f),
            AnyId::Take(i) => i.fmt(f),
            AnyId::DrumRow(i) => i.fmt(f),
        }
    }
}

macro_rules! any_from {
    ($t:ident, $v:ident) => {
        impl From<$t> for AnyId {
            fn from(i: $t) -> Self {
                AnyId::$v(i)
            }
        }
    };
}
any_from!(SeqId, Seq);
any_from!(LaneId, Lane);
any_from!(EventId, Event);
any_from!(DestId, Dest);
any_from!(OutputPathId, Output);
any_from!(MediaId, Media);
any_from!(TakeId, Take);
any_from!(DrumRowId, DrumRow);

/// Per-document ID counters. Each holds the next value to hand out; IDs are
/// never reused within a document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counters {
    /// Next sequence ID.
    pub seq: u64,
    /// Next lane ID.
    pub lane: u64,
    /// Next event ID.
    pub event: u64,
    /// Next destination ID.
    pub dest: u64,
    /// Next output-path ID.
    pub output: u64,
    /// Next media ID.
    pub media: u64,
    /// Next take ID.
    pub take: u64,
    /// Next drum-row ID.
    pub drum_row: u64,
}

fn bump(c: &mut u64) -> u64 {
    let v = *c;
    *c = c.saturating_add(1);
    v
}

impl Counters {
    /// Allocates a sequence ID.
    pub fn alloc_seq(&mut self) -> SeqId {
        SeqId(bump(&mut self.seq))
    }
    /// Allocates a lane ID.
    pub fn alloc_lane(&mut self) -> LaneId {
        LaneId(bump(&mut self.lane))
    }
    /// Allocates an event ID.
    pub fn alloc_event(&mut self) -> EventId {
        EventId(bump(&mut self.event))
    }
    /// Allocates a destination ID.
    pub fn alloc_dest(&mut self) -> DestId {
        DestId(bump(&mut self.dest))
    }
    /// Allocates an output-path ID.
    pub fn alloc_output(&mut self) -> OutputPathId {
        OutputPathId(bump(&mut self.output))
    }
    /// Allocates a media ID.
    pub fn alloc_media(&mut self) -> MediaId {
        MediaId(bump(&mut self.media))
    }
    /// Allocates a take ID.
    pub fn alloc_take(&mut self) -> TakeId {
        TakeId(bump(&mut self.take))
    }
    /// Allocates a drum-row ID.
    pub fn alloc_drum_row(&mut self) -> DrumRowId {
        DrumRowId(bump(&mut self.drum_row))
    }
}
```

Replace the placeholder `crates/core-model/src/lib.rs` with:

```rust
//! `core-model`: the VizDAW project model.
//!
//! A pure-Rust crate holding a VizDAW project and every edit that can be made
//! to it. It has no audio, MIDI I/O, UI or OS dependencies. All edits are
//! [`command::Command`] values applied by the pure function
//! [`command::apply`]; documents are immutable values that share structure.
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod error;
pub mod ids;

pub use error::{ModelError, Result};
```


- [ ] **Step 5: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test ids
```

Expected: PASS — `ids`: `test result: ok. 5 passed`.

- [ ] **Step 6: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add Cargo.lock Cargo.toml crates/core-model/Cargo.toml crates/core-model/src/error.rs crates/core-model/src/ids.rs crates/core-model/src/lib.rs crates/core-model/tests/ids.rs
git commit -m "feat(core-model): workspace scaffold, typed ids, ModelError"
```

---

### Task 2: Ticks and tempo maps (tick ↔ seconds across linear ramps)

**Goal:** Add `Tick` (15,360 PPQ, saturating arithmetic), `BarBeatUnit`, and `TempoMap` with constant segments and linear ramps plus exact tick↔seconds conversion.

**Files:**
- Modify: `crates/core-model/src/lib.rs`
- Create: `crates/core-model/src/tempo.rs`
- Create: `crates/core-model/src/time.rs`
- Create (test): `crates/core-model/tests/tempo.rs`

**Interfaces:**
- Consumes: `ModelError::{range, invalid}`, `Result` (Task 1).
- Produces:
  - `core_model::time::{PPQ: i64 = 15_360, Tick(pub i64), BarBeatUnit { bar: i32, beat: u16, unit: u32 }}`; `Tick::{ZERO, MAX, MIN, quarters(n), times(self, n)}`; `Add/Sub/Neg` saturate.
  - `core_model::tempo::Bpm(pub u32)` (milli-BPM) with `Bpm::{MIN, MAX, whole(u32), as_f64(), in_range()}`.
  - `core_model::tempo::TempoPoint { tick: Tick, bpm: Bpm, ramp: bool }`.
  - `core_model::tempo::TempoMap` (private `points`; `Default` = 120 BPM): `constant(Bpm) -> Result<TempoMap>`, `new(Vec<TempoPoint>) -> Result<TempoMap>`, `points() -> &Vector<TempoPoint>`, `check() -> Result<()>`, `tick_to_seconds(Tick) -> f64`, `seconds_to_tick(f64) -> Tick` (nearest tick; NaN → `Tick::MIN`, ±∞ → MAX/MIN).

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/tempo.rs`:

```rust
use core_model::ModelError;
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::{PPQ, Tick};
use proptest::prelude::*;

fn pt(tick: i64, bpm: u32, ramp: bool) -> TempoPoint {
    TempoPoint {
        tick: Tick(tick),
        bpm: Bpm::whole(bpm),
        ramp,
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn constant_tempo_converts_quarters_to_seconds() {
    let m = TempoMap::constant(Bpm::whole(120)).unwrap();
    assert!(close(m.tick_to_seconds(Tick(PPQ)), 0.5));
    assert!(close(m.tick_to_seconds(Tick(-PPQ)), -0.5));
    assert_eq!(m.seconds_to_tick(0.5), Tick(PPQ));
    assert_eq!(m.seconds_to_tick(-0.5), Tick(-PPQ));
}

#[test]
fn step_change_accumulates_segments() {
    let m = TempoMap::new(vec![pt(0, 120, false), pt(PPQ * 4, 60, false)]).unwrap();
    // 4 beats at 120 = 2 s, then 1 beat at 60 = 1 s.
    assert!(close(m.tick_to_seconds(Tick(PPQ * 5)), 3.0));
    assert_eq!(m.seconds_to_tick(3.0), Tick(PPQ * 5));
}

#[test]
fn linear_ramp_matches_closed_form() {
    // Ramp 60 -> 120 BPM over 4 beats: t = 60/(PPQ*k) * ln(b1/b0), k = 60/(4*PPQ).
    let m = TempoMap::new(vec![pt(0, 60, true), pt(PPQ * 4, 120, false)]).unwrap();
    let expected = 4.0 * std::f64::consts::LN_2;
    assert!(close(m.tick_to_seconds(Tick(PPQ * 4)), expected));
    assert_eq!(m.seconds_to_tick(expected), Tick(PPQ * 4));
    // After the ramp, constant 120.
    assert!(close(m.tick_to_seconds(Tick(PPQ * 5)), expected + 0.5));
}

#[test]
fn ramp_flag_on_last_point_is_ignored() {
    let m = TempoMap::new(vec![pt(0, 120, true)]).unwrap();
    assert!(close(m.tick_to_seconds(Tick(PPQ)), 0.5));
}

#[test]
fn validation_rejects_bad_maps() {
    assert!(matches!(
        TempoMap::new(vec![pt(0, 4, false)]),
        Err(ModelError::OutOfRange {
            field: "tempo.bpm",
            ..
        })
    ));
    assert!(matches!(
        TempoMap::new(vec![pt(0, 1000, false)]),
        Err(ModelError::OutOfRange { .. })
    ));
    assert!(matches!(
        TempoMap::new(vec![pt(10, 120, false)]),
        Err(ModelError::OutOfRange {
            field: "tempo.first_tick",
            ..
        })
    ));
    assert!(matches!(
        TempoMap::new(vec![pt(0, 120, false), pt(0, 90, false)]),
        Err(ModelError::OutOfRange {
            field: "tempo.tick_order",
            ..
        })
    ));
    assert!(TempoMap::new(vec![]).is_err());
}

#[test]
fn non_finite_seconds_do_not_panic() {
    let m = TempoMap::default();
    assert_eq!(m.seconds_to_tick(f64::INFINITY), Tick::MAX);
    assert_eq!(m.seconds_to_tick(f64::NEG_INFINITY), Tick::MIN);
    assert_eq!(m.seconds_to_tick(f64::NAN), Tick::MIN);
}

fn arb_map() -> impl Strategy<Value = TempoMap> {
    prop::collection::vec((1i64..PPQ * 16, 5u32..=999, any::<bool>()), 0..6).prop_map(|rest| {
        let mut pts = vec![pt(0, 120, true)];
        let mut t = 0;
        for (gap, bpm, ramp) in rest {
            t += gap;
            pts.push(pt(t, bpm, ramp));
        }
        TempoMap::new(pts).unwrap()
    })
}

proptest! {
    // Review focus #3: tick <-> seconds across linear tempo ramps.
    #[test]
    fn tick_seconds_round_trip(m in arb_map(), t in -PPQ * 8..PPQ * 2_000) {
        let s = m.tick_to_seconds(Tick(t));
        prop_assert_eq!(m.seconds_to_tick(s), Tick(t));
    }

    #[test]
    fn seconds_are_monotonic(m in arb_map(), t in 0i64..PPQ * 200) {
        prop_assert!(m.tick_to_seconds(Tick(t + 1)) > m.tick_to_seconds(Tick(t)));
    }
}
```


- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test tempo
```

Expected: FAIL to compile — `error[E0432]: unresolved import `core_model::tempo``.

- [ ] **Step 3: Implement**

Replace the whole of `crates/core-model/src/lib.rs` with:

```rust
//! `core-model`: the VizDAW project model.
//!
//! A pure-Rust crate holding a VizDAW project and every edit that can be made
//! to it. It has no audio, MIDI I/O, UI or OS dependencies. All edits are
//! [`command::Command`] values applied by the pure function
//! [`command::apply`]; documents are immutable values that share structure.
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod error;
pub mod ids;
pub mod tempo;
pub mod time;

pub use error::{ModelError, Result};
```

Create `crates/core-model/src/tempo.rs`:

```rust
//! Tempo maps with constant segments and linear ramps.

use crate::error::{ModelError, Result};
use crate::time::{PPQ, Tick};
use im::Vector;
use serde::{Deserialize, Serialize};

/// Tempo as fixed-point milli-BPM (120 BPM is `Bpm(120_000)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Bpm(pub u32);

impl Bpm {
    /// Lowest allowed tempo, 5 BPM.
    pub const MIN: Bpm = Bpm(5_000);
    /// Highest allowed tempo, 999 BPM.
    pub const MAX: Bpm = Bpm(999_000);

    /// Whole beats per minute.
    pub fn whole(bpm: u32) -> Bpm {
        Bpm(bpm.saturating_mul(1000))
    }

    /// Tempo as floating-point BPM.
    pub fn as_f64(self) -> f64 {
        f64::from(self.0) / 1000.0
    }

    /// True when within 5–999 BPM.
    pub fn in_range(self) -> bool {
        (Bpm::MIN..=Bpm::MAX).contains(&self)
    }
}

/// One tempo point. When `ramp` is true the tempo changes linearly (in BPM
/// per tick) until the next point; the last point's ramp flag is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TempoPoint {
    /// Where the point starts.
    pub tick: Tick,
    /// Tempo at that tick.
    pub bpm: Bpm,
    /// Ramp linearly to the next point.
    pub ramp: bool,
}

/// Ordered tempo points; the first is at tick 0 and governs negative ticks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TempoMap {
    points: Vector<TempoPoint>,
}

impl Default for TempoMap {
    fn default() -> Self {
        TempoMap {
            points: Vector::unit(TempoPoint {
                tick: Tick::ZERO,
                bpm: Bpm::whole(120),
                ramp: false,
            }),
        }
    }
}

/// Seconds per tick at `bpm`.
fn spt(bpm: f64) -> f64 {
    60.0 / (bpm * PPQ as f64)
}

impl TempoMap {
    /// A single constant tempo.
    pub fn constant(bpm: Bpm) -> Result<TempoMap> {
        TempoMap::new(vec![TempoPoint {
            tick: Tick::ZERO,
            bpm,
            ramp: false,
        }])
    }

    /// Builds a validated map.
    pub fn new(points: Vec<TempoPoint>) -> Result<TempoMap> {
        let map = TempoMap {
            points: points.into_iter().collect(),
        };
        map.check()?;
        Ok(map)
    }

    /// The points, in order.
    pub fn points(&self) -> &Vector<TempoPoint> {
        &self.points
    }

    /// Returns the first rule violation, if any. Used by commands and by
    /// project-file validation (a deserialized map bypasses [`TempoMap::new`]).
    pub fn check(&self) -> Result<()> {
        let first = self
            .points
            .front()
            .ok_or_else(|| ModelError::invalid("tempo map is empty"))?;
        if first.tick != Tick::ZERO {
            return Err(ModelError::range("tempo.first_tick", first.tick.0));
        }
        let mut prev: Option<Tick> = None;
        for p in &self.points {
            if !p.bpm.in_range() {
                return Err(ModelError::range("tempo.bpm", p.bpm.as_f64()));
            }
            if prev.is_some_and(|t| p.tick <= t) {
                return Err(ModelError::range("tempo.tick_order", p.tick.0));
            }
            prev = Some(p.tick);
        }
        Ok(())
    }

    /// Segments as `(start, end, bpm_start, bpm_end)`; the last has
    /// `end == None`.
    fn segments(&self) -> impl Iterator<Item = (i64, Option<i64>, f64, f64)> + '_ {
        let n = self.points.len();
        (0..n).filter_map(move |i| {
            let p = self.points.get(i)?;
            let next = self.points.get(i + 1);
            let b0 = p.bpm.as_f64().max(1.0);
            let b1 = match next {
                Some(q) if p.ramp => q.bpm.as_f64().max(1.0),
                _ => b0,
            };
            Some((p.tick.0, next.map(|q| q.tick.0), b0, b1))
        })
    }

    fn first_bpm(&self) -> f64 {
        self.points
            .front()
            .map_or(120.0, |p| p.bpm.as_f64().max(1.0))
    }

    /// Seconds from tick 0 to `t` (negative for negative ticks).
    pub fn tick_to_seconds(&self, t: Tick) -> f64 {
        if t.0 < 0 || self.points.is_empty() {
            return t.0 as f64 * spt(self.first_bpm());
        }
        let mut secs = 0.0;
        for (start, end, b0, b1) in self.segments() {
            let seg_end = end.unwrap_or(i64::MAX);
            if t.0 < start {
                break;
            }
            let len = (seg_end - start) as f64;
            let dx = (t.0.min(seg_end) - start) as f64;
            secs += seg_seconds(dx, len, b0, b1);
            if t.0 < seg_end {
                break;
            }
        }
        secs
    }

    /// The tick at `s` seconds, rounded to the nearest tick.
    pub fn seconds_to_tick(&self, s: f64) -> Tick {
        if s.is_nan() {
            return Tick::MIN;
        }
        if s.is_infinite() {
            return if s > 0.0 { Tick::MAX } else { Tick::MIN };
        }
        if s < 0.0 || self.points.is_empty() {
            return Tick((s / spt(self.first_bpm())).round() as i64);
        }
        let mut acc = 0.0;
        for (start, end, b0, b1) in self.segments() {
            match end {
                Some(e) => {
                    let len = (e - start) as f64;
                    let seg = seg_seconds(len, len, b0, b1);
                    if s < acc + seg {
                        let dx = seg_ticks(s - acc, len, b0, b1);
                        return Tick((start as f64 + dx).round() as i64);
                    }
                    acc += seg;
                }
                None => {
                    let dx = (s - acc) / spt(b0);
                    return Tick((start as f64 + dx).round() as i64);
                }
            }
        }
        Tick::ZERO
    }
}

/// Seconds spent covering `dx` ticks of a segment `len` ticks long ramping
/// linearly from `b0` to `b1` BPM.
fn seg_seconds(dx: f64, len: f64, b0: f64, b1: f64) -> f64 {
    if (b1 - b0).abs() < 1e-9 || len <= 0.0 {
        return dx * spt(b0);
    }
    let k = (b1 - b0) / len;
    60.0 / (PPQ as f64 * k) * (k * dx / b0).ln_1p()
}

/// Inverse of [`seg_seconds`]: ticks covered after `s` seconds.
fn seg_ticks(s: f64, len: f64, b0: f64, b1: f64) -> f64 {
    if (b1 - b0).abs() < 1e-9 || len <= 0.0 {
        return s / spt(b0);
    }
    let k = (b1 - b0) / len;
    b0 * (s * PPQ as f64 * k / 60.0).exp_m1() / k
}
```

Create `crates/core-model/src/time.rs`:

```rust
//! Musical time: ticks and bar·beat·unit positions.

use serde::{Deserialize, Serialize};
use std::ops::{Add, Neg, Sub};

/// Ticks per quarter note.
pub const PPQ: i64 = 15_360;

/// Musical time in ticks at [`PPQ`] ticks per quarter note.
///
/// Arithmetic saturates instead of overflowing, so no input can panic.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Tick(pub i64);

impl Tick {
    /// Tick zero: the downbeat of bar 1.
    pub const ZERO: Tick = Tick(0);
    /// The largest tick.
    pub const MAX: Tick = Tick(i64::MAX);
    /// The smallest tick.
    pub const MIN: Tick = Tick(i64::MIN);

    /// `n` quarter notes.
    pub fn quarters(n: i64) -> Tick {
        Tick(n.saturating_mul(PPQ))
    }

    /// Saturating multiplication by an integer.
    pub fn times(self, n: i64) -> Tick {
        Tick(self.0.saturating_mul(n))
    }
}

impl Add for Tick {
    type Output = Tick;
    fn add(self, o: Tick) -> Tick {
        Tick(self.0.saturating_add(o.0))
    }
}

impl Sub for Tick {
    type Output = Tick;
    fn sub(self, o: Tick) -> Tick {
        Tick(self.0.saturating_sub(o.0))
    }
}

impl Neg for Tick {
    type Output = Tick;
    fn neg(self) -> Tick {
        Tick(self.0.saturating_neg())
    }
}

/// A display and entry position. Bar 1 beat 1 unit 0 is tick 0; bar 0 is the
/// pickup bar before it. `beat` is 1-based. `unit` counts display units within
/// the beat at a resolution chosen by the caller (`units_per_beat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarBeatUnit {
    /// Bar number (0 is the pickup bar, may be negative before it).
    pub bar: i32,
    /// Beat within the bar, starting at 1.
    pub beat: u16,
    /// Units within the beat.
    pub unit: u32,
}
```


- [ ] **Step 4: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test tempo
```

Expected: PASS — `tempo`: `test result: ok. 8 passed`.

- [ ] **Step 5: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/lib.rs crates/core-model/src/tempo.rs crates/core-model/src/time.rs crates/core-model/tests/tempo.rs
git commit -m "feat(core-model): ticks and tempo maps with linear ramps"
```

---

### Task 3: Meter maps and bar·beat·unit conversion

**Goal:** Add `MeterMap` (time + key signatures on barlines) and tick ↔ bar·beat·unit conversion with bar 0 as the pickup bar.

**Files:**
- Modify: `crates/core-model/src/lib.rs`
- Create: `crates/core-model/src/meter.rs`
- Create (test): `crates/core-model/tests/meter.rs`

**Interfaces:**
- Consumes: `Tick`, `PPQ`, `BarBeatUnit` (Task 2); `ModelError::NotOnBarline` (Task 1).
- Produces:
  - `core_model::meter::{Meter { num: u8, den: u8 }, KeySig { sharps: i8, minor: bool }, MeterEntry { tick: Tick, meter: Meter, key: KeySig }}`; `Meter::{beat_ticks(), bar_ticks(), is_valid()}`.
  - `core_model::meter::MeterMap` (private `entries`; `Default` = 4/4 C major at tick 0): `new(Vec<MeterEntry>) -> Result<MeterMap>`, `entries()`, `check() -> Result<()>`, `entry_at(Tick) -> MeterEntry`, `tick_to_bbu(Tick, units_per_beat: u32) -> BarBeatUnit`, `bbu_to_tick(BarBeatUnit, units_per_beat: u32) -> Tick` (first tick of the unit), `barline_at_or_after(Tick) -> Tick`, `barline_at_or_before(Tick) -> Tick`.

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/meter.rs`:

```rust
use core_model::ModelError;
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::{BarBeatUnit, PPQ, Tick};
use proptest::prelude::*;

fn entry(tick: i64, num: u8, den: u8) -> MeterEntry {
    MeterEntry {
        tick: Tick(tick),
        meter: Meter { num, den },
        key: KeySig::default(),
    }
}

fn bbu(bar: i32, beat: u16, unit: u32) -> BarBeatUnit {
    BarBeatUnit { bar, beat, unit }
}

#[test]
fn tick_zero_is_bar_one() {
    let m = MeterMap::default();
    assert_eq!(m.tick_to_bbu(Tick(0), 4), bbu(1, 1, 0));
    assert_eq!(m.tick_to_bbu(Tick(PPQ * 5 + PPQ / 2), 4), bbu(2, 2, 2));
}

#[test]
fn pickup_ticks_are_bar_zero() {
    let m = MeterMap::default();
    // One beat before bar 1 is bar 0 beat 4.
    assert_eq!(m.tick_to_bbu(Tick(-PPQ), 1), bbu(0, 4, 0));
    assert_eq!(m.bbu_to_tick(bbu(0, 4, 0), 1), Tick(-PPQ));
    assert_eq!(m.tick_to_bbu(Tick(-PPQ * 5), 1), bbu(-1, 4, 0));
}

#[test]
fn meter_change_renumbers_following_bars() {
    // Two bars of 4/4, then 6/8.
    let m = MeterMap::new(vec![entry(0, 4, 4), entry(PPQ * 8, 6, 8)]).unwrap();
    assert_eq!(m.tick_to_bbu(Tick(PPQ * 8), 1), bbu(3, 1, 0));
    // 6/8 bar = 3 quarters; eighth-note beats.
    assert_eq!(m.tick_to_bbu(Tick(PPQ * 11 + PPQ / 2), 1), bbu(4, 2, 0));
    assert_eq!(m.bbu_to_tick(bbu(4, 2, 0), 1), Tick(PPQ * 11 + PPQ / 2));
}

#[test]
fn off_barline_change_is_rejected() {
    let err = MeterMap::new(vec![entry(0, 4, 4), entry(PPQ * 3, 3, 4)]).unwrap_err();
    assert_eq!(err, ModelError::NotOnBarline { tick: PPQ * 3 });
}

#[test]
fn invalid_signatures_are_rejected() {
    assert!(MeterMap::new(vec![entry(0, 4, 3)]).is_err());
    assert!(MeterMap::new(vec![entry(0, 0, 4)]).is_err());
    assert!(MeterMap::new(vec![entry(0, 4, 128)]).is_err());
    assert!(MeterMap::new(vec![entry(PPQ, 4, 4)]).is_err());
    let mut bad_key = entry(0, 4, 4);
    bad_key.key.sharps = 8;
    assert!(MeterMap::new(vec![bad_key]).is_err());
}

#[test]
fn barline_helpers() {
    let m = MeterMap::new(vec![entry(0, 3, 4)]).unwrap();
    assert_eq!(m.barline_at_or_after(Tick(1)), Tick(PPQ * 3));
    assert_eq!(m.barline_at_or_after(Tick(PPQ * 3)), Tick(PPQ * 3));
    assert_eq!(m.barline_at_or_before(Tick(PPQ * 4)), Tick(PPQ * 3));
    assert_eq!(m.barline_at_or_before(Tick(-1)), Tick(-PPQ * 3));
}

#[test]
fn zero_units_per_beat_does_not_panic() {
    let m = MeterMap::default();
    let p = m.tick_to_bbu(Tick(123), 0);
    assert_eq!(p.bar, 1);
    let _ = m.bbu_to_tick(bbu(i32::MAX, u16::MAX, u32::MAX), 0);
    let _ = m.bbu_to_tick(bbu(i32::MIN, 0, 0), 0);
}

fn arb_meter_map() -> impl Strategy<Value = MeterMap> {
    let sig = (1u8..=13, prop::sample::select(vec![1u8, 2, 4, 8, 16, 32]));
    prop::collection::vec((1i64..6, sig), 0..5).prop_map(|changes| {
        let mut entries = vec![entry(0, 4, 4)];
        let mut tick = 0;
        let mut bar_t = Meter { num: 4, den: 4 }.bar_ticks();
        for (bars, (num, den)) in changes {
            tick += bars * bar_t;
            entries.push(entry(tick, num, den));
            bar_t = Meter { num, den }.bar_ticks();
        }
        MeterMap::new(entries).unwrap()
    })
}

proptest! {
    #[test]
    fn tick_bbu_tick_round_trips_at_full_resolution(
        m in arb_meter_map(), t in -PPQ * 16..PPQ * 400
    ) {
        // PPQ*4 units per beat is a multiple of every beat length.
        let upb = (PPQ * 4) as u32;
        prop_assert_eq!(m.bbu_to_tick(m.tick_to_bbu(Tick(t), upb), upb), Tick(t));
    }

    #[test]
    fn bbu_tick_bbu_round_trips_for_coarser_units(
        m in arb_meter_map(), t in -PPQ * 16..PPQ * 400, upb in 1u32..=960
    ) {
        let p = m.tick_to_bbu(Tick(t), upb);
        prop_assert_eq!(m.tick_to_bbu(m.bbu_to_tick(p, upb), upb), p);
    }

    // Spec §8: tick <-> bar·beat·unit <-> seconds across ramps and meter changes.
    #[test]
    fn bbu_seconds_chain_round_trips(
        m in arb_meter_map(), t in 0..PPQ * 400, b in 5u32..=999
    ) {
        let tempo = TempoMap::new(vec![
            TempoPoint { tick: Tick(0), bpm: Bpm::whole(b), ramp: true },
            TempoPoint { tick: Tick(PPQ * 64), bpm: Bpm::whole(120), ramp: false },
        ]).unwrap();
        let upb = (PPQ * 4) as u32;
        let p = m.tick_to_bbu(Tick(t), upb);
        let secs = tempo.tick_to_seconds(m.bbu_to_tick(p, upb));
        let back = m.tick_to_bbu(tempo.seconds_to_tick(secs), upb);
        prop_assert_eq!(back, p);
    }
}
```


- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test meter
```

Expected: FAIL to compile — `error[E0432]: unresolved import `core_model::meter``.

- [ ] **Step 3: Implement**

Replace the whole of `crates/core-model/src/lib.rs` with:

```rust
//! `core-model`: the VizDAW project model.
//!
//! A pure-Rust crate holding a VizDAW project and every edit that can be made
//! to it. It has no audio, MIDI I/O, UI or OS dependencies. All edits are
//! [`command::Command`] values applied by the pure function
//! [`command::apply`]; documents are immutable values that share structure.
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod error;
pub mod ids;
pub mod meter;
pub mod tempo;
pub mod time;

pub use error::{ModelError, Result};
```

Create `crates/core-model/src/meter.rs`:

```rust
//! Meter maps (time signatures and key signatures) and bar·beat·unit
//! conversion.

use crate::error::{ModelError, Result};
use crate::time::{BarBeatUnit, PPQ, Tick};
use im::Vector;
use serde::{Deserialize, Serialize};

/// A time signature. `den` must be a power of two from 1 to 64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meter {
    /// Beats per bar (1–255).
    pub num: u8,
    /// Beat unit as a note value (4 = quarter note).
    pub den: u8,
}

impl Meter {
    /// Ticks per beat.
    pub fn beat_ticks(self) -> i64 {
        (PPQ * 4) / i64::from(self.den.max(1))
    }

    /// Ticks per bar.
    pub fn bar_ticks(self) -> i64 {
        self.beat_ticks() * i64::from(self.num.max(1))
    }

    /// True when `num >= 1` and `den` is a power of two in 1..=64.
    pub fn is_valid(self) -> bool {
        self.num >= 1 && self.den.is_power_of_two() && self.den <= 64
    }
}

/// A key signature: `sharps` in -7..=7 (negative = flats).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeySig {
    /// Number of sharps (negative for flats).
    pub sharps: i8,
    /// Minor mode.
    pub minor: bool,
}

/// A meter (and key) change. Changes fall on barlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeterEntry {
    /// Where the change takes effect (a barline).
    pub tick: Tick,
    /// The new time signature.
    pub meter: Meter,
    /// The key signature from here on.
    pub key: KeySig,
}

/// Ordered meter entries. The first is at tick 0 (bar 1) and also governs
/// the pickup bars before it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeterMap {
    entries: Vector<MeterEntry>,
}

const FOUR_FOUR: MeterEntry = MeterEntry {
    tick: Tick::ZERO,
    meter: Meter { num: 4, den: 4 },
    key: KeySig {
        sharps: 0,
        minor: false,
    },
};

impl Default for MeterMap {
    fn default() -> Self {
        MeterMap {
            entries: Vector::unit(FOUR_FOUR),
        }
    }
}

impl MeterMap {
    /// Builds a validated map.
    pub fn new(entries: Vec<MeterEntry>) -> Result<MeterMap> {
        let m = MeterMap {
            entries: entries.into_iter().collect(),
        };
        m.check()?;
        Ok(m)
    }

    /// The entries, in order.
    pub fn entries(&self) -> &Vector<MeterEntry> {
        &self.entries
    }

    /// Returns the first rule violation: empty map, first entry not at tick
    /// 0, invalid meter, key outside -7..=7, unordered ticks, or a change off
    /// the previous meter's barlines ([`ModelError::NotOnBarline`]).
    pub fn check(&self) -> Result<()> {
        let first = self
            .entries
            .front()
            .ok_or_else(|| ModelError::invalid("meter map is empty"))?;
        if first.tick != Tick::ZERO {
            return Err(ModelError::range("meter.first_tick", first.tick.0));
        }
        let mut prev: Option<&MeterEntry> = None;
        for e in &self.entries {
            if !e.meter.is_valid() {
                return Err(ModelError::range(
                    "meter.signature",
                    format!("{}/{}", e.meter.num, e.meter.den),
                ));
            }
            if !(-7..=7).contains(&e.key.sharps) {
                return Err(ModelError::range("meter.key", e.key.sharps));
            }
            if let Some(p) = prev {
                if e.tick <= p.tick {
                    return Err(ModelError::range("meter.tick_order", e.tick.0));
                }
                if (e.tick.0 - p.tick.0) % p.meter.bar_ticks() != 0 {
                    return Err(ModelError::NotOnBarline { tick: e.tick.0 });
                }
            }
            prev = Some(e);
        }
        Ok(())
    }

    /// Entries paired with the bar number they start at.
    fn with_bars(&self) -> Vec<(MeterEntry, i64)> {
        let mut out = Vec::with_capacity(self.entries.len());
        let mut bar = 1i64;
        let mut prev: Option<MeterEntry> = None;
        for e in &self.entries {
            if let Some(p) = prev {
                bar += (e.tick.0 - p.tick.0) / p.meter.bar_ticks();
            }
            out.push((*e, bar));
            prev = Some(*e);
        }
        if out.is_empty() {
            out.push((FOUR_FOUR, 1));
        }
        out
    }

    /// The entry in effect at `t` (the first entry for negative ticks).
    pub fn entry_at(&self, t: Tick) -> MeterEntry {
        self.entries
            .iter()
            .take_while(|e| e.tick <= t)
            .last()
            .or_else(|| self.entries.front())
            .copied()
            .unwrap_or(FOUR_FOUR)
    }

    /// Converts a tick to bar·beat·unit with `units_per_beat` display units
    /// per beat (0 is treated as 1).
    pub fn tick_to_bbu(&self, t: Tick, units_per_beat: u32) -> BarBeatUnit {
        let upb = i128::from(units_per_beat.max(1));
        let bars = self.with_bars();
        let (e, bar0) = bars
            .iter()
            .take_while(|(e, _)| e.tick <= t)
            .last()
            .or_else(|| bars.first())
            .copied()
            .unwrap_or((FOUR_FOUR, 1));
        let bar_t = e.meter.bar_ticks();
        let beat_t = e.meter.beat_ticks();
        let rel = t.0.saturating_sub(e.tick.0);
        let bar = bar0.saturating_add(rel.div_euclid(bar_t));
        let in_bar = rel.rem_euclid(bar_t);
        let beat = in_bar / beat_t;
        let within = in_bar % beat_t;
        let unit = i128::from(within) * upb / i128::from(beat_t);
        BarBeatUnit {
            bar: i32::try_from(bar).unwrap_or(if bar < 0 { i32::MIN } else { i32::MAX }),
            beat: u16::try_from(beat + 1).unwrap_or(u16::MAX),
            unit: u32::try_from(unit).unwrap_or(u32::MAX),
        }
    }

    /// Converts bar·beat·unit back to the first tick of that unit. Beat 0 is treated as
    /// beat 1; beats past the bar run into the following bars.
    pub fn bbu_to_tick(&self, p: BarBeatUnit, units_per_beat: u32) -> Tick {
        let upb = i128::from(units_per_beat.max(1));
        let bars = self.with_bars();
        let target = i64::from(p.bar);
        let (e, bar0) = bars
            .iter()
            .take_while(|(_, b)| *b <= target)
            .last()
            .or_else(|| bars.first())
            .copied()
            .unwrap_or((FOUR_FOUR, 1));
        let beat_t = e.meter.beat_ticks();
        let bars_in = (target - bar0).saturating_mul(e.meter.bar_ticks());
        let beats_in = i64::from(p.beat.saturating_sub(1)).saturating_mul(beat_t);
        // Ceiling division: the first tick whose display unit is `p.unit`.
        let units_in = (i128::from(p.unit) * i128::from(beat_t) + upb - 1) / upb;
        let units_in = i64::try_from(units_in).unwrap_or(i64::MAX);
        Tick(
            e.tick
                .0
                .saturating_add(bars_in)
                .saturating_add(beats_in)
                .saturating_add(units_in),
        )
    }

    /// The first barline at or after `t`.
    pub fn barline_at_or_after(&self, t: Tick) -> Tick {
        let b = self.barline_at_or_before(t);
        if b == t {
            b
        } else {
            b + Tick(self.entry_at(t).meter.bar_ticks())
        }
    }

    /// The last barline at or before `t`.
    pub fn barline_at_or_before(&self, t: Tick) -> Tick {
        let e = self.entry_at(t);
        let bar_t = e.meter.bar_ticks();
        let rel = t.0.saturating_sub(e.tick.0);
        Tick(e.tick.0.saturating_add(rel.div_euclid(bar_t) * bar_t))
    }
}
```


- [ ] **Step 4: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test meter
```

Expected: PASS — `meter`: `test result: ok. 10 passed`.

- [ ] **Step 5: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/lib.rs crates/core-model/src/meter.rs crates/core-model/tests/meter.rs
git commit -m "feat(core-model): meter maps and bar-beat-unit conversion"
```

---

### Task 4: Model types, the document and its event index

**Goal:** Add every immutable model type from spec §3 (events, lanes, takes, drum layouts, sequences, destinations, outputs, media, settings) and the `Document` with private collections, a derived event index and serde through an ID-ordered array representation.

**Files:**
- Create: `crates/core-model/src/document.rs`
- Create: `crates/core-model/src/event.rs`
- Create: `crates/core-model/src/lane.rs`
- Modify: `crates/core-model/src/lib.rs`
- Create: `crates/core-model/src/sequence.rs`
- Create (test): `crates/core-model/tests/model_types.rs`

**Interfaces:**
- Consumes: IDs/`Counters` (Task 1), `Tick` (Task 2), `TempoMap` (Task 2), `MeterMap` (Task 3).
- Produces:
  - `core_model::event::{Event { id, tick, dest: Option<DestId>, group_channel: Option<(u8, u8)>, mute, body: EventBody }, EventSpec { tick, dest, group_channel, mute, body }, EventBody, EventKind, NoteAttribute, PerNoteCtl, PerNoteKind, TextKind, LoopCount { Count(u32), Fill }, Fade { length: Tick, shape: FadeShape }, FadeShape}`; `EventSpec::{new(tick, body), with_id(id) -> Event}`, `Event::{spec(), key() -> (Tick, EventId)}`, `EventBody::{kind(), note(pitch, velocity, duration), reference(seq), is_midi()}`.
  - `core_model::lane::{Lane, Take { id, name, events: OrdMap<(Tick, EventId), Event> }, QuantizeSpec { grid: Tick, strength: u8 }, DrumLayout { rows: Vector<DrumRow> }, DrumRow { id, name, note, dest, show_velocity }}`; `Lane::{new(id, take, name), active(), take(id), take_index(id)}`, `Take::new(id, name)`, `QuantizeSpec::apply(Tick) -> Tick`, `DrumRow::matches(pitch, dest) -> bool`.
  - `core_model::sequence::{Sequence, SeqKind { Normal, Generated { generator } }, GeneratorParams { generator: String, params: OrdMap<String, String> }, KeySlot { key: String, modifiers: u8 }, Marker { tick, name }, SeqLength { Derived, Locked(Tick) }}`; `Sequence::{new(id, name, lanes), lane(id), lane_index(id)}`.
  - `core_model::document::{Document, DocumentRepr, Settings { history_depth: u32, autosave_keep: u32 }, Destination { id, name, kind, default_patch }, DestKind { MidiEndpoint { endpoint_ref, group, channel }, PluginInstrument { plugin_ref } }, Patch { program, bank }, OutputPath { id, name, reference }, MediaItem { id, path, sample_rate, channels, length_frames, content_hash }, EventLoc { seq, lane, take, tick }}`.
  - `Document` public API: `new()`, `from_parts(DocumentRepr) -> Result<Document>`, `settings()`, `instruments()`, `outputs()`, `media()`, `sequences()`, `counters()`, `sequence(SeqId) -> Result<&Sequence>`, `lane(SeqId, LaneId) -> Result<&Lane>`, `locate(EventId) -> Result<EventLoc>`, `event(EventId) -> Result<(&Event, EventLoc)>`, `all_events() -> impl Iterator<Item = (EventLoc, &Event)>`.
  - `Document` crate-internal API (used by every command task): `settings_mut`, `instruments_mut`, `outputs_mut`, `media_mut`, `counters_mut`, `seq_mut(SeqId) -> Result<&mut Sequence>`, `lane_mut(SeqId, LaneId) -> Result<&mut Lane>`, `insert_event(seq, lane, take: Option<TakeId>, Event) -> Result<EventLoc>`, `remove_event(EventId) -> Result<(Event, EventLoc)>`, `replace_event(Event) -> Result<EventLoc>`, `put_sequence(Sequence)`, `remove_sequence(SeqId) -> Result<Sequence>`, `remove_lane(SeqId, LaneId) -> Result<Lane>`, `rebuild_index() -> Result<(), String>`. **Never** mutate `Take::events` except through these, or the index goes stale.

**Note:** The `impl Document` block holding the crate-internal mutators carries `#[allow(dead_code)]` until the command tasks use them all; Task 18 removes it.

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/model_types.rs`:

```rust
use core_model::document::{
    DestKind, Destination, Document, DocumentRepr, MediaItem, OutputPath, Settings,
};
use core_model::event::{
    EventBody, EventKind, EventSpec, Fade, FadeShape, LoopCount, NoteAttribute, PerNoteCtl,
    PerNoteKind, TextKind,
};
use core_model::ids::{Counters, DestId, EventId, LaneId, MediaId, OutputPathId, SeqId, TakeId};
use core_model::lane::{DrumRow, Lane, QuantizeSpec};
use core_model::sequence::{GeneratorParams, SeqKind, Sequence};
use core_model::time::{PPQ, Tick};
use im::Vector;
use std::sync::Arc;

fn bodies() -> Vec<EventBody> {
    vec![
        EventBody::Note {
            pitch: 60,
            velocity: 40_000,
            release_velocity: 100,
            duration: Tick(PPQ),
            attribute: Some(NoteAttribute { kind: 3, data: 512 }),
            per_note: Vector::unit(PerNoteCtl {
                kind: PerNoteKind::PitchBend,
                offset: Tick(10),
                value: 7,
            }),
        },
        EventBody::Cc {
            index: 7,
            value: u32::MAX,
        },
        EventBody::PitchBend { value: 0x8000_0000 },
        EventBody::ChannelPressure { value: 1 },
        EventBody::PolyPressure {
            pitch: 61,
            value: 2,
        },
        EventBody::Program {
            program: 5,
            bank: Some((1, 2)),
        },
        EventBody::SysEx {
            data: Arc::from(vec![0x7e_u8, 0xff, 0x00]),
            eight_bit: true,
        },
        EventBody::Text {
            kind: TextKind::Lyric,
            text: "la".into(),
        },
        EventBody::Reference {
            seq: SeqId(1),
            length: Some(Tick(PPQ)),
            loop_count: LoopCount::Fill,
            transpose: -3,
        },
        EventBody::AudioRegion {
            media: MediaId(0),
            source_offset: 10,
            length: 4800,
            gain_db: -6.0,
            fade_in: Fade::default(),
            fade_out: Fade {
                length: Tick(100),
                shape: FadeShape::EqualPower,
            },
        },
    ]
}

/// Builds a document by hand: seq 0 holds one lane with every event kind
/// (event ids assigned in reverse so array order is by tick, not id).
fn sample() -> Document {
    let mut lane = Lane::new(LaneId(0), TakeId(0), "Lane");
    lane.midi_dest = Some(DestId(0));
    lane.audio_out = Some(OutputPathId(0));
    let n = bodies().len() as u64;
    let take = lane.takes.get_mut(0).unwrap();
    for (i, b) in bodies().into_iter().enumerate() {
        let ev = EventSpec::new(Tick(i as i64 * 10), b).with_id(EventId(n - 1 - i as u64));
        take.events.insert(ev.key(), ev);
    }
    let mut top = Sequence::new(SeqId(0), "Song", Vector::unit(lane));
    top.kind = SeqKind::Generated {
        generator: GeneratorParams {
            generator: "arp".into(),
            params: [("rate".to_string(), "1/16".to_string())]
                .into_iter()
                .collect(),
        },
    };
    let sub = Sequence::new(
        SeqId(1),
        "Verse",
        Vector::unit(Lane::new(LaneId(1), TakeId(1), "L")),
    );
    Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![Destination {
            id: DestId(0),
            name: "Synth".into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: "ep".into(),
                group: 1,
                channel: 9,
            },
            default_patch: None,
        }],
        outputs: vec![OutputPath {
            id: OutputPathId(0),
            name: "Main".into(),
            reference: "bus:0".into(),
        }],
        media: vec![MediaItem {
            id: MediaId(0),
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 48_000,
            content_hash: None,
        }],
        sequences: vec![top, sub],
        counters: Counters {
            seq: 2,
            lane: 2,
            event: n,
            dest: 1,
            output: 1,
            media: 1,
            take: 2,
            drum_row: 0,
        },
    })
    .unwrap()
}

// Review focus #4: JSON round-trip of im collections and Arc<[u8]>.
#[test]
fn json_round_trip_preserves_every_event_kind() {
    let doc = sample();
    let json = serde_json::to_string(&doc).unwrap();
    let back: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(back, doc);
    let (ev, _) = back.event(EventId(3)).unwrap();
    assert_eq!(
        ev.body,
        EventBody::SysEx {
            data: Arc::from(vec![0x7e_u8, 0xff, 0x00]),
            eight_bit: true
        }
    );
}

#[test]
fn events_serialize_as_array_ordered_by_tick() {
    let v = serde_json::to_value(sample()).unwrap();
    let events = &v["sequences"][0]["lanes"][0]["takes"][0]["events"];
    let ids: Vec<u64> = events
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![9, 8, 7, 6, 5, 4, 3, 2, 1, 0]);
    assert_eq!(events[0]["body"]["type"], "note");
    assert!(v["sequences"][0]["id"].is_u64());
}

#[test]
fn index_locates_events() {
    let doc = sample();
    let (ev, loc) = doc.event(EventId(9)).unwrap();
    assert_eq!(ev.body.kind(), EventKind::Note);
    assert_eq!(
        (loc.seq, loc.lane, loc.take),
        (SeqId(0), LaneId(0), TakeId(0))
    );
    assert!(doc.event(EventId(99)).is_err());
    assert_eq!(doc.all_events().count(), 10);
}

#[test]
fn unknown_fields_are_rejected() {
    let mut v = serde_json::to_value(sample()).unwrap();
    v["sequences"][0]["lanes"][0]["bogus"] = serde_json::json!(1);
    assert!(serde_json::from_value::<Document>(v).is_err());
    let mut v = serde_json::to_value(sample()).unwrap();
    v["sequences"][0]["lanes"][0]["takes"][0]["events"][0]["bogus"] = serde_json::json!(1);
    assert!(serde_json::from_value::<Document>(v).is_err());
}

#[test]
fn duplicate_ids_are_rejected() {
    let mut v = serde_json::to_value(sample()).unwrap();
    let seqs = v["sequences"].as_array_mut().unwrap();
    let dup = seqs[0].clone();
    seqs.push(dup);
    assert!(serde_json::from_value::<Document>(v).is_err());

    let mut v = serde_json::to_value(sample()).unwrap();
    v["sequences"][1]["lanes"][0]["takes"][0]["events"] =
        v["sequences"][0]["lanes"][0]["takes"][0]["events"].clone();
    assert!(serde_json::from_value::<Document>(v).is_err());
}

#[test]
fn quantize_spec_applies_strength() {
    let q = QuantizeSpec {
        grid: Tick(100),
        strength: 50,
    };
    assert_eq!(q.apply(Tick(140)), Tick(120));
    assert_eq!(q.apply(Tick(160)), Tick(180));
    let zero = QuantizeSpec {
        grid: Tick(0),
        strength: 100,
    };
    assert_eq!(zero.apply(Tick(7)), Tick(7));
}

#[test]
fn drum_row_matches_pitch_and_dest() {
    let row = DrumRow {
        id: core_model::ids::DrumRowId(0),
        name: "Kick".into(),
        note: 36,
        dest: None,
        show_velocity: false,
    };
    assert!(row.matches(36, None));
    assert!(!row.matches(36, Some(DestId(0))));
    assert!(!row.matches(38, None));
}

#[test]
fn empty_document_round_trips() {
    let d = Document::new();
    let back: Document = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
    assert_eq!(back, d);
    assert_eq!(d.settings().history_depth, 200);
    assert_eq!(d.settings().autosave_keep, 10);
}
```


- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test model_types
```

Expected: FAIL to compile — `error[E0432]: unresolved imports `core_model::document`, `core_model::event`, …`.

- [ ] **Step 3: Implement**

Create `crates/core-model/src/document.rs`:

```rust
//! The document: settings, instrument list, outputs, media pool and
//! sequences, plus a derived event index.

use crate::error::{ModelError, Result};
use crate::event::Event;
use crate::ids::{AnyId, Counters, DestId, EventId, LaneId, MediaId, OutputPathId, SeqId, TakeId};
use crate::lane::Lane;
use crate::sequence::Sequence;
use crate::time::Tick;
use im::OrdMap;
use serde::{Deserialize, Serialize};

/// Project-wide settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Undo depth limit (default 200).
    pub history_depth: u32,
    /// Autosave copies to keep (default 10).
    pub autosave_keep: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            history_depth: 200,
            autosave_keep: 10,
        }
    }
}

/// An instrument-list entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    /// Destination ID.
    pub id: DestId,
    /// Display name.
    pub name: String,
    /// Where events go.
    pub kind: DestKind,
    /// Patch sent when the destination is first used.
    pub default_patch: Option<Patch>,
}

/// Destination kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DestKind {
    /// A MIDI endpoint with a UMP group and channel (each 0–15).
    MidiEndpoint {
        /// Opaque endpoint reference (defined by 2b).
        endpoint_ref: String,
        /// UMP group.
        group: u8,
        /// Channel.
        channel: u8,
    },
    /// A hosted plugin instrument (group 0, channel 0).
    PluginInstrument {
        /// Opaque plugin reference.
        plugin_ref: String,
    },
}

/// Program and optional bank `(msb, lsb)`, each 0–127.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    /// Program 0–127.
    pub program: u8,
    /// Bank select.
    pub bank: Option<(u8, u8)>,
}

/// An audio output path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputPath {
    /// Output ID.
    pub id: OutputPathId,
    /// Display name.
    pub name: String,
    /// Opaque engine bus or hardware output reference.
    pub reference: String,
}

/// A media-pool item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaItem {
    /// Media ID.
    pub id: MediaId,
    /// Path relative to the package, e.g. `Media/kick.wav`.
    pub path: String,
    /// Sample rate in Hz (> 0).
    pub sample_rate: u32,
    /// Channel count (> 0).
    pub channels: u16,
    /// Length in sample frames.
    pub length_frames: u64,
    /// Optional content hash.
    pub content_hash: Option<String>,
}

/// Where an event lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventLoc {
    /// Owning sequence.
    pub seq: SeqId,
    /// Owning lane.
    pub lane: LaneId,
    /// Owning take.
    pub take: TakeId,
    /// The event's tick (part of its take key).
    pub tick: Tick,
}

/// A whole project. Immutable from outside the crate: change it with
/// commands. Cloning is cheap (persistent collections).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(into = "DocumentRepr", try_from = "DocumentRepr")]
pub struct Document {
    settings: Settings,
    instruments: OrdMap<DestId, Destination>,
    outputs: OrdMap<OutputPathId, OutputPath>,
    media: OrdMap<MediaId, MediaItem>,
    sequences: OrdMap<SeqId, Sequence>,
    counters: Counters,
    /// Derived, never serialized: event ID -> location.
    index: OrdMap<EventId, EventLoc>,
}

impl Default for Document {
    fn default() -> Self {
        Document::new()
    }
}

/// The serialized shape of a [`Document`]: collections are arrays ordered by
/// ID, each element carrying its own ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentRepr {
    /// Settings.
    pub settings: Settings,
    /// Instrument list.
    pub instruments: Vec<Destination>,
    /// Output paths.
    pub outputs: Vec<OutputPath>,
    /// Media pool.
    pub media: Vec<MediaItem>,
    /// Sequences.
    pub sequences: Vec<Sequence>,
    /// ID counters.
    pub counters: Counters,
}

impl From<Document> for DocumentRepr {
    fn from(d: Document) -> Self {
        DocumentRepr {
            settings: d.settings,
            instruments: d.instruments.values().cloned().collect(),
            outputs: d.outputs.values().cloned().collect(),
            media: d.media.values().cloned().collect(),
            sequences: d.sequences.values().cloned().collect(),
            counters: d.counters,
        }
    }
}

fn collect_unique<K: Ord + Clone + std::fmt::Display, V: Clone>(
    items: Vec<V>,
    key: impl Fn(&V) -> K,
    what: &str,
) -> std::result::Result<OrdMap<K, V>, String> {
    let mut m = OrdMap::new();
    for v in items {
        let k = key(&v);
        if m.insert(k.clone(), v).is_some() {
            return Err(format!("duplicate {what} id {k}"));
        }
    }
    Ok(m)
}

impl TryFrom<DocumentRepr> for Document {
    type Error = String;
    fn try_from(r: DocumentRepr) -> std::result::Result<Self, String> {
        let mut d = Document {
            settings: r.settings,
            instruments: collect_unique(r.instruments, |v| v.id, "destination")?,
            outputs: collect_unique(r.outputs, |v| v.id, "output")?,
            media: collect_unique(r.media, |v| v.id, "media")?,
            sequences: collect_unique(r.sequences, |v| v.id, "sequence")?,
            counters: r.counters,
            index: OrdMap::new(),
        };
        d.rebuild_index()?;
        Ok(d)
    }
}

impl Document {
    /// An empty document with default settings.
    pub fn new() -> Document {
        Document {
            settings: Settings::default(),
            instruments: OrdMap::new(),
            outputs: OrdMap::new(),
            media: OrdMap::new(),
            sequences: OrdMap::new(),
            counters: Counters::default(),
            index: OrdMap::new(),
        }
    }

    /// Builds a document from parts (used by importers and tests). Rejects
    /// duplicate IDs; run [`crate::rules::validate`] for the full checks.
    pub fn from_parts(repr: DocumentRepr) -> Result<Document> {
        Document::try_from(repr).map_err(|p| ModelError::Corrupt { problems: vec![p] })
    }

    /// Settings.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    /// Instrument list.
    pub fn instruments(&self) -> &OrdMap<DestId, Destination> {
        &self.instruments
    }
    /// Output paths.
    pub fn outputs(&self) -> &OrdMap<OutputPathId, OutputPath> {
        &self.outputs
    }
    /// Media pool.
    pub fn media(&self) -> &OrdMap<MediaId, MediaItem> {
        &self.media
    }
    /// Sequences.
    pub fn sequences(&self) -> &OrdMap<SeqId, Sequence> {
        &self.sequences
    }
    /// ID counters.
    pub fn counters(&self) -> &Counters {
        &self.counters
    }

    /// A sequence by ID.
    pub fn sequence(&self, id: SeqId) -> Result<&Sequence> {
        self.sequences
            .get(&id)
            .ok_or(ModelError::UnknownId(AnyId::Seq(id)))
    }

    /// A lane by ID.
    pub fn lane(&self, seq: SeqId, lane: LaneId) -> Result<&Lane> {
        self.sequence(seq)?
            .lane(lane)
            .ok_or(ModelError::UnknownId(AnyId::Lane(lane)))
    }

    /// Where an event lives.
    pub fn locate(&self, id: EventId) -> Result<EventLoc> {
        self.index
            .get(&id)
            .copied()
            .ok_or(ModelError::UnknownId(AnyId::Event(id)))
    }

    /// An event and its location.
    pub fn event(&self, id: EventId) -> Result<(&Event, EventLoc)> {
        let loc = self.locate(id)?;
        let ev = self
            .lane(loc.seq, loc.lane)?
            .take(loc.take)
            .and_then(|t| t.events.get(&(loc.tick, id)))
            .ok_or(ModelError::UnknownId(AnyId::Event(id)))?;
        Ok((ev, loc))
    }

    /// Every event in every take, with its location.
    pub fn all_events(&self) -> impl Iterator<Item = (EventLoc, &Event)> + '_ {
        self.sequences.values().flat_map(|s| {
            s.lanes.iter().flat_map(move |l| {
                l.takes.iter().flat_map(move |t| {
                    t.events.values().map(move |e| {
                        (
                            EventLoc {
                                seq: s.id,
                                lane: l.id,
                                take: t.id,
                                tick: e.tick,
                            },
                            e,
                        )
                    })
                })
            })
        })
    }
}

/// Crate-internal mutation, used only by commands. (`dead_code` is allowed
/// until the command tasks use every function; Task 18 removes the allow.)
#[allow(dead_code)]
impl Document {
    pub(crate) fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }
    pub(crate) fn instruments_mut(&mut self) -> &mut OrdMap<DestId, Destination> {
        &mut self.instruments
    }
    pub(crate) fn outputs_mut(&mut self) -> &mut OrdMap<OutputPathId, OutputPath> {
        &mut self.outputs
    }
    pub(crate) fn media_mut(&mut self) -> &mut OrdMap<MediaId, MediaItem> {
        &mut self.media
    }
    pub(crate) fn counters_mut(&mut self) -> &mut Counters {
        &mut self.counters
    }

    /// Mutable sequence. Callers must not add or remove take events through
    /// it; use the event functions below so the index stays correct.
    pub(crate) fn seq_mut(&mut self, id: SeqId) -> Result<&mut Sequence> {
        self.sequences
            .get_mut(&id)
            .ok_or(ModelError::UnknownId(AnyId::Seq(id)))
    }

    /// Mutable lane, with the same caveat as [`Document::seq_mut`].
    pub(crate) fn lane_mut(&mut self, seq: SeqId, lane: LaneId) -> Result<&mut Lane> {
        self.seq_mut(seq)?
            .lanes
            .iter_mut()
            .find(|l| l.id == lane)
            .ok_or(ModelError::UnknownId(AnyId::Lane(lane)))
    }

    /// Inserts an event into a take (the active take when `take` is None).
    pub(crate) fn insert_event(
        &mut self,
        seq: SeqId,
        lane: LaneId,
        take: Option<TakeId>,
        ev: Event,
    ) -> Result<EventLoc> {
        if self.index.contains_key(&ev.id) {
            return Err(ModelError::invalid(format!(
                "event {} already exists",
                ev.id
            )));
        }
        let l = self.lane_mut(seq, lane)?;
        let take_id = take.unwrap_or(l.active_take);
        let t = l
            .takes
            .iter_mut()
            .find(|t| t.id == take_id)
            .ok_or(ModelError::UnknownId(AnyId::Take(take_id)))?;
        let loc = EventLoc {
            seq,
            lane,
            take: take_id,
            tick: ev.tick,
        };
        let id = ev.id;
        t.events.insert(ev.key(), ev);
        self.index.insert(id, loc);
        Ok(loc)
    }

    /// Removes an event wherever it is.
    pub(crate) fn remove_event(&mut self, id: EventId) -> Result<(Event, EventLoc)> {
        let loc = self.locate(id)?;
        let l = self.lane_mut(loc.seq, loc.lane)?;
        let ev = l
            .takes
            .iter_mut()
            .find(|t| t.id == loc.take)
            .and_then(|t| t.events.remove(&(loc.tick, id)))
            .ok_or(ModelError::UnknownId(AnyId::Event(id)))?;
        self.index.remove(&id);
        Ok((ev, loc))
    }

    /// Replaces an event (matched by ID) in place; its tick may change.
    pub(crate) fn replace_event(&mut self, ev: Event) -> Result<EventLoc> {
        let (_, loc) = self.remove_event(ev.id)?;
        self.insert_event(loc.seq, loc.lane, Some(loc.take), ev)
    }

    fn unindex_seq(&mut self, s: &Sequence) {
        for l in &s.lanes {
            self.unindex_lane(l);
        }
    }

    fn unindex_lane(&mut self, l: &Lane) {
        for t in &l.takes {
            for id in t.events.keys().map(|(_, id)| *id) {
                self.index.remove(&id);
            }
        }
    }

    fn index_seq(&mut self, s: &Sequence) {
        for l in &s.lanes {
            for t in &l.takes {
                for e in t.events.values() {
                    self.index.insert(
                        e.id,
                        EventLoc {
                            seq: s.id,
                            lane: l.id,
                            take: t.id,
                            tick: e.tick,
                        },
                    );
                }
            }
        }
    }

    /// Inserts or replaces a whole sequence, re-indexing its events.
    pub(crate) fn put_sequence(&mut self, s: Sequence) {
        if let Some(old) = self.sequences.get(&s.id).cloned() {
            self.unindex_seq(&old);
        }
        self.index_seq(&s);
        self.sequences.insert(s.id, s);
    }

    /// Removes a sequence and its events from the index.
    pub(crate) fn remove_sequence(&mut self, id: SeqId) -> Result<Sequence> {
        let s = self
            .sequences
            .remove(&id)
            .ok_or(ModelError::UnknownId(AnyId::Seq(id)))?;
        self.unindex_seq(&s);
        Ok(s)
    }

    /// Removes a lane and its events from the index.
    pub(crate) fn remove_lane(&mut self, seq: SeqId, lane: LaneId) -> Result<Lane> {
        let s = self.seq_mut(seq)?;
        let i = s
            .lane_index(lane)
            .ok_or(ModelError::UnknownId(AnyId::Lane(lane)))?;
        let l = s.lanes.remove(i);
        self.unindex_lane(&l);
        Ok(l)
    }

    /// Rebuilds the event index; fails on duplicate event IDs.
    pub(crate) fn rebuild_index(&mut self) -> std::result::Result<(), String> {
        let mut index = OrdMap::new();
        for (loc, e) in self.all_events() {
            if index.insert(e.id, loc).is_some() {
                return Err(format!("duplicate event id {}", e.id));
            }
        }
        self.index = index;
        Ok(())
    }
}
```

Create `crates/core-model/src/event.rs`:

```rust
//! Events: MIDI 2.0 channel-voice data, text, references and audio regions.

use crate::ids::{DestId, EventId, MediaId, SeqId};
use crate::time::Tick;
use im::Vector;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// One event on a lane's take.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    /// Document-unique ID.
    pub id: EventId,
    /// Start position.
    pub tick: Tick,
    /// Destination override (else the lane's MIDI destination).
    pub dest: Option<DestId>,
    /// `(group, channel)` override, each 0–15.
    pub group_channel: Option<(u8, u8)>,
    /// Per-event mute; on a reference it mutes that use.
    pub mute: bool,
    /// What the event is.
    pub body: EventBody,
}

/// An event without an ID, as passed to insert commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventSpec {
    /// Start position.
    pub tick: Tick,
    /// Destination override.
    pub dest: Option<DestId>,
    /// `(group, channel)` override.
    pub group_channel: Option<(u8, u8)>,
    /// Mute flag.
    pub mute: bool,
    /// What the event is.
    pub body: EventBody,
}

impl EventSpec {
    /// A spec with no overrides and no mute.
    pub fn new(tick: Tick, body: EventBody) -> EventSpec {
        EventSpec {
            tick,
            dest: None,
            group_channel: None,
            mute: false,
            body,
        }
    }

    /// Gives the spec an ID.
    pub fn with_id(self, id: EventId) -> Event {
        Event {
            id,
            tick: self.tick,
            dest: self.dest,
            group_channel: self.group_channel,
            mute: self.mute,
            body: self.body,
        }
    }
}

impl Event {
    /// The event without its ID.
    pub fn spec(&self) -> EventSpec {
        EventSpec {
            tick: self.tick,
            dest: self.dest,
            group_channel: self.group_channel,
            mute: self.mute,
            body: self.body.clone(),
        }
    }

    /// The take key `(tick, id)` this event is stored under.
    pub fn key(&self) -> (Tick, EventId) {
        (self.tick, self.id)
    }
}

/// The payload of an event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "snake_case")]
pub enum EventBody {
    /// A note with MIDI 2.0 velocities.
    Note {
        /// Note number 0–127.
        pitch: u8,
        /// 16-bit attack velocity.
        velocity: u16,
        /// 16-bit release velocity.
        release_velocity: u16,
        /// Length in ticks (>= 0).
        duration: Tick,
        /// Optional MIDI 2.0 note attribute.
        attribute: Option<NoteAttribute>,
        /// Per-note controllers and pitch bend.
        per_note: Vector<PerNoteCtl>,
    },
    /// Control change with a 32-bit value.
    Cc {
        /// Controller number 0–127.
        index: u8,
        /// 32-bit value.
        value: u32,
    },
    /// 32-bit pitch bend (0x8000_0000 is centre).
    PitchBend {
        /// 32-bit value.
        value: u32,
    },
    /// 32-bit channel pressure.
    ChannelPressure {
        /// 32-bit value.
        value: u32,
    },
    /// 32-bit polyphonic pressure.
    PolyPressure {
        /// Note number 0–127.
        pitch: u8,
        /// 32-bit value.
        value: u32,
    },
    /// Program change with optional bank select.
    Program {
        /// Program 0–127.
        program: u8,
        /// `(msb, lsb)`, each 0–127.
        bank: Option<(u8, u8)>,
    },
    /// System exclusive data (without F0/F7).
    SysEx {
        /// The payload bytes.
        data: Arc<[u8]>,
        /// 8-bit data allowed; otherwise every byte must be < 0x80.
        eight_bit: bool,
    },
    /// A text-type meta event.
    Text {
        /// Which kind of text.
        kind: TextKind,
        /// The text.
        text: String,
    },
    /// A live reference to another sequence.
    Reference {
        /// The referenced sequence.
        seq: SeqId,
        /// Per-use length (defaults to the sequence's length).
        length: Option<Tick>,
        /// How many times the use repeats.
        loop_count: LoopCount,
        /// Transpose in semitones, applied to notes and poly pressure.
        transpose: i8,
    },
    /// An audio region on the lane's audio output path.
    AudioRegion {
        /// The media item.
        media: MediaId,
        /// Start within the media, in sample frames.
        source_offset: u64,
        /// Length in sample frames (>= 1).
        length: u64,
        /// Gain in dB, finite, -144..=24.
        gain_db: f32,
        /// Fade in.
        fade_in: Fade,
        /// Fade out.
        fade_out: Fade,
    },
}

/// Event kinds, for rules and view filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// [`EventBody::Note`].
    Note,
    /// [`EventBody::Cc`].
    Cc,
    /// [`EventBody::PitchBend`].
    PitchBend,
    /// [`EventBody::ChannelPressure`].
    ChannelPressure,
    /// [`EventBody::PolyPressure`].
    PolyPressure,
    /// [`EventBody::Program`].
    Program,
    /// [`EventBody::SysEx`].
    SysEx,
    /// [`EventBody::Text`].
    Text,
    /// [`EventBody::Reference`].
    Reference,
    /// [`EventBody::AudioRegion`].
    AudioRegion,
}

impl EventBody {
    /// The kind of this body.
    pub fn kind(&self) -> EventKind {
        match self {
            EventBody::Note { .. } => EventKind::Note,
            EventBody::Cc { .. } => EventKind::Cc,
            EventBody::PitchBend { .. } => EventKind::PitchBend,
            EventBody::ChannelPressure { .. } => EventKind::ChannelPressure,
            EventBody::PolyPressure { .. } => EventKind::PolyPressure,
            EventBody::Program { .. } => EventKind::Program,
            EventBody::SysEx { .. } => EventKind::SysEx,
            EventBody::Text { .. } => EventKind::Text,
            EventBody::Reference { .. } => EventKind::Reference,
            EventBody::AudioRegion { .. } => EventKind::AudioRegion,
        }
    }

    /// A plain note with no attribute or per-note controllers.
    pub fn note(pitch: u8, velocity: u16, duration: Tick) -> EventBody {
        EventBody::Note {
            pitch,
            velocity,
            release_velocity: 0,
            duration,
            attribute: None,
            per_note: Vector::new(),
        }
    }

    /// A reference used once at the sequence's own length.
    pub fn reference(seq: SeqId) -> EventBody {
        EventBody::Reference {
            seq,
            length: None,
            loop_count: LoopCount::Count(1),
            transpose: 0,
        }
    }

    /// Whether this body is sent to a MIDI destination (everything except
    /// references and audio regions).
    pub fn is_midi(&self) -> bool {
        !matches!(
            self,
            EventBody::Reference { .. } | EventBody::AudioRegion { .. }
        )
    }
}

/// A MIDI 2.0 note attribute (type and 16-bit data).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteAttribute {
    /// Attribute type (e.g. 3 = pitch 7.9).
    pub kind: u8,
    /// Attribute data.
    pub data: u16,
}

/// A per-note controller value at an offset within the note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerNoteCtl {
    /// Which controller.
    pub kind: PerNoteKind,
    /// Offset from the note start.
    pub offset: Tick,
    /// 32-bit value.
    pub value: u32,
}

/// Per-note controller kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerNoteKind {
    /// Registered per-note controller number.
    Registered(u8),
    /// Assignable per-note controller number.
    Assignable(u8),
    /// Per-note pitch bend.
    PitchBend,
}

/// Text meta-event kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextKind {
    /// Generic text.
    Text,
    /// Lyric.
    Lyric,
    /// Cue point.
    Cue,
    /// Marker.
    Marker,
    /// Copyright notice.
    Copyright,
}

/// How a reference repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopCount {
    /// Play the per-use length this many times (>= 1).
    Count(u32),
    /// Repeat the sequence's own length until the per-use length is filled.
    Fill,
}

/// A re-editable fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fade {
    /// Fade length in ticks (>= 0).
    pub length: Tick,
    /// Curve shape.
    pub shape: FadeShape,
}

impl Default for Fade {
    fn default() -> Self {
        Fade {
            length: Tick::ZERO,
            shape: FadeShape::Linear,
        }
    }
}

/// Fade curve shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FadeShape {
    /// Linear gain.
    Linear,
    /// Equal-power (sine) curve.
    EqualPower,
    /// Exponential curve.
    Exponential,
    /// S-curve.
    SCurve,
}
```

Create `crates/core-model/src/lane.rs`:

```rust
//! Lanes (untyped tracks), takes and drum layouts.

use crate::event::Event;
use crate::ids::{DestId, DrumRowId, EventId, LaneId, OutputPathId, TakeId};
use crate::time::Tick;
use im::{OrdMap, Vector};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A timeline lane holding MIDI events, audio regions or both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lane {
    /// Lane ID.
    pub id: LaneId,
    /// Display name.
    pub name: String,
    /// Default MIDI destination for the lane's events.
    pub midi_dest: Option<DestId>,
    /// Audio output path for the lane's regions.
    pub audio_out: Option<OutputPathId>,
    /// Lane mute.
    pub mute: bool,
    /// Lane solo (within its sequence).
    pub solo: bool,
    /// Record arm.
    pub arm: bool,
    /// Own length; playback of the lane stops there (events at or after
    /// it do not play, and longer notes and references are cut).
    pub length: Option<Tick>,
    /// Repeat `[0, length)` to fill the sequence.
    pub looped: bool,
    /// Recording may not change the length.
    pub length_locked: bool,
    /// Non-destructive play quantize for notes.
    pub play_quantize: Option<QuantizeSpec>,
    /// Non-destructive time shift.
    pub play_shift: Tick,
    /// Present when this is a drum lane.
    pub drum: Option<DrumLayout>,
    /// Alternate takes; never empty.
    pub takes: Vector<Take>,
    /// The take that plays.
    pub active_take: TakeId,
}

impl Lane {
    /// A new lane with one empty take.
    pub fn new(id: LaneId, take: TakeId, name: impl Into<String>) -> Lane {
        Lane {
            id,
            name: name.into(),
            midi_dest: None,
            audio_out: None,
            mute: false,
            solo: false,
            arm: false,
            length: None,
            looped: false,
            length_locked: false,
            play_quantize: None,
            play_shift: Tick::ZERO,
            drum: None,
            takes: Vector::unit(Take::new(take, "Take 1")),
            active_take: take,
        }
    }

    /// The take that plays, if it exists.
    pub fn active(&self) -> Option<&Take> {
        self.take(self.active_take)
    }

    /// A take by ID.
    pub fn take(&self, id: TakeId) -> Option<&Take> {
        self.takes.iter().find(|t| t.id == id)
    }

    /// Index of a take by ID.
    pub fn take_index(&self, id: TakeId) -> Option<usize> {
        self.takes.iter().position(|t| t.id == id)
    }
}

/// One version of a lane's content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Take {
    /// Take ID.
    pub id: TakeId,
    /// Display name.
    pub name: String,
    /// Events ordered by `(tick, id)`. Serialized as an array in that order.
    /// Mutate only through `Document` so the event index stays correct.
    #[serde(with = "events_as_array")]
    pub events: OrdMap<(Tick, EventId), Event>,
}

impl Take {
    /// An empty take.
    pub fn new(id: TakeId, name: impl Into<String>) -> Take {
        Take {
            id,
            name: name.into(),
            events: OrdMap::new(),
        }
    }
}

mod events_as_array {
    use super::*;

    pub fn serialize<S: Serializer>(
        m: &OrdMap<(Tick, EventId), Event>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.collect_seq(m.values())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<OrdMap<(Tick, EventId), Event>, D::Error> {
        let v: Vec<Event> = Vec::deserialize(d)?;
        let mut m = OrdMap::new();
        for e in v {
            if m.insert(e.key(), e.clone()).is_some() {
                return Err(serde::de::Error::custom(format!(
                    "duplicate event {}",
                    e.id
                )));
            }
        }
        Ok(m)
    }
}

/// Non-destructive quantize: snap note starts toward a grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuantizeSpec {
    /// Grid size in ticks (> 0).
    pub grid: Tick,
    /// Strength 0–100 percent.
    pub strength: u8,
}

impl QuantizeSpec {
    /// Applies the quantize to a tick.
    pub fn apply(self, t: Tick) -> Tick {
        if self.grid.0 <= 0 {
            return t;
        }
        let g = self.grid.0;
        let snapped = (t.0.saturating_add(g / 2)).div_euclid(g).saturating_mul(g);
        let delta = i128::from(snapped - t.0) * i128::from(self.strength.min(100)) / 100;
        Tick(t.0.saturating_add(i64::try_from(delta).unwrap_or(0)))
    }
}

/// The rows of a drum lane.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrumLayout {
    /// Rows in display order.
    pub rows: Vector<DrumRow>,
}

/// A drum row: a view over the lane's notes with this pitch and destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrumRow {
    /// Row ID.
    pub id: DrumRowId,
    /// Display name.
    pub name: String,
    /// Note number 0–127.
    pub note: u8,
    /// Destination the row's notes carry as their override (None = lane).
    pub dest: Option<DestId>,
    /// Show velocity in the drum grid.
    pub show_velocity: bool,
}

impl DrumRow {
    /// Whether an event with this pitch and destination override belongs to
    /// the row.
    pub fn matches(&self, pitch: u8, dest: Option<DestId>) -> bool {
        self.note == pitch && self.dest == dest
    }
}
```

Replace the whole of `crates/core-model/src/lib.rs` with:

```rust
//! `core-model`: the VizDAW project model.
//!
//! A pure-Rust crate holding a VizDAW project and every edit that can be made
//! to it. It has no audio, MIDI I/O, UI or OS dependencies. All edits are
//! [`command::Command`] values applied by the pure function
//! [`command::apply`]; documents are immutable values that share structure.
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod document;
pub mod error;
pub mod event;
pub mod ids;
pub mod lane;
pub mod meter;
pub mod sequence;
pub mod tempo;
pub mod time;

pub use error::{ModelError, Result};
```

Create `crates/core-model/src/sequence.rs`:

```rust
//! Sequences: the main unit of work.

use crate::ids::{LaneId, SeqId};
use crate::lane::Lane;
use crate::meter::MeterMap;
use crate::tempo::TempoMap;
use crate::time::Tick;
use im::{OrdMap, Vector};
use serde::{Deserialize, Serialize};

/// A sequence with its own lanes, tempo and meter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sequence {
    /// Sequence ID.
    pub id: SeqId,
    /// Display name.
    pub name: String,
    /// Normal or generated.
    pub kind: SeqKind,
    /// Optional key chord that auditions or inserts this sequence.
    pub keyboard_slot: Option<KeySlot>,
    /// Tempo map (used when played or viewed on its own).
    pub tempo: TempoMap,
    /// Meter map (used when played or viewed on its own).
    pub meter: MeterMap,
    /// Named positions.
    pub markers: Vector<Marker>,
    /// Free-text comment.
    pub comment: String,
    /// Pickup length before bar 1 (>= 0). Content plays from `-start_offset`.
    pub start_offset: Tick,
    /// Derived from the lanes or locked.
    pub length: SeqLength,
    /// Lanes in display order.
    pub lanes: Vector<Lane>,
}

impl Sequence {
    /// A normal sequence at 120 BPM in 4/4 with the given lanes.
    pub fn new(id: SeqId, name: impl Into<String>, lanes: Vector<Lane>) -> Sequence {
        Sequence {
            id,
            name: name.into(),
            kind: SeqKind::Normal,
            keyboard_slot: None,
            tempo: TempoMap::default(),
            meter: MeterMap::default(),
            markers: Vector::new(),
            comment: String::new(),
            start_offset: Tick::ZERO,
            length: SeqLength::Derived,
            lanes,
        }
    }

    /// A lane by ID.
    pub fn lane(&self, id: LaneId) -> Option<&Lane> {
        self.lanes.iter().find(|l| l.id == id)
    }

    /// Index of a lane by ID.
    pub fn lane_index(&self, id: LaneId) -> Option<usize> {
        self.lanes.iter().position(|l| l.id == id)
    }
}

/// Sequence kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeqKind {
    /// Holds ordinary event data.
    Normal,
    /// Produces events from rules; parameters are stored only (2a).
    Generated {
        /// The generator parameters.
        generator: GeneratorParams,
    },
}

/// Opaque generator parameters, stored and round-tripped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorParams {
    /// Generator name.
    pub generator: String,
    /// Parameter values as strings.
    pub params: OrdMap<String, String>,
}

/// A keyboard slot: one key plus a modifier bitmask
/// (1 = shift, 2 = control, 4 = option, 8 = command).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeySlot {
    /// The key, e.g. "a" or "F5".
    pub key: String,
    /// Modifier bitmask.
    pub modifiers: u8,
}

/// A named position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    /// Position.
    pub tick: Tick,
    /// Name.
    pub name: String,
}

/// How a sequence's length is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeqLength {
    /// Follows the longest lane, rounded up to a barline (never stored).
    Derived,
    /// Fixed length in ticks (> 0).
    Locked(Tick),
}
```


- [ ] **Step 4: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test model_types
```

Expected: PASS — `model_types`: `test result: ok. 8 passed`.

- [ ] **Step 5: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/document.rs crates/core-model/src/event.rs crates/core-model/src/lane.rs crates/core-model/src/lib.rs crates/core-model/src/sequence.rs crates/core-model/tests/model_types.rs
git commit -m "feat(core-model): model types, document and event index"
```

---

### Task 5: Integrity rules: cycles, referential integrity, ranges, full validation

**Goal:** Implement spec §4 checks used by commands (`check_*`) and by loading (`validate`, which reports every problem).

**Files:**
- Modify: `crates/core-model/src/lib.rs`
- Create: `crates/core-model/src/rules.rs`
- Create (test): `crates/core-model/tests/rules.rs`

**Interfaces:**
- Consumes: `Document` read API (Task 4), `TempoMap::check`, `MeterMap::check`.
- Produces:
  - `core_model::rules::direct_refs(&Document, SeqId) -> BTreeSet<SeqId>` (all takes).
  - `core_model::rules::path_between(&Document, from, to) -> Option<Vec<SeqId>>`.
  - `core_model::rules::check_reference(&Document, owner: SeqId, target: SeqId) -> Result<()>` — `Cycle { path }` where path starts and ends with `owner`.
  - `core_model::rules::references_to(&Document, SeqId) -> Vec<EventId>`.
  - `core_model::rules::find_cycle(&Document) -> Option<Vec<SeqId>>`.
  - `core_model::rules::check_group_channel(Option<(u8, u8)>) -> Result<()>`, `check_positive(field, Tick) -> Result<()>`.
  - `core_model::rules::check_event_fields(&Document, &EventSpec) -> Result<()>` (ranges + IDs, no cycle check) and `check_event(&Document, owner: SeqId, &EventSpec) -> Result<()>` (adds the cycle check).
  - `core_model::rules::validate(&Document) -> Vec<String>` (empty = valid).

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/rules.rs`:

```rust
use core_model::ModelError;
use core_model::document::{Document, DocumentRepr, MediaItem, Settings};
use core_model::event::{EventBody, EventSpec, LoopCount};
use core_model::ids::{Counters, DestId, EventId, LaneId, MediaId, SeqId, TakeId};
use core_model::lane::Lane;
use core_model::rules::{
    check_event, check_reference, find_cycle, path_between, references_to, validate,
};
use core_model::sequence::Sequence;
use core_model::time::Tick;
use im::Vector;
use std::sync::Arc;

/// `n` sequences (ids 0..n, one lane each) with reference events
/// `(owner, target)` placed in the owner's lane, plus one 1000-frame media item.
fn doc_with_refs(n: u64, refs: &[(u64, u64)]) -> Document {
    let mut seqs = Vec::new();
    let mut next_event = 0;
    for i in 0..n {
        let mut lane = Lane::new(LaneId(i), TakeId(i), "L");
        let take = lane.takes.get_mut(0).unwrap();
        for (o, t) in refs.iter().filter(|(o, _)| *o == i) {
            let ev = EventSpec::new(Tick(0), EventBody::reference(SeqId(*t)))
                .with_id(EventId(next_event));
            next_event += 1;
            let _ = o;
            take.events.insert(ev.key(), ev);
        }
        seqs.push(Sequence::new(SeqId(i), format!("S{i}"), Vector::unit(lane)));
    }
    Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![],
        outputs: vec![],
        media: vec![MediaItem {
            id: MediaId(0),
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 1,
            length_frames: 1000,
            content_hash: None,
        }],
        sequences: seqs,
        counters: Counters {
            seq: n,
            lane: n,
            take: n,
            event: next_event,
            media: 1,
            ..Counters::default()
        },
    })
    .unwrap()
}

#[test]
fn self_reference_is_a_cycle() {
    let d = doc_with_refs(1, &[]);
    assert_eq!(
        check_reference(&d, SeqId(0), SeqId(0)),
        Err(ModelError::Cycle {
            path: vec![SeqId(0), SeqId(0)]
        })
    );
}

#[test]
fn indirect_cycle_reports_full_path() {
    // 0 -> 1 -> 2; adding 2 -> 0 closes the loop.
    let d = doc_with_refs(3, &[(0, 1), (1, 2)]);
    assert_eq!(
        path_between(&d, SeqId(0), SeqId(2)),
        Some(vec![SeqId(0), SeqId(1), SeqId(2)])
    );
    assert_eq!(
        check_reference(&d, SeqId(2), SeqId(0)),
        Err(ModelError::Cycle {
            path: vec![SeqId(2), SeqId(0), SeqId(1), SeqId(2)]
        })
    );
    assert!(check_reference(&d, SeqId(0), SeqId(2)).is_ok());
}

#[test]
fn reference_to_missing_sequence_is_unknown() {
    let d = doc_with_refs(1, &[]);
    assert!(matches!(
        check_reference(&d, SeqId(0), SeqId(9)),
        Err(ModelError::UnknownId(_))
    ));
}

#[test]
fn references_to_lists_users() {
    let d = doc_with_refs(3, &[(0, 2), (1, 2), (0, 1)]);
    let mut users = references_to(&d, SeqId(2));
    users.sort();
    assert_eq!(users, vec![EventId(0), EventId(2)]);
}

#[test]
fn event_range_checks() {
    let d = doc_with_refs(2, &[]);
    let bad = |body| check_event(&d, SeqId(0), &EventSpec::new(Tick(0), body)).is_err();
    assert!(bad(EventBody::note(128, 1, Tick(1))));
    assert!(bad(EventBody::note(60, 1, Tick(-1))));
    assert!(bad(EventBody::Cc {
        index: 200,
        value: 0
    }));
    assert!(bad(EventBody::SysEx {
        data: Arc::from(vec![0x90_u8]),
        eight_bit: false
    }));
    assert!(bad(EventBody::Reference {
        seq: SeqId(1),
        length: Some(Tick(0)),
        loop_count: LoopCount::Count(1),
        transpose: 0
    }));
    assert!(bad(EventBody::Reference {
        seq: SeqId(1),
        length: None,
        loop_count: LoopCount::Count(0),
        transpose: 0
    }));
    assert!(bad(EventBody::AudioRegion {
        media: MediaId(0),
        source_offset: 900,
        length: 200,
        gain_db: 0.0,
        fade_in: Default::default(),
        fade_out: Default::default()
    }));
    assert!(bad(EventBody::AudioRegion {
        media: MediaId(0),
        source_offset: 0,
        length: 10,
        gain_db: f32::NAN,
        fade_in: Default::default(),
        fade_out: Default::default()
    }));
    let mut spec = EventSpec::new(Tick(0), EventBody::note(60, 1, Tick(1)));
    spec.group_channel = Some((0, 16));
    assert!(check_event(&d, SeqId(0), &spec).is_err());
    spec.group_channel = Some((15, 15));
    assert!(check_event(&d, SeqId(0), &spec).is_ok());
    spec.dest = Some(DestId(5));
    assert_eq!(
        check_event(&d, SeqId(0), &spec),
        Err(ModelError::UnknownId(DestId(5).into()))
    );
}

#[test]
fn validate_accepts_a_clean_document() {
    let d = doc_with_refs(3, &[(0, 1), (1, 2)]);
    assert!(validate(&d).is_empty(), "{:?}", validate(&d));
    assert_eq!(find_cycle(&d), None);
}

#[test]
fn validate_reports_every_problem() {
    // A cycle, plus a counter that is too low.
    let d = doc_with_refs(2, &[(0, 1), (1, 0)]);
    let mut v = serde_json::to_value(&d).unwrap();
    v["counters"]["event"] = serde_json::json!(0);
    v["sequences"][0]["tempo"]["points"][0]["bpm"] = serde_json::json!(1);
    let d: Document = serde_json::from_value(v).unwrap();
    let problems = validate(&d);
    assert!(problems.iter().any(|p| p.contains("cycle")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("counters.event")));
    assert!(problems.iter().any(|p| p.contains("tempo.bpm")));
    assert!(problems.len() >= 4);
}
```


- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test rules
```

Expected: FAIL to compile — `error[E0432]: unresolved import `core_model::rules``.

- [ ] **Step 3: Implement**

Replace the whole of `crates/core-model/src/lib.rs` with:

```rust
//! `core-model`: the VizDAW project model.
//!
//! A pure-Rust crate holding a VizDAW project and every edit that can be made
//! to it. It has no audio, MIDI I/O, UI or OS dependencies. All edits are
//! [`command::Command`] values applied by the pure function
//! [`command::apply`]; documents are immutable values that share structure.
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod document;
pub mod error;
pub mod event;
pub mod ids;
pub mod lane;
pub mod meter;
pub mod rules;
pub mod sequence;
pub mod tempo;
pub mod time;

pub use error::{ModelError, Result};
```

Create `crates/core-model/src/rules.rs`:

```rust
//! Rules the model enforces: no reference cycles, referential integrity and
//! value ranges. Commands call the `check_*` functions; loading calls
//! [`validate`], which reports every problem instead of stopping at the first.

use crate::document::{DestKind, Document};
use crate::error::{ModelError, Result};
use crate::event::{EventBody, EventSpec, LoopCount};
use crate::ids::{AnyId, EventId, SeqId};
use crate::time::Tick;
use std::collections::{BTreeMap, BTreeSet};

/// The sequences directly referenced from `seq` (any take).
pub fn direct_refs(doc: &Document, seq: SeqId) -> BTreeSet<SeqId> {
    let mut out = BTreeSet::new();
    if let Ok(s) = doc.sequence(seq) {
        for l in &s.lanes {
            for t in &l.takes {
                for e in t.events.values() {
                    if let EventBody::Reference { seq: target, .. } = e.body {
                        out.insert(target);
                    }
                }
            }
        }
    }
    out
}

/// A reference path from `from` to `to` (inclusive), if one exists.
pub fn path_between(doc: &Document, from: SeqId, to: SeqId) -> Option<Vec<SeqId>> {
    let mut parent: BTreeMap<SeqId, SeqId> = BTreeMap::new();
    let mut seen = BTreeSet::from([from]);
    let mut stack = vec![from];
    while let Some(cur) = stack.pop() {
        if cur == to {
            let mut path = vec![to];
            let mut c = to;
            while let Some(p) = parent.get(&c) {
                path.push(*p);
                c = *p;
            }
            path.reverse();
            return Some(path);
        }
        for next in direct_refs(doc, cur) {
            if seen.insert(next) {
                parent.insert(next, cur);
                stack.push(next);
            }
        }
    }
    None
}

/// Refuses a reference from `owner` to `target` that would close a cycle.
pub fn check_reference(doc: &Document, owner: SeqId, target: SeqId) -> Result<()> {
    doc.sequence(target)?;
    if owner == target {
        return Err(ModelError::Cycle {
            path: vec![owner, owner],
        });
    }
    if let Some(back) = path_between(doc, target, owner) {
        let mut path = vec![owner];
        path.extend(back);
        return Err(ModelError::Cycle { path });
    }
    Ok(())
}

/// Every reference event (any take) that targets `seq`.
pub fn references_to(doc: &Document, seq: SeqId) -> Vec<EventId> {
    doc.all_events()
        .filter(|(_, e)| matches!(e.body, EventBody::Reference { seq: t, .. } if t == seq))
        .map(|(_, e)| e.id)
        .collect()
}

/// Any reference cycle in the document.
pub fn find_cycle(doc: &Document) -> Option<Vec<SeqId>> {
    for &s in doc.sequences().keys() {
        for t in direct_refs(doc, s) {
            if t == s {
                return Some(vec![s, s]);
            }
            if let Some(back) = path_between(doc, t, s) {
                let mut path = vec![s];
                path.extend(back);
                return Some(path);
            }
        }
    }
    None
}

fn need(cond: bool, field: &'static str, value: impl std::fmt::Display) -> Result<()> {
    if cond {
        Ok(())
    } else {
        Err(ModelError::range(field, value))
    }
}

/// Checks a group/channel pair (each 0–15).
pub fn check_group_channel(gc: Option<(u8, u8)>) -> Result<()> {
    if let Some((g, c)) = gc {
        need(g <= 15, "group", g)?;
        need(c <= 15, "channel", c)?;
    }
    Ok(())
}

/// Checks an event's ranges and IDs, but not reference cycles.
pub fn check_event_fields(doc: &Document, ev: &EventSpec) -> Result<()> {
    if let Some(d) = ev.dest {
        if !doc.instruments().contains_key(&d) {
            return Err(ModelError::UnknownId(AnyId::Dest(d)));
        }
    }
    check_group_channel(ev.group_channel)?;
    match &ev.body {
        EventBody::Note {
            pitch, duration, ..
        } => {
            need(*pitch <= 127, "pitch", pitch)?;
            need(duration.0 >= 0, "duration", duration.0)?;
        }
        EventBody::Cc { index, .. } => need(*index <= 127, "cc.index", index)?,
        EventBody::PolyPressure { pitch, .. } => need(*pitch <= 127, "pitch", pitch)?,
        EventBody::Program { program, bank } => {
            need(*program <= 127, "program", program)?;
            if let Some((m, l)) = bank {
                need(*m <= 127 && *l <= 127, "bank", format!("{m}/{l}"))?;
            }
        }
        EventBody::SysEx { data, eight_bit } => {
            need(
                *eight_bit || data.iter().all(|b| *b < 0x80),
                "sysex.data",
                "8-bit byte in 7-bit sysex",
            )?;
        }
        EventBody::Reference {
            seq,
            length,
            loop_count,
            ..
        } => {
            doc.sequence(*seq)?;
            if let Some(l) = length {
                need(l.0 > 0, "reference.length", l.0)?;
            }
            if let LoopCount::Count(n) = loop_count {
                need(*n >= 1, "reference.loop_count", n)?;
            }
        }
        EventBody::AudioRegion {
            media,
            source_offset,
            length,
            gain_db,
            fade_in,
            fade_out,
        } => {
            let m = doc
                .media()
                .get(media)
                .ok_or(ModelError::UnknownId(AnyId::Media(*media)))?;
            need(*length >= 1, "region.length", length)?;
            need(
                source_offset.saturating_add(*length) <= m.length_frames,
                "region.source_end",
                source_offset.saturating_add(*length),
            )?;
            need(
                gain_db.is_finite() && (-144.0..=24.0).contains(gain_db),
                "region.gain_db",
                gain_db,
            )?;
            need(fade_in.length.0 >= 0, "fade_in.length", fade_in.length.0)?;
            need(fade_out.length.0 >= 0, "fade_out.length", fade_out.length.0)?;
        }
        EventBody::PitchBend { .. }
        | EventBody::ChannelPressure { .. }
        | EventBody::Text { .. } => {}
    }
    Ok(())
}

/// Full check for inserting `ev` into sequence `owner`, including cycles.
pub fn check_event(doc: &Document, owner: SeqId, ev: &EventSpec) -> Result<()> {
    check_event_fields(doc, ev)?;
    if let EventBody::Reference { seq, .. } = ev.body {
        check_reference(doc, owner, seq)?;
    }
    Ok(())
}

/// Checks that a length in ticks is greater than zero.
pub fn check_positive(field: &'static str, t: Tick) -> Result<()> {
    need(t.0 > 0, field, t.0)
}

/// Runs every integrity check and returns all problems found (empty when
/// the document is valid).
pub fn validate(doc: &Document) -> Vec<String> {
    let mut p: Vec<String> = Vec::new();
    let c = doc.counters();
    let mut push = |r: Result<()>, ctx: &str| {
        if let Err(e) = r {
            p.push(format!("{ctx}: {e}"));
        }
    };
    for d in doc.instruments().values() {
        if d.id.0 >= c.dest {
            push(Err(ModelError::range("counters.dest", d.id)), "destination");
        }
        if let DestKind::MidiEndpoint { group, channel, .. } = d.kind {
            push(
                check_group_channel(Some((group, channel))),
                &d.id.to_string(),
            );
        }
        if let Some(patch) = d.default_patch {
            let ok = patch.program <= 127 && patch.bank.is_none_or(|(m, l)| m <= 127 && l <= 127);
            push(need(ok, "default_patch", patch.program), &d.id.to_string());
        }
    }
    for o in doc.outputs().values() {
        if o.id.0 >= c.output {
            push(Err(ModelError::range("counters.output", o.id)), "output");
        }
    }
    for m in doc.media().values() {
        if m.id.0 >= c.media {
            push(Err(ModelError::range("counters.media", m.id)), "media");
        }
        push(
            need(
                m.sample_rate > 0 && m.channels > 0,
                "media.format",
                m.sample_rate,
            ),
            &m.id.to_string(),
        );
    }
    let mut lane_ids = BTreeSet::new();
    let mut take_ids = BTreeSet::new();
    let mut row_ids = BTreeSet::new();
    let mut slots = BTreeSet::new();
    for s in doc.sequences().values() {
        let ctx = s.id.to_string();
        if s.id.0 >= c.seq {
            push(Err(ModelError::range("counters.seq", s.id)), &ctx);
        }
        push(s.tempo.check(), &ctx);
        push(s.meter.check(), &ctx);
        push(
            need(s.start_offset.0 >= 0, "start_offset", s.start_offset.0),
            &ctx,
        );
        if let crate::sequence::SeqLength::Locked(t) = s.length {
            push(check_positive("length", t), &ctx);
        }
        if let Some(k) = &s.keyboard_slot {
            if !slots.insert(k.clone()) {
                push(Err(ModelError::invalid("duplicate keyboard slot")), &ctx);
            }
        }
        for l in &s.lanes {
            let lctx = format!("{ctx}/{}", l.id);
            if l.id.0 >= c.lane || !lane_ids.insert(l.id) {
                push(Err(ModelError::range("lane.id", l.id)), &lctx);
            }
            if let Some(d) = l.midi_dest {
                if !doc.instruments().contains_key(&d) {
                    push(Err(ModelError::UnknownId(AnyId::Dest(d))), &lctx);
                }
            }
            if let Some(o) = l.audio_out {
                if !doc.outputs().contains_key(&o) {
                    push(Err(ModelError::UnknownId(AnyId::Output(o))), &lctx);
                }
            }
            if let Some(len) = l.length {
                push(check_positive("lane.length", len), &lctx);
            }
            if let Some(q) = l.play_quantize {
                push(
                    need(q.grid.0 > 0 && q.strength <= 100, "play_quantize", q.grid.0),
                    &lctx,
                );
            }
            if l.take(l.active_take).is_none() {
                push(
                    Err(ModelError::UnknownId(AnyId::Take(l.active_take))),
                    &lctx,
                );
            }
            if let Some(drum) = &l.drum {
                for r in &drum.rows {
                    if r.id.0 >= c.drum_row || !row_ids.insert(r.id) {
                        push(Err(ModelError::range("drum_row.id", r.id)), &lctx);
                    }
                    push(need(r.note <= 127, "drum_row.note", r.note), &lctx);
                    if let Some(d) = r.dest {
                        if !doc.instruments().contains_key(&d) {
                            push(Err(ModelError::UnknownId(AnyId::Dest(d))), &lctx);
                        }
                    }
                }
            }
            for t in &l.takes {
                if t.id.0 >= c.take || !take_ids.insert(t.id) {
                    push(Err(ModelError::range("take.id", t.id)), &lctx);
                }
                for e in t.events.values() {
                    let ectx = format!("{lctx}/{}", e.id);
                    if e.id.0 >= c.event {
                        push(Err(ModelError::range("counters.event", e.id)), &ectx);
                    }
                    push(check_event_fields(doc, &e.spec()), &ectx);
                }
            }
        }
    }
    if let Some(path) = find_cycle(doc) {
        p.push(ModelError::Cycle { path }.to_string());
    }
    p
}
```


- [ ] **Step 4: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test rules
```

Expected: PASS — `rules`: `test result: ok. 7 passed`.

- [ ] **Step 5: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/lib.rs crates/core-model/src/rules.rs crates/core-model/tests/rules.rs
git commit -m "feat(core-model): integrity rules and validation"
```

---

### Task 6: Command core, selection, and sequence commands

**Goal:** Add the pure `apply`/`apply_in` entry points, `Report`/`Applied`/`Context`, the `Selection` and `ViewFilter` types, and every sequence command (new, duplicate, rename, delete with confirmation, kind, keyboard slot, tempo, meter, markers, start offset, length, comment).

**Files:**
- Create: `crates/core-model/src/command/mod.rs`
- Create: `crates/core-model/src/command/sequence.rs`
- Modify: `crates/core-model/src/lib.rs`
- Create: `crates/core-model/src/selection.rs`
- Create (test): `crates/core-model/tests/cmd_sequence.rs`

**Interfaces:**
- Consumes: `Document` crate API (Task 4); `rules::{references_to, check_positive}` (Task 5).
- Produces:
  - `core_model::selection::{Selection { time_range: Option<(Tick, Tick)>, lanes: OrdSet<(SeqId, LaneId)>, events: OrdSet<EventId> }, ViewFilter { hidden_kinds: OrdSet<EventKind>, hidden_lanes: OrdSet<LaneId> }}`; `Selection::retain_existing(&Document) -> Selection`; `ViewFilter::allows(LaneId, EventKind) -> bool`.
  - `core_model::command::{Command, Report { created, removed: Vec<AnyId>, warnings: Vec<String> }, Applied { doc, report, selection: Option<Selection> }, Context<'a> { selection: &'a Selection, filter: &'a ViewFilter }}`.
  - `core_model::command::apply(&Document, &Command) -> Result<Applied>` and `apply_in(&Document, Context<'_>, &Command) -> Result<Applied>`; `Command::name(&self) -> &'static str`.
  - `Command::Sequence(SequenceCmd)`; `SequenceCmd::{New { name }, Duplicate { seq }, Rename { seq, name }, Delete { seq, delete_references }, SetKind { seq, kind }, SetKeyboardSlot { seq, slot }, SetTempo { seq, tempo }, SetMeter { seq, meter }, SetMarkers { seq, markers }, SetStartOffset { seq, offset }, SetLength { seq, length }, SetComment { seq, comment }}`.
  - Crate-internal: `command::sequence::clone_sequence(&mut Document, src: SeqId, name: String) -> Result<SeqId>` (fresh lane/take/event/drum-row IDs; used by Task 12).
  - Every later command group is added the same way: a `pub mod x;` + `pub use x::XCmd;` in `command/mod.rs`, a `Command::X(XCmd)` variant, a `name()` arm and an `apply_in` arm calling `x::apply(&mut d, c, &mut report)?`.

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/cmd_sequence.rs`:

```rust
use core_model::ModelError;
use core_model::command::{Command, SequenceCmd, apply};
use core_model::document::{Document, DocumentRepr, Settings};
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{AnyId, Counters, EventId, LaneId, SeqId, TakeId};
use core_model::lane::Lane;
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::rules::validate;
use core_model::sequence::{KeySlot, SeqLength, Sequence};
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::{PPQ, Tick};
use im::Vector;

fn run(d: &Document, c: SequenceCmd) -> core_model::Result<Document> {
    apply(d, &Command::Sequence(c)).map(|a| a.doc)
}

/// Seq 0 ("Song") references seq 1 ("Verse") twice; seq 1 holds one note.
fn song() -> Document {
    let mut song_lane = Lane::new(LaneId(0), TakeId(0), "L");
    for (i, t) in [0, PPQ * 4].into_iter().enumerate() {
        let ev = EventSpec::new(Tick(t), EventBody::reference(SeqId(1))).with_id(EventId(i as u64));
        song_lane
            .takes
            .get_mut(0)
            .unwrap()
            .events
            .insert(ev.key(), ev);
    }
    let mut verse_lane = Lane::new(LaneId(1), TakeId(1), "L");
    let n = EventSpec::new(Tick(0), EventBody::note(60, 100, Tick(PPQ))).with_id(EventId(2));
    verse_lane
        .takes
        .get_mut(0)
        .unwrap()
        .events
        .insert(n.key(), n);
    Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![],
        outputs: vec![],
        media: vec![],
        sequences: vec![
            Sequence::new(SeqId(0), "Song", Vector::unit(song_lane)),
            Sequence::new(SeqId(1), "Verse", Vector::unit(verse_lane)),
        ],
        counters: Counters {
            seq: 2,
            lane: 2,
            take: 2,
            event: 3,
            ..Counters::default()
        },
    })
    .unwrap()
}

#[test]
fn new_sequence_has_one_lane_with_one_take() {
    let a = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "A".into() }),
    )
    .unwrap();
    assert_eq!(
        a.report.created,
        vec![
            AnyId::Seq(SeqId(0)),
            AnyId::Lane(LaneId(0)),
            AnyId::Take(TakeId(0))
        ]
    );
    let s = a.doc.sequence(SeqId(0)).unwrap();
    assert_eq!(s.lanes.len(), 1);
    assert_eq!(s.lanes[0].takes.len(), 1);
    assert!(validate(&a.doc).is_empty());
}

#[test]
fn apply_is_pure() {
    let d = song();
    let before = d.clone();
    let _ = run(
        &d,
        SequenceCmd::Rename {
            seq: SeqId(0),
            name: "X".into(),
        },
    )
    .unwrap();
    assert_eq!(d, before);
}

#[test]
fn duplicate_gets_fresh_ids_and_no_slot() {
    let d = run(
        &song(),
        SequenceCmd::SetKeyboardSlot {
            seq: SeqId(1),
            slot: Some(KeySlot {
                key: "v".into(),
                modifiers: 0,
            }),
        },
    )
    .unwrap();
    let d = run(&d, SequenceCmd::Duplicate { seq: SeqId(1) }).unwrap();
    let copy = d.sequence(SeqId(2)).unwrap();
    assert_eq!(copy.name, "Verse copy");
    assert_eq!(copy.keyboard_slot, None);
    assert_ne!(copy.lanes[0].id, LaneId(1));
    let ev = copy.lanes[0]
        .active()
        .unwrap()
        .events
        .values()
        .next()
        .unwrap();
    assert_eq!(ev.id, EventId(3));
    assert!(d.event(EventId(3)).is_ok());
    assert!(validate(&d).is_empty(), "{:?}", validate(&d));
}

#[test]
fn deleting_a_used_sequence_needs_confirmation() {
    let d = song();
    let err = run(
        &d,
        SequenceCmd::Delete {
            seq: SeqId(1),
            delete_references: false,
        },
    )
    .unwrap_err();
    assert_eq!(
        err,
        ModelError::InUse {
            users: vec![AnyId::Event(EventId(0)), AnyId::Event(EventId(1))]
        }
    );
    let a = apply(
        &d,
        &Command::Sequence(SequenceCmd::Delete {
            seq: SeqId(1),
            delete_references: true,
        }),
    )
    .unwrap();
    assert_eq!(
        a.report.removed,
        vec![
            AnyId::Event(EventId(0)),
            AnyId::Event(EventId(1)),
            AnyId::Seq(SeqId(1))
        ]
    );
    assert!(a.doc.sequence(SeqId(1)).is_err());
    assert!(a.doc.event(EventId(2)).is_err());
    assert!(validate(&a.doc).is_empty());
}

#[test]
fn unused_sequence_deletes_without_confirmation() {
    let d = run(
        &song(),
        SequenceCmd::Delete {
            seq: SeqId(0),
            delete_references: false,
        },
    )
    .unwrap();
    assert_eq!(d.sequences().len(), 1);
}

#[test]
fn keyboard_slots_are_unique() {
    let slot = KeySlot {
        key: "a".into(),
        modifiers: 8,
    };
    let d = run(
        &song(),
        SequenceCmd::SetKeyboardSlot {
            seq: SeqId(0),
            slot: Some(slot.clone()),
        },
    )
    .unwrap();
    assert!(matches!(
        run(
            &d,
            SequenceCmd::SetKeyboardSlot {
                seq: SeqId(1),
                slot: Some(slot.clone())
            }
        ),
        Err(ModelError::Invalid { .. })
    ));
    // Re-assigning the same slot to its owner is fine.
    assert!(
        run(
            &d,
            SequenceCmd::SetKeyboardSlot {
                seq: SeqId(0),
                slot: Some(slot)
            }
        )
        .is_ok()
    );
}

#[test]
fn tempo_meter_and_ranges_are_validated() {
    let d = song();
    let bad_tempo = TempoMap::constant(Bpm::whole(120)).unwrap();
    assert!(
        run(
            &d,
            SequenceCmd::SetTempo {
                seq: SeqId(0),
                tempo: bad_tempo
            }
        )
        .is_ok()
    );
    let ramp = TempoMap::new(vec![
        TempoPoint {
            tick: Tick(0),
            bpm: Bpm::whole(60),
            ramp: true,
        },
        TempoPoint {
            tick: Tick(PPQ * 8),
            bpm: Bpm::whole(180),
            ramp: false,
        },
    ])
    .unwrap();
    let d2 = run(
        &d,
        SequenceCmd::SetTempo {
            seq: SeqId(0),
            tempo: ramp.clone(),
        },
    )
    .unwrap();
    assert_eq!(d2.sequence(SeqId(0)).unwrap().tempo, ramp);
    let meter = MeterMap::new(vec![MeterEntry {
        tick: Tick(0),
        meter: Meter { num: 3, den: 4 },
        key: KeySig::default(),
    }])
    .unwrap();
    assert!(
        run(
            &d,
            SequenceCmd::SetMeter {
                seq: SeqId(0),
                meter
            }
        )
        .is_ok()
    );
    assert!(matches!(
        run(
            &d,
            SequenceCmd::SetStartOffset {
                seq: SeqId(0),
                offset: Tick(-1)
            }
        ),
        Err(ModelError::OutOfRange { .. })
    ));
    assert!(
        run(
            &d,
            SequenceCmd::SetStartOffset {
                seq: SeqId(0),
                offset: Tick(PPQ)
            }
        )
        .is_ok()
    );
    assert!(matches!(
        run(
            &d,
            SequenceCmd::SetLength {
                seq: SeqId(0),
                length: SeqLength::Locked(Tick(0))
            }
        ),
        Err(ModelError::OutOfRange { .. })
    ));
    assert!(matches!(
        run(
            &d,
            SequenceCmd::Rename {
                seq: SeqId(9),
                name: "x".into()
            }
        ),
        Err(ModelError::UnknownId(AnyId::Seq(SeqId(9))))
    ));
}

#[test]
fn command_names_are_stable() {
    assert_eq!(
        Command::Sequence(SequenceCmd::New {
            name: String::new()
        })
        .name(),
        "New Sequence"
    );
}
```


- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_sequence
```

Expected: FAIL to compile — `error[E0432]: unresolved import `core_model::command``.

- [ ] **Step 3: Implement**

Create `crates/core-model/src/command/mod.rs`:

```rust
//! Commands: every edit is a `Command` applied by the pure [`apply`].

pub mod sequence;

pub use sequence::SequenceCmd;

use crate::document::Document;
use crate::error::Result;
use crate::ids::AnyId;
use crate::selection::{Selection, ViewFilter};

/// What a command did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// IDs created, in creation order.
    pub created: Vec<AnyId>,
    /// IDs removed.
    pub removed: Vec<AnyId>,
    /// Non-fatal notes for the user.
    pub warnings: Vec<String>,
}

/// The result of applying a command.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// The new document.
    pub doc: Document,
    /// What changed.
    pub report: Report,
    /// The new selection, for commands that change it.
    pub selection: Option<Selection>,
}

/// The current selection and view filter, for commands that act on them.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The document's selection.
    pub selection: &'a Selection,
    /// The view filter of the view issuing the command.
    pub filter: &'a ViewFilter,
}

/// Every edit the model supports.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Sequence-level edits.
    Sequence(SequenceCmd),
}

impl Command {
    /// The stable name, used for key bindings and undo labels.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Sequence(c) => c.name(),
        }
    }
}

/// Applies a command with an empty selection and no view filter.
pub fn apply(doc: &Document, cmd: &Command) -> Result<Applied> {
    let selection = Selection::default();
    let filter = ViewFilter::default();
    apply_in(
        doc,
        Context {
            selection: &selection,
            filter: &filter,
        },
        cmd,
    )
}

/// Applies a command in the context of a selection and view filter. Pure:
/// `doc` is never changed; on error nothing is returned but the error.
pub fn apply_in(doc: &Document, ctx: Context<'_>, cmd: &Command) -> Result<Applied> {
    let mut d = doc.clone();
    let mut report = Report::default();
    let _ = ctx;
    let selection = match cmd {
        Command::Sequence(c) => {
            sequence::apply(&mut d, c, &mut report)?;
            None
        }
    };
    Ok(Applied {
        doc: d,
        report,
        selection,
    })
}
```

Create `crates/core-model/src/command/sequence.rs`:

```rust
//! Sequence commands.

use super::Report;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::ids::{AnyId, SeqId};
use crate::lane::{DrumLayout, Lane, Take};
use crate::meter::MeterMap;
use crate::rules::references_to;
use crate::sequence::{KeySlot, Marker, SeqKind, SeqLength, Sequence};
use crate::tempo::TempoMap;
use crate::time::Tick;
use im::{OrdMap, Vector};

/// Sequence-level edits.
#[derive(Debug, Clone, PartialEq)]
pub enum SequenceCmd {
    /// A new normal sequence with one empty lane.
    New {
        /// Name of the new sequence.
        name: String,
    },
    /// A deep copy with fresh IDs (no keyboard slot).
    Duplicate {
        /// The sequence to copy.
        seq: SeqId,
    },
    /// Rename.
    Rename {
        /// Target.
        seq: SeqId,
        /// New name.
        name: String,
    },
    /// Delete. A referenced sequence needs `delete_references: true`, which
    /// also removes every reference to it (listed in the report).
    Delete {
        /// Target.
        seq: SeqId,
        /// Confirmation to remove references.
        delete_references: bool,
    },
    /// Normal or generated.
    SetKind {
        /// Target.
        seq: SeqId,
        /// New kind.
        kind: SeqKind,
    },
    /// Assign or clear the keyboard slot (unique in the project).
    SetKeyboardSlot {
        /// Target.
        seq: SeqId,
        /// New slot.
        slot: Option<KeySlot>,
    },
    /// Replace the tempo map.
    SetTempo {
        /// Target.
        seq: SeqId,
        /// New map.
        tempo: TempoMap,
    },
    /// Replace the meter map.
    SetMeter {
        /// Target.
        seq: SeqId,
        /// New map.
        meter: MeterMap,
    },
    /// Replace the markers.
    SetMarkers {
        /// Target.
        seq: SeqId,
        /// New markers.
        markers: Vector<Marker>,
    },
    /// Set the pickup length (>= 0).
    SetStartOffset {
        /// Target.
        seq: SeqId,
        /// New offset.
        offset: Tick,
    },
    /// Derive or lock the length.
    SetLength {
        /// Target.
        seq: SeqId,
        /// New length mode.
        length: SeqLength,
    },
    /// Set the comment.
    SetComment {
        /// Target.
        seq: SeqId,
        /// New comment.
        comment: String,
    },
}

impl SequenceCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            SequenceCmd::New { .. } => "New Sequence",
            SequenceCmd::Duplicate { .. } => "Duplicate Sequence",
            SequenceCmd::Rename { .. } => "Rename Sequence",
            SequenceCmd::Delete { .. } => "Delete Sequence",
            SequenceCmd::SetKind { .. } => "Set Sequence Kind",
            SequenceCmd::SetKeyboardSlot { .. } => "Set Keyboard Slot",
            SequenceCmd::SetTempo { .. } => "Set Tempo",
            SequenceCmd::SetMeter { .. } => "Set Meter",
            SequenceCmd::SetMarkers { .. } => "Set Markers",
            SequenceCmd::SetStartOffset { .. } => "Set Start Offset",
            SequenceCmd::SetLength { .. } => "Set Sequence Length",
            SequenceCmd::SetComment { .. } => "Set Comment",
        }
    }
}

/// Copies sequence `src` under a new ID with fresh lane, take, event and
/// drum-row IDs. Returns the new ID.
pub(crate) fn clone_sequence(d: &mut Document, src: SeqId, name: String) -> Result<SeqId> {
    let s = d.sequence(src)?.clone();
    let c = d.counters_mut();
    let new_id = c.alloc_seq();
    let mut lanes = Vector::new();
    for l in &s.lanes {
        let mut nl = l.clone();
        nl.id = c.alloc_lane();
        let mut takes = Vector::new();
        for t in &l.takes {
            let tid = c.alloc_take();
            if t.id == l.active_take {
                nl.active_take = tid;
            }
            let mut events = OrdMap::new();
            for e in t.events.values() {
                let mut ne = e.clone();
                ne.id = c.alloc_event();
                events.insert(ne.key(), ne);
            }
            takes.push_back(Take {
                id: tid,
                name: t.name.clone(),
                events,
            });
        }
        nl.takes = takes;
        nl.drum = l.drum.as_ref().map(|dl| DrumLayout {
            rows: dl
                .rows
                .iter()
                .map(|r| {
                    let mut nr = r.clone();
                    nr.id = c.alloc_drum_row();
                    nr
                })
                .collect(),
        });
        lanes.push_back(nl);
    }
    let copy = Sequence {
        id: new_id,
        name,
        keyboard_slot: None,
        lanes,
        ..s
    };
    d.put_sequence(copy);
    Ok(new_id)
}

pub(crate) fn apply(d: &mut Document, cmd: &SequenceCmd, r: &mut Report) -> Result<()> {
    match cmd {
        SequenceCmd::New { name } => {
            let c = d.counters_mut();
            let id = c.alloc_seq();
            let lane = c.alloc_lane();
            let take = c.alloc_take();
            let s = Sequence::new(
                id,
                name.clone(),
                Vector::unit(Lane::new(lane, take, "Lane 1")),
            );
            d.put_sequence(s);
            r.created
                .extend([AnyId::Seq(id), AnyId::Lane(lane), AnyId::Take(take)]);
        }
        SequenceCmd::Duplicate { seq } => {
            let name = format!("{} copy", d.sequence(*seq)?.name);
            let id = clone_sequence(d, *seq, name)?;
            r.created.push(AnyId::Seq(id));
        }
        SequenceCmd::Rename { seq, name } => d.seq_mut(*seq)?.name = name.clone(),
        SequenceCmd::Delete {
            seq,
            delete_references,
        } => {
            d.sequence(*seq)?;
            let users = references_to(d, *seq);
            if !users.is_empty() && !delete_references {
                return Err(ModelError::InUse {
                    users: users.into_iter().map(AnyId::Event).collect(),
                });
            }
            for u in users {
                d.remove_event(u)?;
                r.removed.push(AnyId::Event(u));
            }
            d.remove_sequence(*seq)?;
            r.removed.push(AnyId::Seq(*seq));
        }
        SequenceCmd::SetKind { seq, kind } => d.seq_mut(*seq)?.kind = kind.clone(),
        SequenceCmd::SetKeyboardSlot { seq, slot } => {
            d.sequence(*seq)?;
            if let Some(k) = slot {
                if let Some(other) = d
                    .sequences()
                    .values()
                    .find(|s| s.id != *seq && s.keyboard_slot.as_ref() == Some(k))
                {
                    return Err(ModelError::invalid(format!(
                        "keyboard slot already used by {}",
                        other.id
                    )));
                }
            }
            d.seq_mut(*seq)?.keyboard_slot = slot.clone();
        }
        SequenceCmd::SetTempo { seq, tempo } => {
            tempo.check()?;
            d.seq_mut(*seq)?.tempo = tempo.clone();
        }
        SequenceCmd::SetMeter { seq, meter } => {
            meter.check()?;
            d.seq_mut(*seq)?.meter = meter.clone();
        }
        SequenceCmd::SetMarkers { seq, markers } => d.seq_mut(*seq)?.markers = markers.clone(),
        SequenceCmd::SetStartOffset { seq, offset } => {
            if offset.0 < 0 {
                return Err(ModelError::range("start_offset", offset.0));
            }
            d.seq_mut(*seq)?.start_offset = *offset;
        }
        SequenceCmd::SetLength { seq, length } => {
            if let SeqLength::Locked(t) = length {
                crate::rules::check_positive("length", *t)?;
            }
            d.seq_mut(*seq)?.length = *length;
        }
        SequenceCmd::SetComment { seq, comment } => d.seq_mut(*seq)?.comment = comment.clone(),
    }
    Ok(())
}
```

Replace the whole of `crates/core-model/src/lib.rs` with:

```rust
//! `core-model`: the VizDAW project model.
//!
//! A pure-Rust crate holding a VizDAW project and every edit that can be made
//! to it. It has no audio, MIDI I/O, UI or OS dependencies. All edits are
//! [`command::Command`] values applied by the pure function
//! [`command::apply`]; documents are immutable values that share structure.
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod command;
pub mod document;
pub mod error;
pub mod event;
pub mod ids;
pub mod lane;
pub mod meter;
pub mod rules;
pub mod selection;
pub mod sequence;
pub mod tempo;
pub mod time;

pub use error::{ModelError, Result};
```

Create `crates/core-model/src/selection.rs`:

```rust
//! The document-wide selection and view filters.

use crate::document::Document;
use crate::event::EventKind;
use crate::ids::{EventId, LaneId, SeqId};
use crate::time::Tick;
use im::OrdSet;

/// The single selection of an open document: an edit-in/edit-out range,
/// selected lanes and selected events. Changing it is not an undo step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// `[start, end)` time range.
    pub time_range: Option<(Tick, Tick)>,
    /// Selected lanes.
    pub lanes: OrdSet<(SeqId, LaneId)>,
    /// Selected events.
    pub events: OrdSet<EventId>,
}

impl Selection {
    /// Drops lanes and events that no longer exist in `doc` (after undo,
    /// delete and so on).
    pub fn retain_existing(&self, doc: &Document) -> Selection {
        Selection {
            time_range: self.time_range,
            lanes: self
                .lanes
                .iter()
                .filter(|(s, l)| doc.lane(*s, *l).is_ok())
                .copied()
                .collect(),
            events: self
                .events
                .iter()
                .filter(|e| doc.locate(**e).is_ok())
                .copied()
                .collect(),
        }
    }
}

/// What a view hides. Hidden events can't be selected or changed through
/// that view's commands or queries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewFilter {
    /// Event kinds the view hides.
    pub hidden_kinds: OrdSet<EventKind>,
    /// Lanes the view hides.
    pub hidden_lanes: OrdSet<LaneId>,
}

impl ViewFilter {
    /// Whether an event of `kind` on `lane` is visible.
    pub fn allows(&self, lane: LaneId, kind: EventKind) -> bool {
        !self.hidden_lanes.contains(&lane) && !self.hidden_kinds.contains(&kind)
    }
}
```


- [ ] **Step 4: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_sequence
```

Expected: PASS — `cmd_sequence`: `test result: ok. 8 passed`.

- [ ] **Step 5: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/command/mod.rs crates/core-model/src/command/sequence.rs crates/core-model/src/lib.rs crates/core-model/src/selection.rs crates/core-model/tests/cmd_sequence.rs
git commit -m "feat(core-model): command core, selection and sequence commands"
```

---

### Task 7: Lane and take commands

**Goal:** Add, remove, reorder and rename lanes; set destination/output, mute/solo/arm, own length/loop/lock, play quantize/shift; add takes and switch the active take.

**Files:**
- Create: `crates/core-model/src/command/lane.rs`
- Modify: `crates/core-model/src/command/mod.rs`
- Create (test): `crates/core-model/tests/cmd_lane.rs`

**Interfaces:**
- Consumes: `Command`/`Report` plumbing (Task 6); `Document::{remove_lane, lane_mut, counters_mut}` (Task 4); `rules::check_positive` (Task 5).
- Produces:
  - `Command::Lane(LaneCmd)`; `LaneCmd::{Add { seq, name, index: Option<usize> }, Remove { seq, lane }, Reorder { seq, lane, to }, Rename { seq, lane, name }, SetDest { seq, lane, dest }, SetOutput { seq, lane, output }, SetFlags { seq, lane, mute, solo, arm: Option<bool> }, SetLength { seq, lane, length: Option<Tick>, looped, locked }, SetPlayQuantize { seq, lane, quantize }, SetPlayShift { seq, lane, shift }, AddTake { seq, lane, name }, SetActiveTake { seq, lane, take }}`.

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/cmd_lane.rs`:

```rust
use core_model::ModelError;
use core_model::command::{Command, LaneCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Destination, Document, DocumentRepr, Settings};
use core_model::ids::{AnyId, Counters, DestId, LaneId, OutputPathId, SeqId, TakeId};
use core_model::lane::QuantizeSpec;
use core_model::rules::validate;
use core_model::time::Tick;

fn run(d: &Document, c: LaneCmd) -> core_model::Result<Document> {
    apply(d, &Command::Lane(c)).map(|a| a.doc)
}

/// One destination (dest0) and one new sequence seq0 with lane0/take0.
fn base() -> Document {
    let d = Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![Destination {
            id: DestId(0),
            name: "Piano".into(),
            kind: DestKind::PluginInstrument {
                plugin_ref: "p".into(),
            },
            default_patch: None,
        }],
        outputs: vec![],
        media: vec![],
        sequences: vec![],
        counters: Counters {
            dest: 1,
            ..Counters::default()
        },
    })
    .unwrap();
    apply(
        &d,
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc
}

fn lane_ids(d: &Document) -> Vec<LaneId> {
    d.sequence(SeqId(0))
        .unwrap()
        .lanes
        .iter()
        .map(|l| l.id)
        .collect()
}

#[test]
fn add_inserts_at_index_and_reorder_moves() {
    let d = run(
        &base(),
        LaneCmd::Add {
            seq: SeqId(0),
            name: "B".into(),
            index: None,
        },
    )
    .unwrap();
    let d = run(
        &d,
        LaneCmd::Add {
            seq: SeqId(0),
            name: "C".into(),
            index: Some(0),
        },
    )
    .unwrap();
    assert_eq!(lane_ids(&d), vec![LaneId(2), LaneId(0), LaneId(1)]);
    let d = run(
        &d,
        LaneCmd::Reorder {
            seq: SeqId(0),
            lane: LaneId(2),
            to: 99,
        },
    )
    .unwrap();
    assert_eq!(lane_ids(&d), vec![LaneId(0), LaneId(1), LaneId(2)]);
    assert!(validate(&d).is_empty());
}

#[test]
fn remove_reports_lane() {
    let a = apply(
        &base(),
        &Command::Lane(LaneCmd::Remove {
            seq: SeqId(0),
            lane: LaneId(0),
        }),
    )
    .unwrap();
    assert_eq!(a.report.removed, vec![AnyId::Lane(LaneId(0))]);
    assert!(lane_ids(&a.doc).is_empty());
}

#[test]
fn destination_and_output_must_exist() {
    let d = run(
        &base(),
        LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(DestId(0)),
        },
    )
    .unwrap();
    assert_eq!(
        d.lane(SeqId(0), LaneId(0)).unwrap().midi_dest,
        Some(DestId(0))
    );
    assert_eq!(
        run(
            &d,
            LaneCmd::SetDest {
                seq: SeqId(0),
                lane: LaneId(0),
                dest: Some(DestId(7))
            }
        ),
        Err(ModelError::UnknownId(AnyId::Dest(DestId(7))))
    );
    assert_eq!(
        run(
            &d,
            LaneCmd::SetOutput {
                seq: SeqId(0),
                lane: LaneId(0),
                output: Some(OutputPathId(0))
            }
        ),
        Err(ModelError::UnknownId(AnyId::Output(OutputPathId(0))))
    );
}

#[test]
fn flags_change_only_given_values() {
    let d = run(
        &base(),
        LaneCmd::SetFlags {
            seq: SeqId(0),
            lane: LaneId(0),
            mute: Some(true),
            solo: None,
            arm: Some(true),
        },
    )
    .unwrap();
    let d = run(
        &d,
        LaneCmd::SetFlags {
            seq: SeqId(0),
            lane: LaneId(0),
            mute: None,
            solo: Some(true),
            arm: None,
        },
    )
    .unwrap();
    let l = d.lane(SeqId(0), LaneId(0)).unwrap();
    assert!(l.mute && l.solo && l.arm);
}

#[test]
fn length_quantize_and_shift_are_validated() {
    let id = (SeqId(0), LaneId(0));
    assert!(matches!(
        run(
            &base(),
            LaneCmd::SetLength {
                seq: id.0,
                lane: id.1,
                length: Some(Tick(0)),
                looped: true,
                locked: false
            }
        ),
        Err(ModelError::OutOfRange { .. })
    ));
    let d = run(
        &base(),
        LaneCmd::SetLength {
            seq: id.0,
            lane: id.1,
            length: Some(Tick(960)),
            looped: true,
            locked: true,
        },
    )
    .unwrap();
    let l = d.lane(id.0, id.1).unwrap();
    assert_eq!(
        (l.length, l.looped, l.length_locked),
        (Some(Tick(960)), true, true)
    );
    assert!(
        run(
            &d,
            LaneCmd::SetPlayQuantize {
                seq: id.0,
                lane: id.1,
                quantize: Some(QuantizeSpec {
                    grid: Tick(0),
                    strength: 100
                })
            }
        )
        .is_err()
    );
    assert!(
        run(
            &d,
            LaneCmd::SetPlayQuantize {
                seq: id.0,
                lane: id.1,
                quantize: Some(QuantizeSpec {
                    grid: Tick(960),
                    strength: 101
                })
            }
        )
        .is_err()
    );
    let d = run(
        &d,
        LaneCmd::SetPlayShift {
            seq: id.0,
            lane: id.1,
            shift: Tick(-30),
        },
    )
    .unwrap();
    assert_eq!(d.lane(id.0, id.1).unwrap().play_shift, Tick(-30));
}

#[test]
fn add_take_becomes_active_and_can_switch_back() {
    let d = run(
        &base(),
        LaneCmd::AddTake {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "Take 2".into(),
        },
    )
    .unwrap();
    let l = d.lane(SeqId(0), LaneId(0)).unwrap();
    assert_eq!(l.takes.len(), 2);
    assert_eq!(l.active_take, TakeId(1));
    let d = run(
        &d,
        LaneCmd::SetActiveTake {
            seq: SeqId(0),
            lane: LaneId(0),
            take: TakeId(0),
        },
    )
    .unwrap();
    assert_eq!(d.lane(SeqId(0), LaneId(0)).unwrap().active_take, TakeId(0));
    assert_eq!(
        run(
            &d,
            LaneCmd::SetActiveTake {
                seq: SeqId(0),
                lane: LaneId(0),
                take: TakeId(5)
            }
        ),
        Err(ModelError::UnknownId(AnyId::Take(TakeId(5))))
    );
}
```


- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_lane
```

Expected: FAIL to compile — `error[E0432]: unresolved import `core_model::command::LaneCmd``.

- [ ] **Step 3: Implement**

Create `crates/core-model/src/command/lane.rs`:

```rust
//! Lane and take commands.

use super::Report;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::ids::{AnyId, DestId, LaneId, OutputPathId, SeqId, TakeId};
use crate::lane::{Lane, QuantizeSpec, Take};
use crate::rules::check_positive;
use crate::time::Tick;

/// Lane-level edits.
#[derive(Debug, Clone, PartialEq)]
pub enum LaneCmd {
    /// Add a lane (with one empty take) at `index`, or at the end.
    Add {
        /// Sequence.
        seq: SeqId,
        /// Lane name.
        name: String,
        /// Position; clamped to the lane count.
        index: Option<usize>,
    },
    /// Remove a lane and all its takes and events.
    Remove {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
    },
    /// Move a lane to position `to` (clamped).
    Reorder {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// New index.
        to: usize,
    },
    /// Rename a lane.
    Rename {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// New name.
        name: String,
    },
    /// Set the lane's MIDI destination.
    SetDest {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Destination (must exist).
        dest: Option<DestId>,
    },
    /// Set the lane's audio output path.
    SetOutput {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Output (must exist).
        output: Option<OutputPathId>,
    },
    /// Set mute/solo/arm; `None` leaves a flag unchanged.
    SetFlags {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Mute.
        mute: Option<bool>,
        /// Solo.
        solo: Option<bool>,
        /// Record arm.
        arm: Option<bool>,
    },
    /// Set the lane's own length, loop flag and length lock.
    SetLength {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Own length (> 0) or none.
        length: Option<Tick>,
        /// Loop `[0, length)`.
        looped: bool,
        /// Lock the length against recording.
        locked: bool,
    },
    /// Set non-destructive play quantize.
    SetPlayQuantize {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Quantize (grid > 0, strength <= 100) or none.
        quantize: Option<QuantizeSpec>,
    },
    /// Set non-destructive play shift.
    SetPlayShift {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Shift in ticks.
        shift: Tick,
    },
    /// Add an empty take and make it active.
    AddTake {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Take name.
        name: String,
    },
    /// Choose which take plays.
    SetActiveTake {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Take (must exist in the lane).
        take: TakeId,
    },
}

impl LaneCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            LaneCmd::Add { .. } => "Add Lane",
            LaneCmd::Remove { .. } => "Remove Lane",
            LaneCmd::Reorder { .. } => "Reorder Lane",
            LaneCmd::Rename { .. } => "Rename Lane",
            LaneCmd::SetDest { .. } => "Set Lane Destination",
            LaneCmd::SetOutput { .. } => "Set Lane Output",
            LaneCmd::SetFlags { .. } => "Set Lane Mute/Solo/Arm",
            LaneCmd::SetLength { .. } => "Set Lane Length",
            LaneCmd::SetPlayQuantize { .. } => "Set Play Quantize",
            LaneCmd::SetPlayShift { .. } => "Set Play Shift",
            LaneCmd::AddTake { .. } => "Add Take",
            LaneCmd::SetActiveTake { .. } => "Set Active Take",
        }
    }
}

pub(crate) fn apply(d: &mut Document, cmd: &LaneCmd, r: &mut Report) -> Result<()> {
    match cmd {
        LaneCmd::Add { seq, name, index } => {
            d.sequence(*seq)?;
            let c = d.counters_mut();
            let (lane, take) = (c.alloc_lane(), c.alloc_take());
            let s = d.seq_mut(*seq)?;
            let at = index.unwrap_or(s.lanes.len()).min(s.lanes.len());
            s.lanes.insert(at, Lane::new(lane, take, name.clone()));
            r.created.extend([AnyId::Lane(lane), AnyId::Take(take)]);
        }
        LaneCmd::Remove { seq, lane } => {
            let l = d.remove_lane(*seq, *lane)?;
            for t in &l.takes {
                r.removed
                    .extend(t.events.keys().map(|(_, id)| AnyId::Event(*id)));
            }
            r.removed.push(AnyId::Lane(*lane));
        }
        LaneCmd::Reorder { seq, lane, to } => {
            let s = d.seq_mut(*seq)?;
            let i = s
                .lane_index(*lane)
                .ok_or(ModelError::UnknownId(AnyId::Lane(*lane)))?;
            let l = s.lanes.remove(i);
            let at = (*to).min(s.lanes.len());
            s.lanes.insert(at, l);
        }
        LaneCmd::Rename { seq, lane, name } => d.lane_mut(*seq, *lane)?.name = name.clone(),
        LaneCmd::SetDest { seq, lane, dest } => {
            if let Some(id) = dest {
                if !d.instruments().contains_key(id) {
                    return Err(ModelError::UnknownId(AnyId::Dest(*id)));
                }
            }
            d.lane_mut(*seq, *lane)?.midi_dest = *dest;
        }
        LaneCmd::SetOutput { seq, lane, output } => {
            if let Some(id) = output {
                if !d.outputs().contains_key(id) {
                    return Err(ModelError::UnknownId(AnyId::Output(*id)));
                }
            }
            d.lane_mut(*seq, *lane)?.audio_out = *output;
        }
        LaneCmd::SetFlags {
            seq,
            lane,
            mute,
            solo,
            arm,
        } => {
            let l = d.lane_mut(*seq, *lane)?;
            if let Some(v) = mute {
                l.mute = *v;
            }
            if let Some(v) = solo {
                l.solo = *v;
            }
            if let Some(v) = arm {
                l.arm = *v;
            }
        }
        LaneCmd::SetLength {
            seq,
            lane,
            length,
            looped,
            locked,
        } => {
            if let Some(t) = length {
                check_positive("lane.length", *t)?;
            }
            let l = d.lane_mut(*seq, *lane)?;
            l.length = *length;
            l.looped = *looped;
            l.length_locked = *locked;
        }
        LaneCmd::SetPlayQuantize {
            seq,
            lane,
            quantize,
        } => {
            if let Some(q) = quantize {
                check_positive("play_quantize.grid", q.grid)?;
                if q.strength > 100 {
                    return Err(ModelError::range("play_quantize.strength", q.strength));
                }
            }
            d.lane_mut(*seq, *lane)?.play_quantize = *quantize;
        }
        LaneCmd::SetPlayShift { seq, lane, shift } => {
            d.lane_mut(*seq, *lane)?.play_shift = *shift;
        }
        LaneCmd::AddTake { seq, lane, name } => {
            d.lane(*seq, *lane)?;
            let take = d.counters_mut().alloc_take();
            let l = d.lane_mut(*seq, *lane)?;
            l.takes.push_back(Take::new(take, name.clone()));
            l.active_take = take;
            r.created.push(AnyId::Take(take));
        }
        LaneCmd::SetActiveTake { seq, lane, take } => {
            let l = d.lane_mut(*seq, *lane)?;
            if l.take(*take).is_none() {
                return Err(ModelError::UnknownId(AnyId::Take(*take)));
            }
            l.active_take = *take;
        }
    }
    Ok(())
}
```

Replace the whole of `crates/core-model/src/command/mod.rs` with:

```rust
//! Commands: every edit is a `Command` applied by the pure [`apply`].

pub mod lane;
pub mod sequence;

pub use lane::LaneCmd;
pub use sequence::SequenceCmd;

use crate::document::Document;
use crate::error::Result;
use crate::ids::AnyId;
use crate::selection::{Selection, ViewFilter};

/// What a command did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// IDs created, in creation order.
    pub created: Vec<AnyId>,
    /// IDs removed.
    pub removed: Vec<AnyId>,
    /// Non-fatal notes for the user.
    pub warnings: Vec<String>,
}

/// The result of applying a command.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// The new document.
    pub doc: Document,
    /// What changed.
    pub report: Report,
    /// The new selection, for commands that change it.
    pub selection: Option<Selection>,
}

/// The current selection and view filter, for commands that act on them.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The document's selection.
    pub selection: &'a Selection,
    /// The view filter of the view issuing the command.
    pub filter: &'a ViewFilter,
}

/// Every edit the model supports.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Sequence-level edits.
    Sequence(SequenceCmd),
    /// Lane and take edits.
    Lane(LaneCmd),
}

impl Command {
    /// The stable name, used for key bindings and undo labels.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Sequence(c) => c.name(),
            Command::Lane(c) => c.name(),
        }
    }
}

/// Applies a command with an empty selection and no view filter.
pub fn apply(doc: &Document, cmd: &Command) -> Result<Applied> {
    let selection = Selection::default();
    let filter = ViewFilter::default();
    apply_in(
        doc,
        Context {
            selection: &selection,
            filter: &filter,
        },
        cmd,
    )
}

/// Applies a command in the context of a selection and view filter. Pure:
/// `doc` is never changed; on error nothing is returned but the error.
pub fn apply_in(doc: &Document, ctx: Context<'_>, cmd: &Command) -> Result<Applied> {
    let mut d = doc.clone();
    let mut report = Report::default();
    let _ = ctx;
    let selection = match cmd {
        Command::Sequence(c) => {
            sequence::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Lane(c) => {
            lane::apply(&mut d, c, &mut report)?;
            None
        }
    };
    Ok(Applied {
        doc: d,
        report,
        selection,
    })
}
```


- [ ] **Step 4: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_lane
```

Expected: PASS — `cmd_lane`: `test result: ok. 6 passed`.

- [ ] **Step 5: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/command/lane.rs crates/core-model/src/command/mod.rs crates/core-model/tests/cmd_lane.rs
git commit -m "feat(core-model): lane and take commands"
```

---

### Task 8: Event commands (insert, delete, move, set fields, overrides, nudge)

**Goal:** All event-level commands, validated through the rules, with cycle checks for references and the shared edit helpers later tasks reuse (including drum-row auto-creation).

**Files:**
- Create: `crates/core-model/src/command/event.rs`
- Modify: `crates/core-model/src/command/mod.rs`
- Create (test): `crates/core-model/tests/cmd_event.rs`

**Interfaces:**
- Consumes: Task 6 plumbing; `Document::{insert_event, remove_event, replace_event, event}`; `rules::{check_event, check_event_fields}`.
- Produces:
  - `Command::Event(EventCmd)`; `EventCmd::{Insert { seq, lane, events: Vec<EventSpec> }, Delete { events }, Move { events, delta, to_lane: Option<(SeqId, LaneId)> }, SetPitch { events, pitch }, SetVelocity { events, velocity }, SetDuration { events, duration }, SetValue { events, value }, SetDest { events, dest }, SetGroupChannel { events, group_channel }, SetMute { events, mute }, Nudge { events, ticks, semitones }}` (`events: Vec<EventId>`).
  - Crate-internal (in `command::event`): `note_name(u8) -> String` (`60` → `C4`), `shift_pitch(u8, i32) -> u8` (clamps 0..=127), `ensure_drum_row(&mut Document, EventId, &mut Report) -> Result<()>`, `edit_events(&mut Document, &[EventId], &mut Report, impl FnMut(&mut Event) -> Result<()>) -> Result<()>`, `insert_specs(&mut Document, SeqId, LaneId, &[EventSpec], &mut Report) -> Result<Vec<EventId>>`.

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/cmd_event.rs`:

```rust
use core_model::ModelError;
use core_model::command::{Command, EventCmd, LaneCmd, SequenceCmd, apply};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{AnyId, EventId, LaneId, SeqId, TakeId};
use core_model::rules::validate;
use core_model::time::{PPQ, Tick};

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn ev(c: EventCmd) -> Command {
    Command::Event(c)
}

/// Two new sequences: seq0 (lane0) and seq1 (lane1).
fn two_seqs() -> Document {
    let d = run(
        &Document::new(),
        Command::Sequence(SequenceCmd::New { name: "A".into() }),
    )
    .unwrap();
    run(&d, Command::Sequence(SequenceCmd::New { name: "B".into() })).unwrap()
}

fn insert(
    d: &Document,
    seq: u64,
    lane: u64,
    specs: Vec<EventSpec>,
) -> core_model::Result<Document> {
    run(
        d,
        ev(EventCmd::Insert {
            seq: SeqId(seq),
            lane: LaneId(lane),
            events: specs,
        }),
    )
}

fn note(t: i64, pitch: u8) -> EventSpec {
    EventSpec::new(Tick(t), EventBody::note(pitch, 100, Tick(PPQ)))
}

fn body(d: &Document, id: u64) -> EventBody {
    d.event(EventId(id)).unwrap().0.body.clone()
}

#[test]
fn insert_allocates_ids_and_reports_them() {
    let a = apply(
        &two_seqs(),
        &ev(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![note(0, 60), note(PPQ, 62)],
        }),
    )
    .unwrap();
    assert_eq!(
        a.report.created,
        vec![AnyId::Event(EventId(0)), AnyId::Event(EventId(1))]
    );
    let (_, loc) = a.doc.event(EventId(1)).unwrap();
    assert_eq!(
        (loc.seq, loc.lane, loc.take, loc.tick),
        (SeqId(0), LaneId(0), TakeId(0), Tick(PPQ))
    );
    assert!(validate(&a.doc).is_empty());
}

#[test]
fn insert_goes_to_the_active_take() {
    let d = run(
        &two_seqs(),
        Command::Lane(LaneCmd::AddTake {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "T2".into(),
        }),
    )
    .unwrap();
    let d = insert(&d, 0, 0, vec![note(0, 60)]).unwrap();
    assert_eq!(d.event(EventId(0)).unwrap().1.take, TakeId(2));
}

#[test]
fn failed_insert_changes_nothing() {
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 60)]).unwrap();
    let err = insert(&d, 0, 0, vec![note(0, 61), note(0, 200)]).unwrap_err();
    assert!(matches!(err, ModelError::OutOfRange { field: "pitch", .. }));
    assert_eq!(d.all_events().count(), 1);
}

#[test]
fn self_and_indirect_reference_cycles_are_refused() {
    let d = two_seqs();
    let self_ref = EventSpec::new(Tick(0), EventBody::reference(SeqId(0)));
    assert!(matches!(
        insert(&d, 0, 0, vec![self_ref]),
        Err(ModelError::Cycle { .. })
    ));
    let d = insert(
        &d,
        0,
        0,
        vec![EventSpec::new(Tick(0), EventBody::reference(SeqId(1)))],
    )
    .unwrap();
    let back = EventSpec::new(Tick(0), EventBody::reference(SeqId(0)));
    assert_eq!(
        insert(&d, 1, 1, vec![back]),
        Err(ModelError::Cycle {
            path: vec![SeqId(1), SeqId(0), SeqId(1)]
        })
    );
}

#[test]
fn moving_a_reference_into_its_target_is_a_cycle() {
    let d = insert(
        &two_seqs(),
        0,
        0,
        vec![EventSpec::new(Tick(0), EventBody::reference(SeqId(1)))],
    )
    .unwrap();
    let err = run(
        &d,
        ev(EventCmd::Move {
            events: vec![EventId(0)],
            delta: Tick(0),
            to_lane: Some((SeqId(1), LaneId(1))),
        }),
    )
    .unwrap_err();
    assert!(matches!(err, ModelError::Cycle { .. }));
}

#[test]
fn move_in_time_and_across_lanes() {
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 60)]).unwrap();
    let d = run(
        &d,
        ev(EventCmd::Move {
            events: vec![EventId(0)],
            delta: Tick(PPQ),
            to_lane: None,
        }),
    )
    .unwrap();
    assert_eq!(d.event(EventId(0)).unwrap().0.tick, Tick(PPQ));
    let d = run(
        &d,
        ev(EventCmd::Move {
            events: vec![EventId(0), EventId(0)],
            delta: Tick(-PPQ),
            to_lane: Some((SeqId(1), LaneId(1))),
        }),
    )
    .unwrap();
    let (e, loc) = d.event(EventId(0)).unwrap();
    assert_eq!((e.tick, loc.seq, loc.lane), (Tick(0), SeqId(1), LaneId(1)));
    assert!(validate(&d).is_empty());
}

#[test]
fn delete_removes_and_reports() {
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 60)]).unwrap();
    let a = apply(
        &d,
        &ev(EventCmd::Delete {
            events: vec![EventId(0)],
        }),
    )
    .unwrap();
    assert_eq!(a.report.removed, vec![AnyId::Event(EventId(0))]);
    assert!(a.doc.event(EventId(0)).is_err());
    assert!(matches!(
        run(
            &a.doc,
            ev(EventCmd::Delete {
                events: vec![EventId(0)]
            })
        ),
        Err(ModelError::UnknownId(_))
    ));
}

#[test]
fn setters_touch_only_applicable_kinds() {
    let cc = EventSpec::new(Tick(0), EventBody::Cc { index: 1, value: 5 });
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 60), cc]).unwrap();
    let both = vec![EventId(0), EventId(1)];
    let d = run(
        &d,
        ev(EventCmd::SetPitch {
            events: both.clone(),
            pitch: 64,
        }),
    )
    .unwrap();
    let d = run(
        &d,
        ev(EventCmd::SetVelocity {
            events: both.clone(),
            velocity: 9,
        }),
    )
    .unwrap();
    let d = run(
        &d,
        ev(EventCmd::SetDuration {
            events: both.clone(),
            duration: Tick(10),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        ev(EventCmd::SetValue {
            events: both.clone(),
            value: 77,
        }),
    )
    .unwrap();
    assert_eq!(body(&d, 0), EventBody::note(64, 9, Tick(10)));
    assert_eq!(
        body(&d, 1),
        EventBody::Cc {
            index: 1,
            value: 77
        }
    );
    assert!(
        run(
            &d,
            ev(EventCmd::SetDuration {
                events: both.clone(),
                duration: Tick(-1)
            })
        )
        .is_err()
    );
}

#[test]
fn overrides_and_mute() {
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 60)]).unwrap();
    let one = vec![EventId(0)];
    let d = run(
        &d,
        ev(EventCmd::SetGroupChannel {
            events: one.clone(),
            group_channel: Some((2, 9)),
        }),
    )
    .unwrap();
    assert_eq!(d.event(EventId(0)).unwrap().0.group_channel, Some((2, 9)));
    assert!(
        run(
            &d,
            ev(EventCmd::SetGroupChannel {
                events: one.clone(),
                group_channel: Some((16, 0))
            })
        )
        .is_err()
    );
    assert!(matches!(
        run(
            &d,
            ev(EventCmd::SetDest {
                events: one.clone(),
                dest: Some(core_model::ids::DestId(3))
            })
        ),
        Err(ModelError::UnknownId(_))
    ));
    let d = run(
        &d,
        ev(EventCmd::SetMute {
            events: one,
            mute: true,
        }),
    )
    .unwrap();
    assert!(d.event(EventId(0)).unwrap().0.mute);
}

#[test]
fn nudge_clamps_pitch_at_0_and_127() {
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 120), note(0, 3)]).unwrap();
    let d = run(
        &d,
        ev(EventCmd::Nudge {
            events: vec![EventId(0)],
            ticks: Tick(5),
            semitones: 12,
        }),
    )
    .unwrap();
    let d = run(
        &d,
        ev(EventCmd::Nudge {
            events: vec![EventId(1)],
            ticks: Tick(0),
            semitones: -12,
        }),
    )
    .unwrap();
    assert_eq!(body(&d, 0), EventBody::note(127, 100, Tick(PPQ)));
    assert_eq!(d.event(EventId(0)).unwrap().0.tick, Tick(5));
    assert_eq!(body(&d, 1), EventBody::note(0, 100, Tick(PPQ)));
}
```


- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_event
```

Expected: FAIL to compile — `error[E0432]: unresolved import `core_model::command::EventCmd``.

- [ ] **Step 3: Implement**

Create `crates/core-model/src/command/event.rs`:

```rust
//! Event commands, plus the shared edit helper and drum-row auto-creation.

use super::Report;
use crate::document::Document;
use crate::error::Result;
use crate::event::{Event, EventBody, EventSpec};
use crate::ids::{AnyId, DestId, EventId, LaneId, SeqId};
use crate::lane::DrumRow;
use crate::rules::{check_event, check_event_fields};
use crate::time::Tick;
use std::collections::BTreeSet;

/// Event-level edits. Commands that take several events apply to each;
/// fields that do not apply to an event's kind leave it unchanged.
#[derive(Debug, Clone, PartialEq)]
pub enum EventCmd {
    /// Insert events into a lane's active take.
    Insert {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// The events.
        events: Vec<EventSpec>,
    },
    /// Delete events.
    Delete {
        /// The events.
        events: Vec<EventId>,
    },
    /// Move in time and optionally to another lane's active take.
    Move {
        /// The events.
        events: Vec<EventId>,
        /// Time offset.
        delta: Tick,
        /// Destination lane.
        to_lane: Option<(SeqId, LaneId)>,
    },
    /// Set note / poly-pressure pitch.
    SetPitch {
        /// The events.
        events: Vec<EventId>,
        /// Pitch 0–127.
        pitch: u8,
    },
    /// Set note velocity.
    SetVelocity {
        /// The events.
        events: Vec<EventId>,
        /// 16-bit velocity.
        velocity: u16,
    },
    /// Set note duration.
    SetDuration {
        /// The events.
        events: Vec<EventId>,
        /// Duration (>= 0).
        duration: Tick,
    },
    /// Set the 32-bit value of CC, pitch bend and pressure events.
    SetValue {
        /// The events.
        events: Vec<EventId>,
        /// Value.
        value: u32,
    },
    /// Set or clear the destination override.
    SetDest {
        /// The events.
        events: Vec<EventId>,
        /// Destination (must exist).
        dest: Option<DestId>,
    },
    /// Set or clear the group/channel override.
    SetGroupChannel {
        /// The events.
        events: Vec<EventId>,
        /// `(group, channel)`, each 0–15.
        group_channel: Option<(u8, u8)>,
    },
    /// Mute or unmute.
    SetMute {
        /// The events.
        events: Vec<EventId>,
        /// Mute flag.
        mute: bool,
    },
    /// Nudge in time and pitch (pitch clamps at 0 and 127).
    Nudge {
        /// The events.
        events: Vec<EventId>,
        /// Time offset.
        ticks: Tick,
        /// Pitch offset.
        semitones: i8,
    },
}

impl EventCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            EventCmd::Insert { .. } => "Insert Events",
            EventCmd::Delete { .. } => "Delete Events",
            EventCmd::Move { .. } => "Move Events",
            EventCmd::SetPitch { .. } => "Set Pitch",
            EventCmd::SetVelocity { .. } => "Set Velocity",
            EventCmd::SetDuration { .. } => "Set Duration",
            EventCmd::SetValue { .. } => "Set Value",
            EventCmd::SetDest { .. } => "Set Event Destination",
            EventCmd::SetGroupChannel { .. } => "Set Group/Channel",
            EventCmd::SetMute { .. } => "Mute Events",
            EventCmd::Nudge { .. } => "Nudge Events",
        }
    }
}

/// Note name such as `C2` for 36 (C-1 is note 0, middle C 60 is `C4`).
pub(crate) fn note_name(n: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let name = NAMES.get(usize::from(n % 12)).copied().unwrap_or("?");
    format!("{name}{}", i32::from(n / 12) - 1)
}

/// Adds a pitch-shifted copy of `p`, clamped to 0..=127.
pub(crate) fn shift_pitch(p: u8, by: i32) -> u8 {
    u8::try_from((i32::from(p) + by).clamp(0, 127)).unwrap_or(127)
}

/// If `id` is a note on a drum lane with no matching row, adds a row.
pub(crate) fn ensure_drum_row(d: &mut Document, id: EventId, r: &mut Report) -> Result<()> {
    let (ev, loc) = d.event(id)?;
    let EventBody::Note { pitch, .. } = ev.body else {
        return Ok(());
    };
    let dest = ev.dest;
    let lane = d.lane(loc.seq, loc.lane)?;
    let Some(drum) = &lane.drum else {
        return Ok(());
    };
    if drum.rows.iter().any(|row| row.matches(pitch, dest)) {
        return Ok(());
    }
    let row_id = d.counters_mut().alloc_drum_row();
    if let Some(drum) = d.lane_mut(loc.seq, loc.lane)?.drum.as_mut() {
        drum.rows.push_back(DrumRow {
            id: row_id,
            name: note_name(pitch),
            note: pitch,
            dest,
            show_velocity: false,
        });
    }
    r.created.push(AnyId::DrumRow(row_id));
    Ok(())
}

/// Applies `f` to each event (deduplicated), re-validates it, stores it and
/// auto-creates drum rows.
pub(crate) fn edit_events(
    d: &mut Document,
    ids: &[EventId],
    r: &mut Report,
    mut f: impl FnMut(&mut Event) -> Result<()>,
) -> Result<()> {
    let unique: BTreeSet<EventId> = ids.iter().copied().collect();
    for id in unique {
        let (ev, _) = d.event(id)?;
        let mut ev = ev.clone();
        f(&mut ev)?;
        check_event_fields(d, &ev.spec())?;
        d.replace_event(ev)?;
        ensure_drum_row(d, id, r)?;
    }
    Ok(())
}

/// Inserts specs into the active take of a lane; returns the new IDs.
pub(crate) fn insert_specs(
    d: &mut Document,
    seq: SeqId,
    lane: LaneId,
    specs: &[EventSpec],
    r: &mut Report,
) -> Result<Vec<EventId>> {
    d.lane(seq, lane)?;
    let mut ids = Vec::with_capacity(specs.len());
    for spec in specs {
        check_event(d, seq, spec)?;
        let id = d.counters_mut().alloc_event();
        d.insert_event(seq, lane, None, spec.clone().with_id(id))?;
        r.created.push(AnyId::Event(id));
        ensure_drum_row(d, id, r)?;
        ids.push(id);
    }
    Ok(ids)
}

pub(crate) fn apply(d: &mut Document, cmd: &EventCmd, r: &mut Report) -> Result<()> {
    match cmd {
        EventCmd::Insert { seq, lane, events } => {
            insert_specs(d, *seq, *lane, events, r)?;
        }
        EventCmd::Delete { events } => {
            let unique: BTreeSet<EventId> = events.iter().copied().collect();
            for id in unique {
                d.remove_event(id)?;
                r.removed.push(AnyId::Event(id));
            }
        }
        EventCmd::Move {
            events,
            delta,
            to_lane,
        } => match to_lane {
            None => edit_events(d, events, r, |e| {
                e.tick = e.tick + *delta;
                Ok(())
            })?,
            Some((seq, lane)) => {
                d.lane(*seq, *lane)?;
                let unique: BTreeSet<EventId> = events.iter().copied().collect();
                for id in unique {
                    let (mut ev, _) = d.remove_event(id)?;
                    ev.tick = ev.tick + *delta;
                    check_event(d, *seq, &ev.spec())?;
                    d.insert_event(*seq, *lane, None, ev)?;
                    ensure_drum_row(d, id, r)?;
                }
            }
        },
        EventCmd::SetPitch { events, pitch } => edit_events(d, events, r, |e| {
            match &mut e.body {
                EventBody::Note { pitch: p, .. } | EventBody::PolyPressure { pitch: p, .. } => {
                    *p = *pitch
                }
                _ => {}
            }
            Ok(())
        })?,
        EventCmd::SetVelocity { events, velocity } => edit_events(d, events, r, |e| {
            if let EventBody::Note { velocity: v, .. } = &mut e.body {
                *v = *velocity;
            }
            Ok(())
        })?,
        EventCmd::SetDuration { events, duration } => edit_events(d, events, r, |e| {
            if let EventBody::Note { duration: v, .. } = &mut e.body {
                *v = *duration;
            }
            Ok(())
        })?,
        EventCmd::SetValue { events, value } => edit_events(d, events, r, |e| {
            match &mut e.body {
                EventBody::Cc { value: v, .. }
                | EventBody::PitchBend { value: v }
                | EventBody::ChannelPressure { value: v }
                | EventBody::PolyPressure { value: v, .. } => *v = *value,
                _ => {}
            }
            Ok(())
        })?,
        EventCmd::SetDest { events, dest } => edit_events(d, events, r, |e| {
            e.dest = *dest;
            Ok(())
        })?,
        EventCmd::SetGroupChannel {
            events,
            group_channel,
        } => edit_events(d, events, r, |e| {
            e.group_channel = *group_channel;
            Ok(())
        })?,
        EventCmd::SetMute { events, mute } => edit_events(d, events, r, |e| {
            e.mute = *mute;
            Ok(())
        })?,
        EventCmd::Nudge {
            events,
            ticks,
            semitones,
        } => edit_events(d, events, r, |e| {
            e.tick = e.tick + *ticks;
            match &mut e.body {
                EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } => {
                    *pitch = shift_pitch(*pitch, i32::from(*semitones))
                }
                _ => {}
            }
            Ok(())
        })?,
    }
    Ok(())
}
```

Replace the whole of `crates/core-model/src/command/mod.rs` with:

```rust
//! Commands: every edit is a `Command` applied by the pure [`apply`].

pub mod event;
pub mod lane;
pub mod sequence;

pub use event::EventCmd;
pub use lane::LaneCmd;
pub use sequence::SequenceCmd;

use crate::document::Document;
use crate::error::Result;
use crate::ids::AnyId;
use crate::selection::{Selection, ViewFilter};

/// What a command did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// IDs created, in creation order.
    pub created: Vec<AnyId>,
    /// IDs removed.
    pub removed: Vec<AnyId>,
    /// Non-fatal notes for the user.
    pub warnings: Vec<String>,
}

/// The result of applying a command.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// The new document.
    pub doc: Document,
    /// What changed.
    pub report: Report,
    /// The new selection, for commands that change it.
    pub selection: Option<Selection>,
}

/// The current selection and view filter, for commands that act on them.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The document's selection.
    pub selection: &'a Selection,
    /// The view filter of the view issuing the command.
    pub filter: &'a ViewFilter,
}

/// Every edit the model supports.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Sequence-level edits.
    Sequence(SequenceCmd),
    /// Lane and take edits.
    Lane(LaneCmd),
    /// Event edits.
    Event(EventCmd),
}

impl Command {
    /// The stable name, used for key bindings and undo labels.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Sequence(c) => c.name(),
            Command::Lane(c) => c.name(),
            Command::Event(c) => c.name(),
        }
    }
}

/// Applies a command with an empty selection and no view filter.
pub fn apply(doc: &Document, cmd: &Command) -> Result<Applied> {
    let selection = Selection::default();
    let filter = ViewFilter::default();
    apply_in(
        doc,
        Context {
            selection: &selection,
            filter: &filter,
        },
        cmd,
    )
}

/// Applies a command in the context of a selection and view filter. Pure:
/// `doc` is never changed; on error nothing is returned but the error.
pub fn apply_in(doc: &Document, ctx: Context<'_>, cmd: &Command) -> Result<Applied> {
    let mut d = doc.clone();
    let mut report = Report::default();
    let _ = ctx;
    let selection = match cmd {
        Command::Sequence(c) => {
            sequence::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Lane(c) => {
            lane::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Event(c) => {
            event::apply(&mut d, c, &mut report)?;
            None
        }
    };
    Ok(Applied {
        doc: d,
        report,
        selection,
    })
}
```


- [ ] **Step 4: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_event
```

Expected: PASS — `cmd_event`: `test result: ok. 10 passed`.

- [ ] **Step 5: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/command/event.rs crates/core-model/src/command/mod.rs crates/core-model/tests/cmd_event.rs
git commit -m "feat(core-model): event commands"
```

---

### Task 9: Drum-lane commands and drum-row auto-creation

**Goal:** Mark a lane as a drum lane, add/rename/reorder/remove rows, move a row to another lane and split a row to its own lane; notes inserted or changed from any editor create missing rows.

**Files:**
- Create: `crates/core-model/src/command/drum.rs`
- Modify: `crates/core-model/src/command/mod.rs`
- Create (test): `crates/core-model/tests/cmd_drum.rs`

**Interfaces:**
- Consumes: Task 8 helpers (`note_name`, `ensure_drum_row` already runs on every insert/edit); `Document` crate API.
- Produces:
  - `Command::Drum(DrumCmd)`; `DrumCmd::{MarkDrumLane { seq, lane, drum: bool }, AddRow { seq, lane, name, note, dest }, RenameRow { seq, lane, row, name }, ReorderRow { seq, lane, row, to }, RemoveRow { seq, lane, row }, MoveRowToLane { seq, lane, row, to_lane }, SplitRowToLane { seq, lane, row }}`.
  - Crate-internal: `command::drum::row_events(&Lane, &DrumRow, active_only: bool) -> Vec<EventId>` (used by the drum view in Task 10).

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/cmd_drum.rs`:

```rust
use core_model::ModelError;
use core_model::command::{Command, DrumCmd, EventCmd, LaneCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Destination, Document, DocumentRepr, Settings};
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{AnyId, Counters, DestId, DrumRowId, EventId, LaneId, SeqId};
use core_model::rules::validate;
use core_model::time::{PPQ, Tick};

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn drum(c: DrumCmd) -> Command {
    Command::Drum(c)
}

fn note(t: i64, pitch: u8) -> EventSpec {
    EventSpec::new(Tick(t), EventBody::note(pitch, 100, Tick(PPQ / 4)))
}

/// dest0 and dest1; seq0 with lane0 (dest0, drum lane) and lane1 (dest1).
fn kit() -> Document {
    let dest = |i: u64| Destination {
        id: DestId(i),
        name: format!("D{i}"),
        kind: DestKind::PluginInstrument {
            plugin_ref: "p".into(),
        },
        default_patch: None,
    };
    let d = Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![dest(0), dest(1)],
        outputs: vec![],
        media: vec![],
        sequences: vec![],
        counters: Counters {
            dest: 2,
            ..Counters::default()
        },
    })
    .unwrap();
    let d = run(
        &d,
        Command::Sequence(SequenceCmd::New {
            name: "Beat".into(),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "B".into(),
            index: None,
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(DestId(0)),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(1),
            dest: Some(DestId(1)),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![note(0, 36), note(PPQ, 38), note(PPQ * 2, 36)],
        }),
    )
    .unwrap();
    run(
        &d,
        drum(DrumCmd::MarkDrumLane {
            seq: SeqId(0),
            lane: LaneId(0),
            drum: true,
        }),
    )
    .unwrap()
}

fn rows(d: &Document, lane: u64) -> Vec<(u8, Option<DestId>, String)> {
    d.lane(SeqId(0), LaneId(lane))
        .unwrap()
        .drum
        .as_ref()
        .map(|l| {
            l.rows
                .iter()
                .map(|r| (r.note, r.dest, r.name.clone()))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn marking_creates_rows_for_existing_notes() {
    assert_eq!(
        rows(&kit(), 0),
        vec![(36, None, "C2".into()), (38, None, "D2".into())]
    );
    assert!(validate(&kit()).is_empty());
}

#[test]
fn inserting_a_new_note_auto_creates_a_row() {
    let a = apply(
        &kit(),
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![note(0, 42), note(PPQ, 42)],
        }),
    )
    .unwrap();
    assert_eq!(rows(&a.doc, 0).len(), 3);
    assert_eq!(rows(&a.doc, 0)[2].0, 42);
    // Exactly one row created for two notes.
    assert_eq!(
        a.report
            .created
            .iter()
            .filter(|i| matches!(i, AnyId::DrumRow(_)))
            .count(),
        1
    );
}

#[test]
fn changing_a_note_from_another_editor_auto_creates_a_row() {
    let d = run(
        &kit(),
        Command::Event(EventCmd::SetPitch {
            events: vec![EventId(1)],
            pitch: 46,
        }),
    )
    .unwrap();
    assert!(rows(&d, 0).iter().any(|r| r.0 == 46));
    let d = run(
        &d,
        Command::Event(EventCmd::SetDest {
            events: vec![EventId(1)],
            dest: Some(DestId(1)),
        }),
    )
    .unwrap();
    assert!(
        rows(&d, 0)
            .iter()
            .any(|r| r.0 == 46 && r.1 == Some(DestId(1)))
    );
}

#[test]
fn add_rename_reorder_remove_rows() {
    let d = run(
        &kit(),
        drum(DrumCmd::AddRow {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "Hat".into(),
            note: 42,
            dest: None,
        }),
    )
    .unwrap();
    assert!(matches!(
        run(
            &d,
            drum(DrumCmd::AddRow {
                seq: SeqId(0),
                lane: LaneId(0),
                name: "Dup".into(),
                note: 42,
                dest: None
            })
        ),
        Err(ModelError::Invalid { .. })
    ));
    let d = run(
        &d,
        drum(DrumCmd::RenameRow {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(0),
            name: "Kick".into(),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        drum(DrumCmd::ReorderRow {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(2),
            to: 0,
        }),
    )
    .unwrap();
    assert_eq!(
        rows(&d, 0).iter().map(|r| r.2.as_str()).collect::<Vec<_>>(),
        vec!["Hat", "Kick", "D2"]
    );
    let a = apply(
        &d,
        &drum(DrumCmd::RemoveRow {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(0),
        }),
    )
    .unwrap();
    assert_eq!(
        a.report.removed,
        vec![
            AnyId::Event(EventId(0)),
            AnyId::Event(EventId(2)),
            AnyId::DrumRow(DrumRowId(0))
        ]
    );
    assert!(
        run(
            &d,
            drum(DrumCmd::AddRow {
                seq: SeqId(0),
                lane: LaneId(1),
                name: "x".into(),
                note: 1,
                dest: None
            })
        )
        .is_err()
    );
}

#[test]
fn move_row_keeps_sound_by_pinning_destination() {
    let d = run(
        &kit(),
        drum(DrumCmd::MoveRowToLane {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(0),
            to_lane: LaneId(1),
        }),
    )
    .unwrap();
    assert_eq!(rows(&d, 0), vec![(38, None, "D2".into())]);
    assert_eq!(rows(&d, 1), vec![(36, Some(DestId(0)), "C2".into())]);
    for id in [0, 2] {
        let (e, loc) = d.event(EventId(id)).unwrap();
        assert_eq!((loc.lane, e.dest), (LaneId(1), Some(DestId(0))));
    }
    assert!(validate(&d).is_empty());
}

#[test]
fn split_row_creates_lane_after_source() {
    let a = apply(
        &kit(),
        &drum(DrumCmd::SplitRowToLane {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(1),
        }),
    )
    .unwrap();
    let s = a.doc.sequence(SeqId(0)).unwrap();
    assert_eq!(
        s.lanes.iter().map(|l| l.id).collect::<Vec<_>>(),
        vec![LaneId(0), LaneId(2), LaneId(1)]
    );
    let new_lane = &s.lanes[1];
    assert_eq!(
        (new_lane.name.as_str(), new_lane.midi_dest),
        ("D2", Some(DestId(0)))
    );
    assert_eq!(rows(&a.doc, 2), vec![(38, None, "D2".into())]);
    assert_eq!(a.doc.event(EventId(1)).unwrap().1.lane, LaneId(2));
    assert!(validate(&a.doc).is_empty());
}
```


- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_drum
```

Expected: FAIL to compile — `error[E0432]: unresolved import `core_model::command::DrumCmd``.

- [ ] **Step 3: Implement**

Create `crates/core-model/src/command/drum.rs`:

```rust
//! Drum-lane commands. Rows are a view over the lane's notes grouped by
//! `(pitch, dest override)`; membership is derived, never stored.

use super::Report;
use super::event::note_name;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::event::EventBody;
use crate::ids::{AnyId, DestId, DrumRowId, EventId, LaneId, SeqId};
use crate::lane::{DrumLayout, DrumRow, Lane};
use std::collections::BTreeSet;

/// Drum-lane edits.
#[derive(Debug, Clone, PartialEq)]
pub enum DrumCmd {
    /// Make a lane a drum lane (rows are created for its existing notes) or
    /// a normal lane again.
    MarkDrumLane {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// True to make it a drum lane.
        drum: bool,
    },
    /// Add a row; `(note, dest)` must be new in the lane.
    AddRow {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Row name.
        name: String,
        /// Note 0–127.
        note: u8,
        /// Destination override for the row's notes.
        dest: Option<DestId>,
    },
    /// Rename a row.
    RenameRow {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Row.
        row: DrumRowId,
        /// New name.
        name: String,
    },
    /// Move a row to display position `to` (clamped).
    ReorderRow {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Row.
        row: DrumRowId,
        /// New index.
        to: usize,
    },
    /// Remove a row and delete its notes from every take.
    RemoveRow {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Row.
        row: DrumRowId,
    },
    /// Move a row and its active-take notes to another lane of the same
    /// sequence, keeping their sound.
    MoveRowToLane {
        /// Sequence.
        seq: SeqId,
        /// Source lane.
        lane: LaneId,
        /// Row.
        row: DrumRowId,
        /// Target lane.
        to_lane: LaneId,
    },
    /// Move a row to a new lane of its own, placed after the source lane.
    SplitRowToLane {
        /// Sequence.
        seq: SeqId,
        /// Source lane.
        lane: LaneId,
        /// Row.
        row: DrumRowId,
    },
}

impl DrumCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            DrumCmd::MarkDrumLane { .. } => "Mark as Drum Lane",
            DrumCmd::AddRow { .. } => "Add Drum Row",
            DrumCmd::RenameRow { .. } => "Rename Drum Row",
            DrumCmd::ReorderRow { .. } => "Reorder Drum Row",
            DrumCmd::RemoveRow { .. } => "Remove Drum Row",
            DrumCmd::MoveRowToLane { .. } => "Move Drum Row to Lane",
            DrumCmd::SplitRowToLane { .. } => "Split Drum Row to Lane",
        }
    }
}

fn layout_mut(d: &mut Document, seq: SeqId, lane: LaneId) -> Result<&mut DrumLayout> {
    d.lane_mut(seq, lane)?
        .drum
        .as_mut()
        .ok_or_else(|| ModelError::invalid(format!("{lane} is not a drum lane")))
}

fn find_row(d: &Document, seq: SeqId, lane: LaneId, row: DrumRowId) -> Result<DrumRow> {
    d.lane(seq, lane)?
        .drum
        .as_ref()
        .and_then(|l| l.rows.iter().find(|r| r.id == row))
        .cloned()
        .ok_or(ModelError::UnknownId(AnyId::DrumRow(row)))
}

/// Notes belonging to `row` in one lane; `active_only` limits to the
/// active take.
pub(crate) fn row_events(lane: &Lane, row: &DrumRow, active_only: bool) -> Vec<EventId> {
    lane.takes
        .iter()
        .filter(|t| !active_only || t.id == lane.active_take)
        .flat_map(|t| t.events.values())
        .filter(|e| matches!(e.body, EventBody::Note { pitch, .. } if row.matches(pitch, e.dest)))
        .map(|e| e.id)
        .collect()
}

fn move_row(
    d: &mut Document,
    seq: SeqId,
    lane: LaneId,
    row: DrumRowId,
    to_lane: LaneId,
    r: &mut Report,
) -> Result<()> {
    if lane == to_lane {
        return Ok(());
    }
    let mut moved = find_row(d, seq, lane, row)?;
    let src = d.lane(seq, lane)?.clone();
    let dst = d.lane(seq, to_lane)?.clone();
    let ids = row_events(&src, &moved, true);
    // Keep the sound: pin the source lane's destination when it differs.
    let pin = if moved.dest.is_none() && src.midi_dest != dst.midi_dest {
        src.midi_dest
    } else {
        None
    };
    if pin.is_some() {
        moved.dest = pin;
    }
    for id in ids {
        let (mut ev, _) = d.remove_event(id)?;
        if pin.is_some() {
            ev.dest = pin;
        }
        d.insert_event(seq, to_lane, None, ev)?;
    }
    let src_layout = layout_mut(d, seq, lane)?;
    src_layout.rows.retain(|x| x.id != row);
    let dst_lane = d.lane_mut(seq, to_lane)?;
    let layout = dst_lane.drum.get_or_insert_with(DrumLayout::default);
    if !layout
        .rows
        .iter()
        .any(|x| x.matches(moved.note, moved.dest))
    {
        layout.rows.push_back(moved);
    } else {
        r.warnings
            .push(format!("{row} merged into an existing row of {to_lane}"));
    }
    Ok(())
}

pub(crate) fn apply(d: &mut Document, cmd: &DrumCmd, r: &mut Report) -> Result<()> {
    match cmd {
        DrumCmd::MarkDrumLane { seq, lane, drum } => {
            let l = d.lane(*seq, *lane)?.clone();
            if !*drum {
                d.lane_mut(*seq, *lane)?.drum = None;
                return Ok(());
            }
            if l.drum.is_some() {
                return Ok(());
            }
            let keys: BTreeSet<(u8, Option<DestId>)> = l
                .active()
                .map(|t| {
                    t.events
                        .values()
                        .filter_map(|e| match e.body {
                            EventBody::Note { pitch, .. } => Some((pitch, e.dest)),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut layout = DrumLayout::default();
            for (note, dest) in keys {
                let id = d.counters_mut().alloc_drum_row();
                layout.rows.push_back(DrumRow {
                    id,
                    name: note_name(note),
                    note,
                    dest,
                    show_velocity: false,
                });
                r.created.push(AnyId::DrumRow(id));
            }
            d.lane_mut(*seq, *lane)?.drum = Some(layout);
        }
        DrumCmd::AddRow {
            seq,
            lane,
            name,
            note,
            dest,
        } => {
            if *note > 127 {
                return Err(ModelError::range("drum_row.note", note));
            }
            if let Some(id) = dest {
                if !d.instruments().contains_key(id) {
                    return Err(ModelError::UnknownId(AnyId::Dest(*id)));
                }
            }
            let layout = layout_mut(d, *seq, *lane)?;
            if layout.rows.iter().any(|x| x.matches(*note, *dest)) {
                return Err(ModelError::invalid(
                    "a row for that note and destination exists",
                ));
            }
            let id = d.counters_mut().alloc_drum_row();
            layout_mut(d, *seq, *lane)?.rows.push_back(DrumRow {
                id,
                name: name.clone(),
                note: *note,
                dest: *dest,
                show_velocity: false,
            });
            r.created.push(AnyId::DrumRow(id));
        }
        DrumCmd::RenameRow {
            seq,
            lane,
            row,
            name,
        } => {
            let layout = layout_mut(d, *seq, *lane)?;
            let x = layout
                .rows
                .iter_mut()
                .find(|x| x.id == *row)
                .ok_or(ModelError::UnknownId(AnyId::DrumRow(*row)))?;
            x.name = name.clone();
        }
        DrumCmd::ReorderRow { seq, lane, row, to } => {
            let layout = layout_mut(d, *seq, *lane)?;
            let i = layout
                .rows
                .iter()
                .position(|x| x.id == *row)
                .ok_or(ModelError::UnknownId(AnyId::DrumRow(*row)))?;
            let x = layout.rows.remove(i);
            let at = (*to).min(layout.rows.len());
            layout.rows.insert(at, x);
        }
        DrumCmd::RemoveRow { seq, lane, row } => {
            let x = find_row(d, *seq, *lane, *row)?;
            let ids = row_events(d.lane(*seq, *lane)?, &x, false);
            for id in ids {
                d.remove_event(id)?;
                r.removed.push(AnyId::Event(id));
            }
            layout_mut(d, *seq, *lane)?.rows.retain(|y| y.id != *row);
            r.removed.push(AnyId::DrumRow(*row));
        }
        DrumCmd::MoveRowToLane {
            seq,
            lane,
            row,
            to_lane,
        } => move_row(d, *seq, *lane, *row, *to_lane, r)?,
        DrumCmd::SplitRowToLane { seq, lane, row } => {
            let x = find_row(d, *seq, *lane, *row)?;
            let src = d.lane(*seq, *lane)?.clone();
            let c = d.counters_mut();
            let (new_lane, take) = (c.alloc_lane(), c.alloc_take());
            let mut nl = Lane::new(new_lane, take, x.name.clone());
            nl.midi_dest = src.midi_dest;
            nl.drum = Some(DrumLayout::default());
            let s = d.seq_mut(*seq)?;
            let at = s.lane_index(*lane).map_or(s.lanes.len(), |i| i + 1);
            s.lanes.insert(at, nl);
            r.created.extend([AnyId::Lane(new_lane), AnyId::Take(take)]);
            move_row(d, *seq, *lane, *row, new_lane, r)?;
        }
    }
    Ok(())
}
```

Replace the whole of `crates/core-model/src/command/mod.rs` with:

```rust
//! Commands: every edit is a `Command` applied by the pure [`apply`].

pub mod drum;
pub mod event;
pub mod lane;
pub mod sequence;

pub use drum::DrumCmd;
pub use event::EventCmd;
pub use lane::LaneCmd;
pub use sequence::SequenceCmd;

use crate::document::Document;
use crate::error::Result;
use crate::ids::AnyId;
use crate::selection::{Selection, ViewFilter};

/// What a command did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// IDs created, in creation order.
    pub created: Vec<AnyId>,
    /// IDs removed.
    pub removed: Vec<AnyId>,
    /// Non-fatal notes for the user.
    pub warnings: Vec<String>,
}

/// The result of applying a command.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// The new document.
    pub doc: Document,
    /// What changed.
    pub report: Report,
    /// The new selection, for commands that change it.
    pub selection: Option<Selection>,
}

/// The current selection and view filter, for commands that act on them.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The document's selection.
    pub selection: &'a Selection,
    /// The view filter of the view issuing the command.
    pub filter: &'a ViewFilter,
}

/// Every edit the model supports.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Sequence-level edits.
    Sequence(SequenceCmd),
    /// Lane and take edits.
    Lane(LaneCmd),
    /// Event edits.
    Event(EventCmd),
    /// Drum-lane edits.
    Drum(DrumCmd),
}

impl Command {
    /// The stable name, used for key bindings and undo labels.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Sequence(c) => c.name(),
            Command::Lane(c) => c.name(),
            Command::Event(c) => c.name(),
            Command::Drum(c) => c.name(),
        }
    }
}

/// Applies a command with an empty selection and no view filter.
pub fn apply(doc: &Document, cmd: &Command) -> Result<Applied> {
    let selection = Selection::default();
    let filter = ViewFilter::default();
    apply_in(
        doc,
        Context {
            selection: &selection,
            filter: &filter,
        },
        cmd,
    )
}

/// Applies a command in the context of a selection and view filter. Pure:
/// `doc` is never changed; on error nothing is returned but the error.
pub fn apply_in(doc: &Document, ctx: Context<'_>, cmd: &Command) -> Result<Applied> {
    let mut d = doc.clone();
    let mut report = Report::default();
    let _ = ctx;
    let selection = match cmd {
        Command::Sequence(c) => {
            sequence::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Lane(c) => {
            lane::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Event(c) => {
            event::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Drum(c) => {
            drum::apply(&mut d, c, &mut report)?;
            None
        }
    };
    Ok(Applied {
        doc: d,
        report,
        selection,
    })
}
```


- [ ] **Step 4: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_drum
```

Expected: PASS — `cmd_drum`: `test result: ok. 6 passed`.

- [ ] **Step 5: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/command/drum.rs crates/core-model/src/command/mod.rs crates/core-model/tests/cmd_drum.rs
git commit -m "feat(core-model): drum-lane commands"
```

---

### Task 10: Queries: derived length, routing resolution, usage and drum view

**Goal:** Read-only queries from spec §6 except flatten: derived sequence length (never stored), routing resolution (§4.4), users/use counts/unused sequences, unresolved-destination events, media in use, and the drum view.

**Files:**
- Modify: `crates/core-model/src/lib.rs`
- Create: `crates/core-model/src/query/length.rs`
- Create: `crates/core-model/src/query/mod.rs`
- Create: `crates/core-model/src/query/routing.rs`
- Create: `crates/core-model/src/query/usage.rs`
- Create (test): `crates/core-model/tests/query_usage.rs`

**Interfaces:**
- Consumes: `Document` read API; `rules::references_to`; `command::drum::row_events`; `MeterMap::barline_at_or_after`; `TempoMap` conversions.
- Produces:
  - `core_model::query::length::{reference_span(seq_len: Tick, length: Option<Tick>, LoopCount) -> Tick, seq_length(&Document, SeqId) -> Result<Tick>}`; crate-internal memoising `Lengths::{of(&mut self, &Document, SeqId) -> Tick, event_end(...)}`.
  - `core_model::query::routing::{Routing { dest: Option<DestId>, group_channel: Option<(u8, u8)> }, resolve_with(&Document, fallback: Option<DestId>, &Event) -> Routing, resolve(&Document, &Lane, &Event) -> Routing, resolve_event(&Document, EventId) -> Result<Routing>}`.
  - `core_model::query::usage::{users_of, use_count, use_counts, unused_sequences, unresolved_events, media_in_use, drum_view}` (signatures in the code below), all re-exported from `core_model::query`.

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/query_usage.rs`:

```rust
use core_model::command::{Command, DrumCmd, EventCmd, LaneCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Destination, Document, DocumentRepr, MediaItem, Settings};
use core_model::event::{EventBody, EventSpec, Fade, LoopCount};
use core_model::ids::{Counters, DestId, EventId, LaneId, MediaId, SeqId};
use core_model::query::{
    Routing, drum_view, media_in_use, reference_span, resolve_event, seq_length, unresolved_events,
    unused_sequences, use_count, use_counts, users_of,
};
use core_model::sequence::SeqLength;
use core_model::time::{PPQ, Tick};

fn run(d: &Document, c: Command) -> Document {
    apply(d, &c).unwrap().doc
}

fn insert(d: &Document, seq: u64, lane: u64, specs: Vec<EventSpec>) -> Document {
    run(
        d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(seq),
            lane: LaneId(lane),
            events: specs,
        }),
    )
}

/// dest0 = MIDI endpoint group 1 channel 9, dest1 = plugin; media0 = 1 s at
/// 48 kHz; seq0 (lane0, dest0) and seq1 (lane1, no dest).
fn base() -> Document {
    let d = Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![
            Destination {
                id: DestId(0),
                name: "Ext".into(),
                kind: DestKind::MidiEndpoint {
                    endpoint_ref: "e".into(),
                    group: 1,
                    channel: 9,
                },
                default_patch: None,
            },
            Destination {
                id: DestId(1),
                name: "Plug".into(),
                kind: DestKind::PluginInstrument {
                    plugin_ref: "p".into(),
                },
                default_patch: None,
            },
        ],
        outputs: vec![],
        media: vec![MediaItem {
            id: MediaId(0),
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 48_000,
            content_hash: None,
        }],
        sequences: vec![],
        counters: Counters {
            dest: 2,
            media: 1,
            ..Counters::default()
        },
    })
    .unwrap();
    let d = run(&d, Command::Sequence(SequenceCmd::New { name: "A".into() }));
    let d = run(&d, Command::Sequence(SequenceCmd::New { name: "B".into() }));
    run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(DestId(0)),
        }),
    )
}

fn note(t: i64, dur: i64) -> EventSpec {
    EventSpec::new(Tick(t), EventBody::note(60, 100, Tick(dur)))
}

fn reference(t: i64, length: Option<i64>, loop_count: LoopCount) -> EventSpec {
    EventSpec::new(
        Tick(t),
        EventBody::Reference {
            seq: SeqId(1),
            length: length.map(Tick),
            loop_count,
            transpose: 0,
        },
    )
}

#[test]
fn empty_sequence_is_one_bar() {
    assert_eq!(seq_length(&base(), SeqId(0)).unwrap(), Tick(PPQ * 4));
}

#[test]
fn derived_length_rounds_up_to_a_barline() {
    let d = insert(&base(), 0, 0, vec![note(PPQ * 4, PPQ / 2)]);
    assert_eq!(seq_length(&d, SeqId(0)).unwrap(), Tick(PPQ * 8));
    let d = run(
        &d,
        Command::Sequence(SequenceCmd::SetLength {
            seq: SeqId(0),
            length: SeqLength::Locked(Tick(PPQ * 3)),
        }),
    );
    assert_eq!(seq_length(&d, SeqId(0)).unwrap(), Tick(PPQ * 3));
}

#[test]
fn lane_length_counts_even_when_empty() {
    let d = run(
        &base(),
        Command::Lane(LaneCmd::SetLength {
            seq: SeqId(0),
            lane: LaneId(0),
            length: Some(Tick(PPQ * 9)),
            looped: false,
            locked: false,
        }),
    );
    assert_eq!(seq_length(&d, SeqId(0)).unwrap(), Tick(PPQ * 12));
}

#[test]
fn reference_spans() {
    let bar = Tick(PPQ * 4);
    assert_eq!(
        reference_span(bar, None, LoopCount::Count(3)),
        Tick(PPQ * 12)
    );
    assert_eq!(
        reference_span(bar, Some(Tick(PPQ * 2)), LoopCount::Count(2)),
        bar
    );
    assert_eq!(
        reference_span(bar, Some(Tick(PPQ * 6)), LoopCount::Fill),
        Tick(PPQ * 6)
    );
    assert_eq!(reference_span(bar, None, LoopCount::Fill), bar);
    // Length longer than the sequence and shorter than the sequence.
    let d = insert(
        &base(),
        0,
        0,
        vec![reference(0, Some(PPQ * 10), LoopCount::Count(1))],
    );
    assert_eq!(seq_length(&d, SeqId(0)).unwrap(), Tick(PPQ * 12));
    let d = insert(
        &base(),
        0,
        0,
        vec![reference(0, Some(PPQ), LoopCount::Count(2))],
    );
    assert_eq!(seq_length(&d, SeqId(0)).unwrap(), Tick(PPQ * 4));
}

#[test]
fn audio_region_extent_uses_the_tempo_map() {
    let region = EventSpec::new(
        Tick(PPQ * 3),
        EventBody::AudioRegion {
            media: MediaId(0),
            source_offset: 0,
            length: 48_000,
            gain_db: 0.0,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
        },
    );
    // 1 s at 120 BPM = 2 beats, so it ends at beat 5 -> rounded to 2 bars.
    let d = insert(&base(), 0, 0, vec![region]);
    assert_eq!(seq_length(&d, SeqId(0)).unwrap(), Tick(PPQ * 8));
}

#[test]
fn routing_resolution_with_and_without_overrides() {
    let mut gc = note(0, 1);
    gc.group_channel = Some((2, 3));
    let mut plug = note(0, 1);
    plug.dest = Some(DestId(1));
    let mut gc_plug = note(0, 1);
    gc_plug.dest = Some(DestId(1));
    gc_plug.group_channel = Some((4, 5));
    let d = insert(&base(), 0, 0, vec![note(0, 1), gc, plug, gc_plug]);
    let d = insert(&d, 1, 1, vec![note(0, 1)]);
    let r = |id| resolve_event(&d, EventId(id)).unwrap();
    assert_eq!(
        r(0),
        Routing {
            dest: Some(DestId(0)),
            group_channel: Some((1, 9))
        }
    );
    assert_eq!(
        r(1),
        Routing {
            dest: Some(DestId(0)),
            group_channel: Some((2, 3))
        }
    );
    assert_eq!(
        r(2),
        Routing {
            dest: Some(DestId(1)),
            group_channel: Some((0, 0))
        }
    );
    assert_eq!(
        r(3),
        Routing {
            dest: Some(DestId(1)),
            group_channel: Some((4, 5))
        }
    );
    assert_eq!(
        r(4),
        Routing {
            dest: None,
            group_channel: None
        }
    );
    assert_eq!(unresolved_events(&d), vec![EventId(4)]);
}

#[test]
fn users_counts_and_unused() {
    let d = insert(
        &base(),
        0,
        0,
        vec![
            reference(0, None, LoopCount::Count(1)),
            reference(PPQ * 4, None, LoopCount::Count(1)),
        ],
    );
    assert_eq!(users_of(&d, SeqId(1)), vec![EventId(0), EventId(1)]);
    assert_eq!(use_count(&d, SeqId(1)), 2);
    assert_eq!(use_counts(&d).get(&SeqId(0)), Some(&0));
    assert_eq!(unused_sequences(&d), vec![SeqId(0)]);
}

#[test]
fn media_in_use_lists_regions() {
    let region = EventSpec::new(
        Tick(0),
        EventBody::AudioRegion {
            media: MediaId(0),
            source_offset: 0,
            length: 10,
            gain_db: 0.0,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
        },
    );
    let d = insert(&base(), 1, 1, vec![region]);
    assert_eq!(media_in_use(&d).get(&MediaId(0)), Some(&vec![EventId(0)]));
}

#[test]
fn drum_view_groups_notes_by_row() {
    let d = insert(&base(), 0, 0, vec![note(0, 1), note(PPQ, 1)]);
    let d = run(
        &d,
        Command::Drum(DrumCmd::MarkDrumLane {
            seq: SeqId(0),
            lane: LaneId(0),
            drum: true,
        }),
    );
    let view = drum_view(&d, SeqId(0), LaneId(0)).unwrap();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0].0.note, 60);
    assert_eq!(view[0].1, vec![EventId(0), EventId(1)]);
}
```


- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test query_usage
```

Expected: FAIL to compile — `error[E0432]: unresolved import `core_model::query``.

- [ ] **Step 3: Implement**

Replace the whole of `crates/core-model/src/lib.rs` with:

```rust
//! `core-model`: the VizDAW project model.
//!
//! A pure-Rust crate holding a VizDAW project and every edit that can be made
//! to it. It has no audio, MIDI I/O, UI or OS dependencies. All edits are
//! [`command::Command`] values applied by the pure function
//! [`command::apply`]; documents are immutable values that share structure.
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod command;
pub mod document;
pub mod error;
pub mod event;
pub mod ids;
pub mod lane;
pub mod meter;
pub mod query;
pub mod rules;
pub mod selection;
pub mod sequence;
pub mod tempo;
pub mod time;

pub use error::{ModelError, Result};
```

Create `crates/core-model/src/query/length.rs`:

```rust
//! Derived sequence lengths and event extents.

use crate::document::Document;
use crate::error::Result;
use crate::event::{Event, EventBody, LoopCount};
use crate::ids::SeqId;
use crate::sequence::{SeqLength, Sequence};
use crate::time::Tick;
use std::collections::{BTreeMap, BTreeSet};

/// Total ticks a reference use occupies, given the referenced sequence's
/// length `seq_len`: `Count(n)` plays the per-use length `n` times; `Fill`
/// plays the per-use length once (filling it with repeats of the sequence).
pub fn reference_span(seq_len: Tick, length: Option<Tick>, loop_count: LoopCount) -> Tick {
    let use_len = length.unwrap_or(seq_len);
    match loop_count {
        LoopCount::Count(n) => use_len.times(i64::from(n.max(1))),
        LoopCount::Fill => use_len,
    }
}

/// Memoising length calculator (guards against cycles in corrupt input).
#[derive(Default)]
pub(crate) struct Lengths {
    memo: BTreeMap<SeqId, Tick>,
    visiting: BTreeSet<SeqId>,
}

impl Lengths {
    /// The playing length of `seq`: locked, or the end of the longest lane
    /// rounded up to a barline (at least one bar).
    pub(crate) fn of(&mut self, doc: &Document, seq: SeqId) -> Tick {
        if let Some(t) = self.memo.get(&seq) {
            return *t;
        }
        let Ok(s) = doc.sequence(seq) else {
            return Tick::ZERO;
        };
        if let SeqLength::Locked(t) = s.length {
            return t;
        }
        if !self.visiting.insert(seq) {
            return Tick::ZERO;
        }
        let mut end = Tick::ZERO;
        for lane in &s.lanes {
            let lane_end = match lane.length {
                Some(l) => l,
                None => lane
                    .active()
                    .map(|t| {
                        t.events
                            .values()
                            .map(|e| self.event_end(doc, s, e))
                            .max()
                            .unwrap_or(Tick::ZERO)
                    })
                    .unwrap_or(Tick::ZERO),
            };
            end = end.max(lane_end);
        }
        self.visiting.remove(&seq);
        let len = s.meter.barline_at_or_after(end.max(Tick(1)));
        self.memo.insert(seq, len);
        len
    }

    /// Where an event in `owner` ends.
    pub(crate) fn event_end(&mut self, doc: &Document, owner: &Sequence, e: &Event) -> Tick {
        match &e.body {
            EventBody::Note { duration, .. } => e.tick + *duration,
            EventBody::Reference {
                seq,
                length,
                loop_count,
                ..
            } => {
                let b = self.of(doc, *seq);
                e.tick + reference_span(b, *length, *loop_count)
            }
            EventBody::AudioRegion { media, length, .. } => {
                let rate = doc
                    .media()
                    .get(media)
                    .map_or(48_000, |m| m.sample_rate.max(1));
                let start = owner.tempo.tick_to_seconds(e.tick);
                owner
                    .tempo
                    .seconds_to_tick(start + *length as f64 / f64::from(rate))
            }
            _ => e.tick,
        }
    }
}

/// The playing length of a sequence (derived lengths are computed, never
/// stored).
pub fn seq_length(doc: &Document, seq: SeqId) -> Result<Tick> {
    doc.sequence(seq)?;
    Ok(Lengths::default().of(doc, seq))
}
```

Create `crates/core-model/src/query/mod.rs`:

```rust
//! Read-only queries over a [`crate::document::Document`].

pub mod length;
pub mod routing;
pub mod usage;

pub use length::{reference_span, seq_length};
pub use routing::{Routing, resolve, resolve_event};
pub use usage::{
    drum_view, media_in_use, unresolved_events, unused_sequences, use_count, use_counts, users_of,
};
```

Create `crates/core-model/src/query/routing.rs`:

```rust
//! Routing resolution: event dest, else lane dest; event group/channel,
//! else the destination's (plugins use group 0, channel 0).

use crate::document::{DestKind, Document};
use crate::error::Result;
use crate::event::Event;
use crate::ids::{DestId, EventId};
use crate::lane::Lane;

/// Where a MIDI event resolves to. `dest == None` means unresolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Routing {
    /// Resolved destination.
    pub dest: Option<DestId>,
    /// Resolved `(group, channel)`; `None` when there is neither an override
    /// nor a destination.
    pub group_channel: Option<(u8, u8)>,
}

/// Resolves an event's destination override against `fallback` (the lane's
/// destination).
pub fn resolve_with(doc: &Document, fallback: Option<DestId>, ev: &Event) -> Routing {
    let dest = ev.dest.or(fallback);
    let from_dest = dest
        .and_then(|d| doc.instruments().get(&d))
        .map(|d| match d.kind {
            DestKind::MidiEndpoint { group, channel, .. } => (group, channel),
            DestKind::PluginInstrument { .. } => (0, 0),
        });
    Routing {
        dest,
        group_channel: ev.group_channel.or(from_dest),
    }
}

/// Resolves an event on `lane`.
pub fn resolve(doc: &Document, lane: &Lane, ev: &Event) -> Routing {
    resolve_with(doc, lane.midi_dest, ev)
}

/// Resolves an event by ID.
pub fn resolve_event(doc: &Document, id: EventId) -> Result<Routing> {
    let (ev, loc) = doc.event(id)?;
    let lane = doc.lane(loc.seq, loc.lane)?;
    Ok(resolve(doc, lane, ev))
}
```

Create `crates/core-model/src/query/usage.rs`:

```rust
//! Usage queries: users and use counts of sequences, unused sequences,
//! unresolved events, media in use, and the drum view.

use super::routing::resolve;
use crate::command::drum::row_events;
use crate::document::Document;
use crate::error::Result;
use crate::event::EventBody;
use crate::ids::{EventId, LaneId, MediaId, SeqId};
use crate::lane::DrumRow;
use crate::rules::references_to;
use std::collections::BTreeMap;

/// Reference events (in any take) that use `seq`, sorted by ID.
pub fn users_of(doc: &Document, seq: SeqId) -> Vec<EventId> {
    let mut v = references_to(doc, seq);
    v.sort();
    v
}

/// How many reference events use `seq`.
pub fn use_count(doc: &Document, seq: SeqId) -> usize {
    references_to(doc, seq).len()
}

/// Use counts for every sequence (zero included), in one pass.
pub fn use_counts(doc: &Document) -> BTreeMap<SeqId, usize> {
    let mut m: BTreeMap<SeqId, usize> = doc.sequences().keys().map(|k| (*k, 0)).collect();
    for (_, e) in doc.all_events() {
        if let EventBody::Reference { seq, .. } = e.body {
            *m.entry(seq).or_insert(0) += 1;
        }
    }
    m
}

/// Sequences that no reference uses (top-level songs included; callers
/// decide which to keep).
pub fn unused_sequences(doc: &Document) -> Vec<SeqId> {
    use_counts(doc)
        .into_iter()
        .filter(|(_, n)| *n == 0)
        .map(|(s, _)| s)
        .collect()
}

/// MIDI events (in any take) with no resolvable destination.
pub fn unresolved_events(doc: &Document) -> Vec<EventId> {
    let mut out = Vec::new();
    for s in doc.sequences().values() {
        for l in &s.lanes {
            for t in &l.takes {
                for e in t.events.values() {
                    if e.body.is_midi() && resolve(doc, l, e).dest.is_none() {
                        out.push(e.id);
                    }
                }
            }
        }
    }
    out
}

/// Each media item in use, with the regions using it.
pub fn media_in_use(doc: &Document) -> BTreeMap<MediaId, Vec<EventId>> {
    let mut m: BTreeMap<MediaId, Vec<EventId>> = BTreeMap::new();
    for (_, e) in doc.all_events() {
        if let EventBody::AudioRegion { media, .. } = e.body {
            m.entry(media).or_default().push(e.id);
        }
    }
    m
}

/// The rows of a drum lane with the active-take notes in each.
pub fn drum_view(doc: &Document, seq: SeqId, lane: LaneId) -> Result<Vec<(DrumRow, Vec<EventId>)>> {
    let l = doc.lane(seq, lane)?;
    Ok(l.drum
        .as_ref()
        .map(|layout| {
            layout
                .rows
                .iter()
                .map(|r| (r.clone(), row_events(l, r, true)))
                .collect()
        })
        .unwrap_or_default())
}
```


- [ ] **Step 4: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test query_usage
```

Expected: PASS — `query_usage`: `test result: ok. 9 passed`.

- [ ] **Step 5: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/lib.rs crates/core-model/src/query/length.rs crates/core-model/src/query/mod.rs crates/core-model/src/query/routing.rs crates/core-model/src/query/usage.rs crates/core-model/tests/query_usage.rs
git commit -m "feat(core-model): length, routing and usage queries"
```

---

### Task 11: Flatten query with nested timing, loops, length and transpose

**Goal:** `flatten(doc, seq, range, filter)` walks lanes and references recursively, applying per-use length/loop/transpose/mute, active take, lane mute/solo/length/loop/shift/quantize, routing, and the nested-timing rule (seconds always from the playing sequence's tempo map).

**Files:**
- Create: `crates/core-model/src/query/flatten.rs`
- Modify: `crates/core-model/src/query/mod.rs`
- Create (test): `crates/core-model/tests/query_flatten.rs`

**Interfaces:**
- Consumes: `query::length::Lengths` and `query::routing::resolve` (Task 10); `command::event::shift_pitch` (Task 8); `ViewFilter` (Task 6).
- Produces:
  - `core_model::query::flatten::{PathStep { seq, lane, event }, ResolvedEvent { tick, seconds, dest, group_channel, audio_out, body, path: Vec<PathStep> }, flatten(&Document, SeqId, Option<(Tick, Tick)>, &ViewFilter) -> Result<std::vec::IntoIter<ResolvedEvent>>}` (re-exported from `core_model::query`).
  - Crate-internal `query::flatten::Walk` with `new(&Document, &TempoMap, &ViewFilter, range)`, `reference(&mut self, &Event, lt: Tick, offset: Tick, win: (Tick, Tick), transpose: i32, &mut Vec<PathStep>)` and field `out: Vec<ResolvedEvent>` (used by the Flatten command in Task 12).

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/query_flatten.rs`:

```rust
use core_model::command::{Command, EventCmd, LaneCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Destination, Document, DocumentRepr, Settings};
use core_model::event::{EventBody, EventKind, EventSpec, LoopCount};
use core_model::ids::{Counters, DestId, EventId, LaneId, SeqId};
use core_model::query::flatten::{PathStep, ResolvedEvent, flatten};
use core_model::selection::ViewFilter;
use core_model::tempo::{Bpm, TempoMap};
use core_model::time::{PPQ, Tick};

const Q: i64 = PPQ;

fn run(d: &Document, c: Command) -> Document {
    apply(d, &c).unwrap().doc
}

fn insert(d: &Document, seq: u64, lane: u64, specs: Vec<EventSpec>) -> Document {
    run(
        d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(seq),
            lane: LaneId(lane),
            events: specs,
        }),
    )
}

fn note(beat: i64, pitch: u8, beats: i64) -> EventSpec {
    EventSpec::new(Tick(beat * Q), EventBody::note(pitch, 100, Tick(beats * Q)))
}

fn reference(
    beat: i64,
    length_beats: Option<i64>,
    loop_count: LoopCount,
    transpose: i8,
) -> EventSpec {
    EventSpec::new(
        Tick(beat * Q),
        EventBody::Reference {
            seq: SeqId(1),
            length: length_beats.map(|b| Tick(b * Q)),
            loop_count,
            transpose,
        },
    )
}

/// dest0 (plugin) and dest1 (MIDI endpoint group 2 channel 5).
/// seq0 "Song" (lane0, dest0) and seq1 "Sub" (lane1, dest1), both 120 BPM 4/4.
fn base() -> Document {
    let d = Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![
            Destination {
                id: DestId(0),
                name: "Plug".into(),
                kind: DestKind::PluginInstrument {
                    plugin_ref: "p".into(),
                },
                default_patch: None,
            },
            Destination {
                id: DestId(1),
                name: "Ext".into(),
                kind: DestKind::MidiEndpoint {
                    endpoint_ref: "e".into(),
                    group: 2,
                    channel: 5,
                },
                default_patch: None,
            },
        ],
        outputs: vec![],
        media: vec![],
        sequences: vec![],
        counters: Counters {
            dest: 2,
            ..Counters::default()
        },
    })
    .unwrap();
    let d = run(
        &d,
        Command::Sequence(SequenceCmd::New {
            name: "Song".into(),
        }),
    );
    let d = run(
        &d,
        Command::Sequence(SequenceCmd::New { name: "Sub".into() }),
    );
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(DestId(0)),
        }),
    );
    run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(1),
            lane: LaneId(1),
            dest: Some(DestId(1)),
        }),
    )
}

fn flat(d: &Document) -> Vec<ResolvedEvent> {
    flatten(d, SeqId(0), None, &ViewFilter::default())
        .unwrap()
        .collect()
}

/// `(beat, pitch, duration in beats)` of every resolved note.
fn notes(d: &Document) -> Vec<(f64, u8, f64)> {
    flat(d)
        .into_iter()
        .filter_map(|r| match r.body {
            EventBody::Note {
                pitch, duration, ..
            } => Some((
                r.tick.0 as f64 / Q as f64,
                pitch,
                duration.0 as f64 / Q as f64,
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn plain_notes_resolve_with_lane_routing() {
    let d = insert(&base(), 0, 0, vec![note(1, 62, 1), note(0, 60, 1)]);
    let r = flat(&d);
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].tick, Tick(0));
    assert_eq!(
        (r[0].dest, r[0].group_channel),
        (Some(DestId(0)), Some((0, 0)))
    );
    assert!((r[1].seconds - 0.5).abs() < 1e-9);
    assert_eq!(
        r[1].path,
        vec![PathStep {
            seq: SeqId(0),
            lane: LaneId(0),
            event: EventId(0)
        }]
    );
}

#[test]
fn nested_events_use_inner_routing_and_carry_their_path() {
    let d = insert(&base(), 1, 1, vec![note(0, 60, 1)]);
    let d = insert(&d, 0, 0, vec![reference(4, None, LoopCount::Count(1), 0)]);
    let r = flat(&d);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].tick, Tick(4 * Q));
    assert_eq!(
        (r[0].dest, r[0].group_channel),
        (Some(DestId(1)), Some((2, 5)))
    );
    assert_eq!(
        r[0].path,
        vec![
            PathStep {
                seq: SeqId(0),
                lane: LaneId(0),
                event: EventId(1)
            },
            PathStep {
                seq: SeqId(1),
                lane: LaneId(1),
                event: EventId(0)
            },
        ]
    );
}

#[test]
fn nested_timing_uses_the_playing_sequences_tempo() {
    let d = insert(&base(), 1, 1, vec![note(1, 60, 1)]);
    let d = run(
        &d,
        Command::Sequence(SequenceCmd::SetTempo {
            seq: SeqId(1),
            tempo: TempoMap::constant(Bpm::whole(60)).unwrap(),
        }),
    );
    let d = insert(&d, 0, 0, vec![reference(4, None, LoopCount::Count(1), 0)]);
    let r = flat(&d);
    // Beat 5 at the song's 120 BPM is 2.5 s; the sub's own 60 BPM is ignored.
    assert_eq!(r[0].tick, Tick(5 * Q));
    assert!((r[0].seconds - 2.5).abs() < 1e-9);
}

// Review focus #2: loop_count combined with a per-use length shorter than the sequence.
#[test]
fn loop_count_with_length_shorter_than_sequence() {
    let d = insert(
        &base(),
        1,
        1,
        vec![
            note(0, 60, 1),
            note(1, 61, 2),
            note(2, 62, 1),
            note(3, 63, 1),
        ],
    );
    let d = insert(
        &d,
        0,
        0,
        vec![reference(0, Some(2), LoopCount::Count(3), 0)],
    );
    assert_eq!(
        notes(&d),
        vec![
            (0.0, 60, 1.0),
            (1.0, 61, 1.0),
            (2.0, 60, 1.0),
            (3.0, 61, 1.0),
            (4.0, 60, 1.0),
            (5.0, 61, 1.0)
        ]
    );
}

#[test]
fn length_longer_than_sequence_lets_looped_lanes_fill() {
    let d = insert(&base(), 1, 1, vec![note(0, 60, 1)]);
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetLength {
            seq: SeqId(1),
            lane: LaneId(1),
            length: Some(Tick(Q)),
            looped: true,
            locked: false,
        }),
    );
    let d = run(
        &d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(1),
            name: "once".into(),
            index: None,
        }),
    );
    let d = insert(&d, 1, 2, vec![note(0, 70, 1)]);
    let d = insert(
        &d,
        0,
        0,
        vec![reference(0, Some(6), LoopCount::Count(1), 0)],
    );
    let n = notes(&d);
    assert_eq!(n.iter().filter(|x| x.1 == 60).count(), 6);
    assert_eq!(n.iter().filter(|x| x.1 == 70).count(), 1);
}

#[test]
fn fill_repeats_sequence_and_truncates_last_pass() {
    let d = insert(&base(), 1, 1, vec![note(0, 60, 1), note(3, 63, 1)]);
    let d = insert(&d, 0, 0, vec![reference(0, Some(6), LoopCount::Fill, 0)]);
    assert_eq!(
        notes(&d),
        vec![(0.0, 60, 1.0), (3.0, 63, 1.0), (4.0, 60, 1.0)]
    );
}

#[test]
fn transpose_accumulates_and_clamps() {
    let d = insert(&base(), 1, 1, vec![note(0, 120, 1), note(1, 5, 1)]);
    let d = insert(&d, 0, 0, vec![reference(0, None, LoopCount::Count(1), 12)]);
    assert_eq!(notes(&d), vec![(0.0, 127, 1.0), (1.0, 17, 1.0)]);
    let d = insert(&base(), 1, 1, vec![note(0, 3, 1)]);
    let d = insert(&d, 0, 0, vec![reference(0, None, LoopCount::Count(1), -12)]);
    assert_eq!(notes(&d), vec![(0.0, 0, 1.0)]);
}

#[test]
fn mute_solo_and_active_take() {
    let d = insert(&base(), 1, 1, vec![note(0, 60, 1)]);
    let mut muted_ref = reference(0, None, LoopCount::Count(1), 0);
    muted_ref.mute = true;
    let d = insert(&d, 0, 0, vec![muted_ref, note(1, 50, 1)]);
    assert_eq!(notes(&d), vec![(1.0, 50, 1.0)]);
    let d = run(
        &d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "solo".into(),
            index: None,
        }),
    );
    let d = insert(&d, 0, 2, vec![note(2, 52, 1)]);
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetFlags {
            seq: SeqId(0),
            lane: LaneId(2),
            mute: None,
            solo: Some(true),
            arm: None,
        }),
    );
    assert_eq!(notes(&d), vec![(2.0, 52, 1.0)]);
    let d = run(
        &d,
        Command::Lane(LaneCmd::AddTake {
            seq: SeqId(0),
            lane: LaneId(2),
            name: "T2".into(),
        }),
    );
    assert_eq!(notes(&d), vec![]);
}

#[test]
fn pickup_plays_only_before_the_first_pass() {
    let d = run(
        &base(),
        Command::Sequence(SequenceCmd::SetStartOffset {
            seq: SeqId(1),
            offset: Tick(Q),
        }),
    );
    let d = insert(&d, 1, 1, vec![note(-1, 59, 1), note(0, 60, 1)]);
    let d = insert(&d, 0, 0, vec![reference(4, None, LoopCount::Count(2), 0)]);
    assert_eq!(
        notes(&d),
        vec![(3.0, 59, 1.0), (4.0, 60, 1.0), (8.0, 60, 1.0)]
    );
}

#[test]
fn range_and_view_filter() {
    let cc = EventSpec::new(Tick(0), EventBody::Cc { index: 1, value: 1 });
    let d = insert(&base(), 0, 0, vec![note(0, 60, 1), note(2, 62, 1), cc]);
    let in_range: Vec<_> = flatten(
        &d,
        SeqId(0),
        Some((Tick(Q), Tick(3 * Q))),
        &ViewFilter::default(),
    )
    .unwrap()
    .collect();
    assert_eq!(in_range.len(), 1);
    let mut f = ViewFilter::default();
    f.hidden_kinds.insert(EventKind::Cc);
    assert_eq!(flatten(&d, SeqId(0), None, &f).unwrap().count(), 2);
    let mut f = ViewFilter::default();
    f.hidden_lanes.insert(LaneId(0));
    assert_eq!(flatten(&d, SeqId(0), None, &f).unwrap().count(), 0);
}

#[test]
fn three_levels_of_nesting_offset_correctly() {
    // seq2 (one note at beat 1) <- seq1 at beat 2 <- seq0 at beat 4.
    let d = run(
        &base(),
        Command::Sequence(SequenceCmd::New {
            name: "Inner".into(),
        }),
    );
    let d = insert(&d, 2, 2, vec![note(1, 64, 1)]);
    let inner = EventSpec::new(Tick(2 * Q), EventBody::reference(SeqId(2)));
    let d = insert(&d, 1, 1, vec![inner]);
    let d = insert(&d, 0, 0, vec![reference(4, None, LoopCount::Count(1), 0)]);
    let r = flat(&d);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].tick, Tick(7 * Q));
    assert_eq!(r[0].path.len(), 3);
}

#[test]
fn reference_starting_before_the_window_plays_its_inside_part() {
    let d = insert(&base(), 1, 1, vec![note(0, 60, 1), note(2, 62, 1)]);
    // Starts one beat before bar 1: the note at its beat 0 is cut, beat 2 plays at beat 1.
    let d = insert(&d, 0, 0, vec![reference(-1, None, LoopCount::Count(1), 0)]);
    assert_eq!(notes(&d), vec![(1.0, 62, 1.0)]);
}

#[test]
fn lane_length_cuts_notes_and_references() {
    let d = insert(&base(), 1, 1, vec![note(0, 60, 1), note(3, 63, 1)]);
    let d = insert(
        &d,
        0,
        0,
        vec![note(0, 50, 4), reference(1, None, LoopCount::Count(1), 0)],
    );
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetLength {
            seq: SeqId(0),
            lane: LaneId(0),
            length: Some(Tick(2 * Q)),
            looped: false,
            locked: false,
        }),
    );
    // The long note is cut at the lane end; the referenced note at beat 4 does not play.
    assert_eq!(notes(&d), vec![(0.0, 50, 2.0), (1.0, 60, 1.0)]);
}

#[test]
fn looped_lane_passes_start_at_the_loop_point() {
    let d = insert(&base(), 0, 0, vec![note(-1, 40, 1), note(0, 41, 1)]);
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetLength {
            seq: SeqId(0),
            lane: LaneId(0),
            length: Some(Tick(2 * Q)),
            looped: true,
            locked: false,
        }),
    );
    // Negative ticks never play in a looped lane; the beat-0 note repeats every 2 beats.
    assert_eq!(notes(&d), vec![(0.0, 41, 1.0), (2.0, 41, 1.0)]);
}
```


- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test query_flatten
```

Expected: FAIL to compile — `error[E0432]: unresolved import `core_model::query::flatten``.

- [ ] **Step 3: Implement**

Create `crates/core-model/src/query/flatten.rs`:

```rust
//! Flattening: resolve a sequence and everything it references into
//! absolute, routed events.
//!
//! Nested timing rule: ticks inside a reference are offset into the playing
//! sequence's tick space, and `seconds` always comes from the playing
//! (top-level) sequence's tempo map. A referenced sequence's own tempo and
//! meter are never consulted here.

use super::length::Lengths;
use super::routing::resolve;
use crate::command::event::shift_pitch;
use crate::document::Document;
use crate::error::Result;
use crate::event::{Event, EventBody, LoopCount};
use crate::ids::{DestId, EventId, LaneId, OutputPathId, SeqId};
use crate::lane::Lane;
use crate::selection::ViewFilter;
use crate::sequence::Sequence;
use crate::tempo::TempoMap;
use crate::time::Tick;

/// One step of the path from the flattened sequence to a source event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathStep {
    /// Sequence holding the event.
    pub seq: SeqId,
    /// Lane holding the event.
    pub lane: LaneId,
    /// The event (a reference for every step but the last).
    pub event: EventId,
}

/// A playable event with absolute time and resolved routing.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEvent {
    /// Absolute tick in the flattened sequence.
    pub tick: Tick,
    /// Seconds from tick 0 under the flattened sequence's tempo map.
    pub seconds: f64,
    /// Resolved MIDI destination (None for audio or unresolved events).
    pub dest: Option<DestId>,
    /// Resolved `(group, channel)`.
    pub group_channel: Option<(u8, u8)>,
    /// Audio output path for audio regions.
    pub audio_out: Option<OutputPathId>,
    /// The body after transpose and duration clipping.
    pub body: EventBody,
    /// Path from the flattened sequence down to the source event.
    pub path: Vec<PathStep>,
}

pub(crate) struct Walk<'a> {
    doc: &'a Document,
    tempo: &'a TempoMap,
    filter: &'a ViewFilter,
    range: (Tick, Tick),
    lengths: Lengths,
    stack: Vec<SeqId>,
    pub(crate) out: Vec<ResolvedEvent>,
}

impl<'a> Walk<'a> {
    pub(crate) fn new(
        doc: &'a Document,
        tempo: &'a TempoMap,
        filter: &'a ViewFilter,
        range: (Tick, Tick),
    ) -> Walk<'a> {
        Walk {
            doc,
            tempo,
            filter,
            range,
            lengths: Lengths::default(),
            stack: Vec::new(),
            out: Vec::new(),
        }
    }

    /// Walks sequence `s` placed at `offset`, playing its local window
    /// `win`, with accumulated `transpose`.
    fn sequence(
        &mut self,
        s: &Sequence,
        offset: Tick,
        win: (Tick, Tick),
        transpose: i32,
        path: &mut Vec<PathStep>,
    ) {
        if win.0 >= win.1 || self.stack.contains(&s.id) {
            return;
        }
        self.stack.push(s.id);
        let any_solo = s.lanes.iter().any(|l| l.solo);
        for lane in &s.lanes {
            if lane.mute || (any_solo && !lane.solo) || self.filter.hidden_lanes.contains(&lane.id)
            {
                continue;
            }
            self.lane(s.id, lane, offset, win, transpose, path);
        }
        self.stack.pop();
    }

    fn lane(
        &mut self,
        seq: SeqId,
        lane: &Lane,
        offset: Tick,
        win: (Tick, Tick),
        transpose: i32,
        path: &mut Vec<PathStep>,
    ) {
        let Some(take) = lane.active() else {
            return;
        };
        let (period, limit) = match lane.length {
            Some(l) if lane.looped && l.0 > 0 => (Some(l), l),
            Some(l) => (None, l),
            None => (None, Tick::MAX),
        };
        let mut k = 0i64;
        loop {
            let base = period.map_or(Tick::ZERO, |p| p.times(k));
            if period.is_some() && base >= win.1 {
                break;
            }
            // A pass spans [base, base + limit) (shifted); playback of the
            // lane stops at its own length, and a looped pass starts at its
            // loop point.
            let pass_start = if period.is_some() {
                base + lane.play_shift
            } else {
                Tick::MIN
            };
            let lw = (
                win.0.max(pass_start),
                win.1.min(base + limit + lane.play_shift),
            );
            for e in take.events.values() {
                if e.tick >= limit {
                    continue;
                }
                if e.mute || !self.filter.allows(lane.id, e.body.kind()) {
                    continue;
                }
                let mut lt = e.tick + base;
                if let (Some(q), EventBody::Note { .. }) = (lane.play_quantize, &e.body) {
                    lt = q.apply(lt);
                }
                lt = lt + lane.play_shift;
                // A reference that starts before the window still plays the
                // part of it that falls inside.
                let is_ref = matches!(e.body, EventBody::Reference { .. });
                if (lt < lw.0 && !is_ref) || lt >= lw.1 {
                    continue;
                }
                path.push(PathStep {
                    seq,
                    lane: lane.id,
                    event: e.id,
                });
                if is_ref {
                    self.reference(e, lt, offset, lw, transpose, path);
                } else {
                    self.emit(lane, e, lt, offset, lw.1, transpose, path);
                }
                path.pop();
            }
            if period.is_none() {
                break;
            }
            k += 1;
        }
    }

    /// Expands reference `e` placed at local tick `lt` of a sequence playing
    /// its local window `win`.
    pub(crate) fn reference(
        &mut self,
        e: &Event,
        lt: Tick,
        offset: Tick,
        win: (Tick, Tick),
        transpose: i32,
        path: &mut Vec<PathStep>,
    ) {
        let EventBody::Reference {
            seq,
            length,
            loop_count,
            transpose: tp,
        } = e.body
        else {
            return;
        };
        let doc = self.doc;
        let Ok(target) = doc.sequence(seq) else {
            return;
        };
        let b = self.lengths.of(doc, seq);
        if b.0 <= 0 {
            return;
        }
        let use_len = length.unwrap_or(b);
        let (iter_len, count) = match loop_count {
            LoopCount::Count(n) => (use_len, i64::from(n.max(1))),
            LoopCount::Fill => (b, (use_len.0 + b.0 - 1) / b.0),
        };
        let total = match loop_count {
            LoopCount::Count(_) => iter_len.times(count),
            LoopCount::Fill => use_len,
        };
        let end = (lt + total).min(win.1);
        for i in 0..count {
            let start = lt + iter_len.times(i);
            if start >= end {
                break;
            }
            let lo = if i == 0 {
                -target.start_offset
            } else {
                Tick::ZERO
            }
            .max(win.0 - start);
            let hi = iter_len.min(end - start);
            let abs_lo = offset + start + lo;
            let abs_hi = offset + start + hi;
            if abs_hi <= self.range.0 || abs_lo >= self.range.1 {
                continue;
            }
            self.sequence(
                target,
                offset + start,
                (lo, hi),
                transpose + i32::from(tp),
                path,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        lane: &Lane,
        e: &Event,
        lt: Tick,
        offset: Tick,
        win_end: Tick,
        transpose: i32,
        path: &[PathStep],
    ) {
        let abs = offset + lt;
        if abs < self.range.0 || abs >= self.range.1 {
            return;
        }
        let mut body = e.body.clone();
        match &mut body {
            EventBody::Note {
                pitch, duration, ..
            } => {
                *pitch = shift_pitch(*pitch, transpose);
                *duration = (*duration).min(win_end - lt);
            }
            EventBody::PolyPressure { pitch, .. } => *pitch = shift_pitch(*pitch, transpose),
            _ => {}
        }
        let (dest, group_channel, audio_out) = if body.is_midi() {
            let r = resolve(self.doc, lane, e);
            (r.dest, r.group_channel, None)
        } else {
            (None, None, lane.audio_out)
        };
        self.out.push(ResolvedEvent {
            tick: abs,
            seconds: self.tempo.tick_to_seconds(abs),
            dest,
            group_channel,
            audio_out,
            body,
            path: path.to_vec(),
        });
    }
}

/// Flattens `seq` over its playing window `[-start_offset, length)`,
/// keeping events whose absolute tick is in `range` (all when `None`) and
/// visible through `filter`. Events are ordered by tick (stable).
pub fn flatten(
    doc: &Document,
    seq: SeqId,
    range: Option<(Tick, Tick)>,
    filter: &ViewFilter,
) -> Result<std::vec::IntoIter<ResolvedEvent>> {
    let s = doc.sequence(seq)?;
    let len = Lengths::default().of(doc, seq);
    let mut w = Walk::new(
        doc,
        &s.tempo,
        filter,
        range.unwrap_or((Tick::MIN, Tick::MAX)),
    );
    let mut path = Vec::new();
    w.sequence(s, Tick::ZERO, (-s.start_offset, len), 0, &mut path);
    let mut out = w.out;
    out.sort_by_key(|r| r.tick);
    Ok(out.into_iter())
}
```

Replace the whole of `crates/core-model/src/query/mod.rs` with:

```rust
//! Read-only queries over a [`crate::document::Document`].

pub mod flatten;
pub mod length;
pub mod routing;
pub mod usage;

pub use flatten::{PathStep, ResolvedEvent, flatten};
pub use length::{reference_span, seq_length};
pub use routing::{Routing, resolve, resolve_event};
pub use usage::{
    drum_view, media_in_use, unresolved_events, unused_sequences, use_count, use_counts, users_of,
};
```


- [ ] **Step 4: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test query_flatten
```

Expected: PASS — `query_flatten`: `test result: ok. 14 passed`.

- [ ] **Step 5: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/query/flatten.rs crates/core-model/src/query/mod.rs crates/core-model/tests/query_flatten.rs
git commit -m "feat(core-model): flatten query with nested timing"
```

---

### Task 12: Reference commands (insert, from selection, re-point, private copy, flatten, per-use params)

**Goal:** All reference commands from spec §5, with cycle refusal on insert and re-point; Make Reference from Selection reads the selection through `Context`.

**Files:**
- Modify: `crates/core-model/src/command/mod.rs`
- Create: `crates/core-model/src/command/reference.rs`
- Create (test): `crates/core-model/tests/cmd_reference.rs`

**Interfaces:**
- Consumes: `insert_specs`, `ensure_drum_row` (Task 8); `clone_sequence` (Task 6); `Walk` (Task 11); `seq_length` (Task 10); `rules::{check_reference, check_event_fields}` (Task 5).
- Produces:
  - `Command::Reference(ReferenceCmd)`; `ReferenceCmd::{Insert { seq, lane, tick, target, length, loop_count, transpose }, FromSelection { seq, per_lane, round_to_bars, loop_count }, RePoint { event, target }, MakePrivateCopy { event }, Flatten { event }, SetParams { event, length, loop_count, transpose }}`.
  - `apply_in` now passes `ctx` to `reference::apply(&mut d, ctx, c, &mut report)` (the placeholder `let _ = ctx;` line from Task 6 is removed).

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/cmd_reference.rs`:

```rust
use core_model::ModelError;
use core_model::command::{
    Command, Context, EventCmd, LaneCmd, ReferenceCmd, SequenceCmd, apply, apply_in,
};
use core_model::document::{DestKind, Destination, Document, DocumentRepr, Settings};
use core_model::event::{EventBody, EventSpec, LoopCount};
use core_model::ids::{AnyId, Counters, DestId, EventId, LaneId, SeqId};
use core_model::query::{flatten, seq_length, use_count, users_of};
use core_model::rules::{find_cycle, validate};
use core_model::selection::{Selection, ViewFilter};
use core_model::sequence::SeqLength;
use core_model::time::{PPQ, Tick};

const Q: i64 = PPQ;

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn rc(c: ReferenceCmd) -> Command {
    Command::Reference(c)
}

fn insert(d: &Document, seq: u64, lane: u64, specs: Vec<EventSpec>) -> Document {
    run(
        d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(seq),
            lane: LaneId(lane),
            events: specs,
        }),
    )
    .unwrap()
}

fn note(beat: i64, pitch: u8) -> EventSpec {
    EventSpec::new(Tick(beat * Q), EventBody::note(pitch, 100, Tick(Q)))
}

fn insert_ref(
    d: &Document,
    seq: u64,
    lane: u64,
    beat: i64,
    target: u64,
) -> core_model::Result<Document> {
    run(
        d,
        rc(ReferenceCmd::Insert {
            seq: SeqId(seq),
            lane: LaneId(lane),
            tick: Tick(beat * Q),
            target: SeqId(target),
            length: None,
            loop_count: LoopCount::Count(1),
            transpose: 0,
        }),
    )
}

/// dest0, dest1 (plugins). Sequences 0..3, each with one lane (lane i);
/// lane0 -> dest0, lane1 -> dest1. seq1 has notes 60 @0 and 62 @1.
fn base() -> Document {
    let dest = |i: u64| Destination {
        id: DestId(i),
        name: format!("D{i}"),
        kind: DestKind::PluginInstrument {
            plugin_ref: "p".into(),
        },
        default_patch: None,
    };
    let mut d = Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![dest(0), dest(1)],
        outputs: vec![],
        media: vec![],
        sequences: vec![],
        counters: Counters {
            dest: 2,
            ..Counters::default()
        },
    })
    .unwrap();
    for name in ["Song", "Verse", "Fill"] {
        d = run(
            &d,
            Command::Sequence(SequenceCmd::New { name: name.into() }),
        )
        .unwrap();
    }
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(DestId(0)),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(1),
            lane: LaneId(1),
            dest: Some(DestId(1)),
        }),
    )
    .unwrap();
    insert(&d, 1, 1, vec![note(0, 60), note(1, 62)])
}

/// Comparable view of a flattened sequence: (tick, dest, group/channel, body).
fn played(d: &Document, seq: u64) -> Vec<String> {
    let mut v: Vec<String> = flatten(d, SeqId(seq), None, &ViewFilter::default())
        .unwrap()
        .map(|r| {
            format!(
                "{:?}|{:?}|{:?}|{:?}",
                r.tick, r.dest, r.group_channel, r.body
            )
        })
        .collect();
    v.sort();
    v
}

// Review focus #1: re-pointing must not be able to create a cycle.
#[test]
fn re_point_refuses_cycles() {
    // seq3 "x" is a harmless target.
    let d = run(
        &base(),
        Command::Sequence(SequenceCmd::New { name: "x".into() }),
    )
    .unwrap();
    let d = insert_ref(&d, 0, 0, 0, 1).unwrap(); // ev2: song -> verse
    let d = insert_ref(&d, 1, 1, 4, 2).unwrap(); // ev3: verse -> fill
    let d = insert_ref(&d, 2, 2, 0, 3).unwrap(); // ev4: fill -> x
    assert_eq!(find_cycle(&d), None);
    // fill -> song would close song -> verse -> fill -> song.
    let err = run(
        &d,
        rc(ReferenceCmd::RePoint {
            event: EventId(4),
            target: SeqId(0),
        }),
    )
    .unwrap_err();
    assert_eq!(
        err,
        ModelError::Cycle {
            path: vec![SeqId(2), SeqId(0), SeqId(1), SeqId(2)]
        }
    );
    // fill -> fill is a self-cycle.
    let err = run(
        &d,
        rc(ReferenceCmd::RePoint {
            event: EventId(4),
            target: SeqId(2),
        }),
    )
    .unwrap_err();
    assert_eq!(
        err,
        ModelError::Cycle {
            path: vec![SeqId(2), SeqId(2)]
        }
    );
    // Inserting fill -> verse directly is refused the same way.
    assert!(matches!(
        insert_ref(&d, 2, 2, 0, 1),
        Err(ModelError::Cycle { .. })
    ));
    // A harmless re-point succeeds; re-pointing a note is invalid.
    let ok = run(
        &d,
        rc(ReferenceCmd::RePoint {
            event: EventId(2),
            target: SeqId(3),
        }),
    )
    .unwrap();
    assert_eq!(users_of(&ok, SeqId(3)), vec![EventId(2), EventId(4)]);
    assert!(matches!(
        run(
            &d,
            rc(ReferenceCmd::RePoint {
                event: EventId(0),
                target: SeqId(3)
            })
        ),
        Err(ModelError::Invalid { .. })
    ));
}

#[test]
fn private_copy_versus_flatten() {
    let d = insert_ref(&base(), 0, 0, 4, 1).unwrap(); // ev2
    let before = played(&d, 0);

    let a = apply(&d, &rc(ReferenceCmd::MakePrivateCopy { event: EventId(2) })).unwrap();
    assert_eq!(a.report.created, vec![AnyId::Seq(SeqId(3))]);
    assert_eq!(a.doc.sequence(SeqId(3)).unwrap().name, "Verse (copy)");
    assert_eq!(use_count(&a.doc, SeqId(1)), 0);
    assert_eq!(users_of(&a.doc, SeqId(3)), vec![EventId(2)]);
    assert_eq!(played(&a.doc, 0), before);
    assert!(validate(&a.doc).is_empty());

    let f = apply(&d, &rc(ReferenceCmd::Flatten { event: EventId(2) })).unwrap();
    assert_eq!(use_count(&f.doc, SeqId(1)), 0);
    assert!(f.doc.event(EventId(2)).is_err());
    assert_eq!(f.report.removed, vec![AnyId::Event(EventId(2))]);
    assert_eq!(played(&f.doc, 0), before);
    // Inlined notes carry the inner lane's destination explicitly.
    let (e, loc) = f.doc.event(EventId(3)).unwrap();
    assert_eq!(
        (e.tick, e.dest, loc.lane),
        (Tick(4 * Q), Some(DestId(1)), LaneId(0))
    );
    assert!(validate(&f.doc).is_empty());
}

#[test]
fn flatten_applies_length_loop_and_transpose() {
    let d = run(
        &base(),
        rc(ReferenceCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(0),
            target: SeqId(1),
            length: Some(Tick(Q)),
            loop_count: LoopCount::Count(3),
            transpose: 2,
        }),
    )
    .unwrap();
    let before = played(&d, 0);
    let f = run(&d, rc(ReferenceCmd::Flatten { event: EventId(2) })).unwrap();
    assert_eq!(played(&f, 0), before);
    let pitches: Vec<(Tick, EventBody)> = f
        .lane(SeqId(0), LaneId(0))
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .map(|e| (e.tick, e.body.clone()))
        .collect();
    assert_eq!(
        pitches,
        vec![
            (Tick(0), EventBody::note(62, 100, Tick(Q))),
            (Tick(Q), EventBody::note(62, 100, Tick(Q))),
            (Tick(2 * Q), EventBody::note(62, 100, Tick(Q))),
        ]
    );
}

#[test]
fn set_params_validates() {
    let d = insert_ref(&base(), 0, 0, 0, 1).unwrap();
    let d2 = run(
        &d,
        rc(ReferenceCmd::SetParams {
            event: EventId(2),
            length: Some(Tick(Q * 8)),
            loop_count: LoopCount::Fill,
            transpose: -5,
        }),
    )
    .unwrap();
    assert_eq!(seq_length(&d2, SeqId(0)).unwrap(), Tick(Q * 8));
    assert!(
        run(
            &d,
            rc(ReferenceCmd::SetParams {
                event: EventId(2),
                length: None,
                loop_count: LoopCount::Count(0),
                transpose: 0
            })
        )
        .is_err()
    );
    assert!(matches!(
        run(
            &d,
            rc(ReferenceCmd::SetParams {
                event: EventId(0),
                length: None,
                loop_count: LoopCount::Count(1),
                transpose: 0
            })
        ),
        Err(ModelError::Invalid { .. })
    ));
}

fn selection(range: (i64, i64), lanes: &[(u64, u64)]) -> Selection {
    Selection {
        time_range: Some((Tick(range.0), Tick(range.1))),
        lanes: lanes.iter().map(|(s, l)| (SeqId(*s), LaneId(*l))).collect(),
        events: Default::default(),
    }
}

/// Song with two lanes (lane0, lane3) holding notes at beats 1, 5 and 9.
fn two_lane_song() -> Document {
    let d = run(
        &base(),
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "B".into(),
            index: None,
        }),
    )
    .unwrap();
    let d = insert(&d, 0, 0, vec![note(1, 40), note(5, 41), note(9, 42)]);
    insert(&d, 0, 3, vec![note(1, 50), note(5, 51), note(9, 52)])
}

#[test]
fn make_reference_from_selection_multi_lane_rounded() {
    let d = two_lane_song();
    let before = played(&d, 0);
    let sel = selection((5 * Q, 6 * Q), &[(0, 0), (0, 3)]);
    let filter = ViewFilter::default();
    let a = apply_in(
        &d,
        Context {
            selection: &sel,
            filter: &filter,
        },
        &rc(ReferenceCmd::FromSelection {
            seq: SeqId(0),
            per_lane: false,
            round_to_bars: true,
            loop_count: LoopCount::Count(1),
        }),
    )
    .unwrap();
    let new = a.doc.sequence(SeqId(3)).unwrap();
    assert_eq!(new.lanes.len(), 2);
    assert_eq!(new.length, SeqLength::Locked(Tick(4 * Q)));
    assert_eq!(new.lanes[0].midi_dest, Some(DestId(0)));
    // The moved notes keep their ids and sit at beat 1 of the new sequence.
    let (e, loc) = a.doc.event(EventId(3)).unwrap();
    assert_eq!((e.tick, loc.seq), (Tick(Q), SeqId(3)));
    assert_eq!(users_of(&a.doc, SeqId(3)).len(), 1);
    assert_eq!(played(&a.doc, 0), before);
    assert!(validate(&a.doc).is_empty(), "{:?}", validate(&a.doc));
}

#[test]
fn make_reference_from_selection_per_lane_with_loop() {
    let d = two_lane_song();
    let sel = selection((4 * Q, 8 * Q), &[(0, 0), (0, 3)]);
    let filter = ViewFilter::default();
    let a = apply_in(
        &d,
        Context {
            selection: &sel,
            filter: &filter,
        },
        &rc(ReferenceCmd::FromSelection {
            seq: SeqId(0),
            per_lane: true,
            round_to_bars: false,
            loop_count: LoopCount::Count(2),
        }),
    )
    .unwrap();
    assert_eq!(a.doc.sequences().len(), 5);
    assert_eq!(users_of(&a.doc, SeqId(3)).len(), 1);
    assert_eq!(users_of(&a.doc, SeqId(4)).len(), 1);
    let refs = users_of(&a.doc, SeqId(4));
    let (r, loc) = a.doc.event(refs[0]).unwrap();
    assert_eq!(loc.lane, LaneId(3));
    assert!(matches!(
        r.body,
        EventBody::Reference {
            loop_count: LoopCount::Count(2),
            ..
        }
    ));
}

#[test]
fn make_reference_from_selection_needs_range_and_lanes() {
    let d = two_lane_song();
    let filter = ViewFilter::default();
    let cmd = rc(ReferenceCmd::FromSelection {
        seq: SeqId(0),
        per_lane: false,
        round_to_bars: false,
        loop_count: LoopCount::Count(1),
    });
    let empty = Selection::default();
    assert!(
        apply_in(
            &d,
            Context {
                selection: &empty,
                filter: &filter
            },
            &cmd
        )
        .is_err()
    );
    let no_lanes = selection((0, Q), &[]);
    assert!(
        apply_in(
            &d,
            Context {
                selection: &no_lanes,
                filter: &filter
            },
            &cmd
        )
        .is_err()
    );
}

#[test]
fn flattening_a_span_longer_than_its_events_keeps_the_length() {
    // Song references the empty "Fill" (one bar) twice: two bars long.
    let d = run(
        &base(),
        rc(ReferenceCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(0),
            target: SeqId(2),
            length: None,
            loop_count: LoopCount::Count(2),
            transpose: 0,
        }),
    )
    .unwrap();
    assert_eq!(seq_length(&d, SeqId(0)).unwrap(), Tick(8 * Q));
    let a = apply(&d, &rc(ReferenceCmd::Flatten { event: EventId(2) })).unwrap();
    assert_eq!(
        a.doc.sequence(SeqId(0)).unwrap().length,
        SeqLength::Locked(Tick(8 * Q))
    );
    assert_eq!(a.report.warnings.len(), 1);
}

#[test]
fn flattening_unresolved_events_warns() {
    // Verse's lane has a destination; Fill's does not.
    let d = insert(&base(), 2, 2, vec![note(0, 70)]);
    let d = insert_ref(&d, 0, 0, 0, 2).unwrap();
    let a = apply(&d, &rc(ReferenceCmd::Flatten { event: EventId(3) })).unwrap();
    assert!(
        a.report
            .warnings
            .iter()
            .any(|w| w.contains("no destination"))
    );
}
```


- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_reference
```

Expected: FAIL to compile — `error[E0432]: unresolved import `core_model::command::ReferenceCmd``.

- [ ] **Step 3: Implement**

Replace the whole of `crates/core-model/src/command/mod.rs` with:

```rust
//! Commands: every edit is a `Command` applied by the pure [`apply`].

pub mod drum;
pub mod event;
pub mod lane;
pub mod reference;
pub mod sequence;

pub use drum::DrumCmd;
pub use event::EventCmd;
pub use lane::LaneCmd;
pub use reference::ReferenceCmd;
pub use sequence::SequenceCmd;

use crate::document::Document;
use crate::error::Result;
use crate::ids::AnyId;
use crate::selection::{Selection, ViewFilter};

/// What a command did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// IDs created, in creation order.
    pub created: Vec<AnyId>,
    /// IDs removed.
    pub removed: Vec<AnyId>,
    /// Non-fatal notes for the user.
    pub warnings: Vec<String>,
}

/// The result of applying a command.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// The new document.
    pub doc: Document,
    /// What changed.
    pub report: Report,
    /// The new selection, for commands that change it.
    pub selection: Option<Selection>,
}

/// The current selection and view filter, for commands that act on them.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The document's selection.
    pub selection: &'a Selection,
    /// The view filter of the view issuing the command.
    pub filter: &'a ViewFilter,
}

/// Every edit the model supports.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Sequence-level edits.
    Sequence(SequenceCmd),
    /// Lane and take edits.
    Lane(LaneCmd),
    /// Event edits.
    Event(EventCmd),
    /// Drum-lane edits.
    Drum(DrumCmd),
    /// Reference edits.
    Reference(ReferenceCmd),
}

impl Command {
    /// The stable name, used for key bindings and undo labels.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Sequence(c) => c.name(),
            Command::Lane(c) => c.name(),
            Command::Event(c) => c.name(),
            Command::Drum(c) => c.name(),
            Command::Reference(c) => c.name(),
        }
    }
}

/// Applies a command with an empty selection and no view filter.
pub fn apply(doc: &Document, cmd: &Command) -> Result<Applied> {
    let selection = Selection::default();
    let filter = ViewFilter::default();
    apply_in(
        doc,
        Context {
            selection: &selection,
            filter: &filter,
        },
        cmd,
    )
}

/// Applies a command in the context of a selection and view filter. Pure:
/// `doc` is never changed; on error nothing is returned but the error.
pub fn apply_in(doc: &Document, ctx: Context<'_>, cmd: &Command) -> Result<Applied> {
    let mut d = doc.clone();
    let mut report = Report::default();
    let selection = match cmd {
        Command::Sequence(c) => {
            sequence::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Lane(c) => {
            lane::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Event(c) => {
            event::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Drum(c) => {
            drum::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Reference(c) => {
            reference::apply(&mut d, ctx, c, &mut report)?;
            None
        }
    };
    Ok(Applied {
        doc: d,
        report,
        selection,
    })
}
```

Create `crates/core-model/src/command/reference.rs`:

```rust
//! Reference commands: insert, make from selection, re-point, private copy,
//! flatten and per-use parameters.

use super::event::{ensure_drum_row, insert_specs};
use super::sequence::clone_sequence;
use super::{Context, Report};
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::event::{EventBody, EventSpec, LoopCount};
use crate::ids::{AnyId, EventId, LaneId, SeqId};
use crate::lane::Lane;
use crate::query::flatten::Walk;
use crate::query::seq_length;
use crate::rules::{check_event_fields, check_reference};
use crate::selection::ViewFilter;
use crate::sequence::{SeqLength, Sequence};
use crate::time::Tick;
use im::Vector;

/// Reference edits.
#[derive(Debug, Clone, PartialEq)]
pub enum ReferenceCmd {
    /// Insert a reference to `target` into a lane's active take.
    Insert {
        /// Owning sequence.
        seq: SeqId,
        /// Owning lane.
        lane: LaneId,
        /// Position.
        tick: Tick,
        /// Referenced sequence.
        target: SeqId,
        /// Per-use length.
        length: Option<Tick>,
        /// Repeats.
        loop_count: LoopCount,
        /// Transpose in semitones.
        transpose: i8,
    },
    /// Move the selected lanes' active-take events inside the selection's
    /// time range into new sequence(s) and put reference(s) in their place.
    FromSelection {
        /// The sequence the selection is in.
        seq: SeqId,
        /// One reference per lane instead of one multi-lane reference.
        per_lane: bool,
        /// Widen the range to barlines.
        round_to_bars: bool,
        /// Loop count of the new reference(s).
        loop_count: LoopCount,
    },
    /// Point a reference at another sequence.
    RePoint {
        /// The reference event.
        event: EventId,
        /// New target.
        target: SeqId,
    },
    /// Clone the referenced sequence and point this use at the clone.
    MakePrivateCopy {
        /// The reference event.
        event: EventId,
    },
    /// Replace the reference with its events, inlined in place (length,
    /// loop, transpose, mute and nested references applied; destinations made
    /// explicit).
    Flatten {
        /// The reference event.
        event: EventId,
    },
    /// Set per-use length, loop count and transpose.
    SetParams {
        /// The reference event.
        event: EventId,
        /// Per-use length.
        length: Option<Tick>,
        /// Repeats.
        loop_count: LoopCount,
        /// Transpose.
        transpose: i8,
    },
}

impl ReferenceCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            ReferenceCmd::Insert { .. } => "Insert Reference",
            ReferenceCmd::FromSelection { .. } => "Make Reference from Selection",
            ReferenceCmd::RePoint { .. } => "Re-point Reference",
            ReferenceCmd::MakePrivateCopy { .. } => "Make Private Copy",
            ReferenceCmd::Flatten { .. } => "Flatten Reference",
            ReferenceCmd::SetParams { .. } => "Set Reference Parameters",
        }
    }
}

/// The target of a reference event, or `Invalid` if it is not one.
fn target_of(d: &Document, event: EventId) -> Result<SeqId> {
    match d.event(event)?.0.body {
        EventBody::Reference { seq, .. } => Ok(seq),
        _ => Err(ModelError::invalid(format!("{event} is not a reference"))),
    }
}

fn set_target(d: &mut Document, event: EventId, target: SeqId) -> Result<()> {
    let (ev, loc) = d.event(event)?;
    check_reference(d, loc.seq, target)?;
    let mut ev = ev.clone();
    if let EventBody::Reference { seq, .. } = &mut ev.body {
        *seq = target;
    }
    d.replace_event(ev)?;
    Ok(())
}

fn flatten_ref(d: &mut Document, event: EventId, r: &mut Report) -> Result<()> {
    target_of(d, event)?;
    let (ev, loc) = d.event(event)?;
    let ev = ev.clone();
    let owner = d.sequence(loc.seq)?.clone();
    let filter = ViewFilter::default();
    let resolved = {
        let mut w = Walk::new(d, &owner.tempo, &filter, (Tick::MIN, Tick::MAX));
        let mut path = Vec::new();
        w.reference(
            &ev,
            ev.tick,
            Tick::ZERO,
            (Tick::MIN, Tick::MAX),
            0,
            &mut path,
        );
        w.out
    };
    let mut specs = Vec::with_capacity(resolved.len());
    for x in resolved {
        let source_gc = x
            .path
            .last()
            .and_then(|p| d.event(p.event).ok())
            .and_then(|(e, _)| e.group_channel);
        specs.push(EventSpec {
            tick: x.tick,
            dest: x.dest,
            group_channel: source_gc,
            mute: false,
            body: x.body,
        });
    }
    let unresolved = specs
        .iter()
        .filter(|s| s.body.is_midi() && s.dest.is_none())
        .count();
    if unresolved > 0 {
        r.warnings.push(format!(
            "{unresolved} flattened events had no destination and now follow this lane's"
        ));
    }
    let old_len = seq_length(d, loc.seq)?;
    d.remove_event(event)?;
    r.removed.push(AnyId::Event(event));
    for spec in specs {
        check_event_fields(d, &spec)?;
        let id = d.counters_mut().alloc_event();
        d.insert_event(loc.seq, loc.lane, Some(loc.take), spec.with_id(id))?;
        r.created.push(AnyId::Event(id));
        ensure_drum_row(d, id, r)?;
    }
    // The reference's span may have been longer than its events; keep the
    // owner's playing length so playback is unchanged.
    if owner.length == SeqLength::Derived && seq_length(d, loc.seq)? < old_len {
        d.seq_mut(loc.seq)?.length = SeqLength::Locked(old_len);
        r.warnings
            .push("sequence length locked to keep the flattened span".to_string());
    }
    Ok(())
}

fn from_selection(
    d: &mut Document,
    ctx: Context<'_>,
    seq: SeqId,
    per_lane: bool,
    round_to_bars: bool,
    loop_count: LoopCount,
    r: &mut Report,
) -> Result<()> {
    let parent = d.sequence(seq)?.clone();
    let (mut a, mut b) = ctx
        .selection
        .time_range
        .ok_or_else(|| ModelError::invalid("selection has no time range"))?;
    if round_to_bars {
        a = parent.meter.barline_at_or_before(a);
        b = parent.meter.barline_at_or_after(b);
    }
    if b <= a {
        return Err(ModelError::range(
            "selection.time_range",
            format!("{}..{}", a.0, b.0),
        ));
    }
    let lanes: Vec<Lane> = parent
        .lanes
        .iter()
        .filter(|l| ctx.selection.lanes.contains(&(seq, l.id)))
        .cloned()
        .collect();
    if lanes.is_empty() {
        return Err(ModelError::invalid(
            "no lanes of that sequence are selected",
        ));
    }
    let groups: Vec<Vec<Lane>> = if per_lane {
        lanes.into_iter().map(|l| vec![l]).collect()
    } else {
        vec![lanes]
    };
    for group in groups {
        let c = d.counters_mut();
        let new_seq = c.alloc_seq();
        let mut new_lanes = Vector::new();
        let mut moves = Vec::new();
        for l in &group {
            let (nl_id, take) = (c.alloc_lane(), c.alloc_take());
            let mut nl = Lane::new(nl_id, take, l.name.clone());
            nl.midi_dest = l.midi_dest;
            nl.audio_out = l.audio_out;
            new_lanes.push_back(nl);
            if let Some(t) = l.active() {
                for e in t.events.values().filter(|e| e.tick >= a && e.tick < b) {
                    moves.push((e.id, nl_id));
                }
            }
        }
        let mut s = Sequence::new(
            new_seq,
            format!("{} part {}", parent.name, new_seq.0),
            new_lanes,
        );
        s.tempo = parent.tempo.clone();
        s.meter = parent.meter.clone();
        s.length = SeqLength::Locked(b - a);
        d.put_sequence(s);
        r.created.push(AnyId::Seq(new_seq));
        for (id, nl) in moves {
            let (mut ev, _) = d.remove_event(id)?;
            ev.tick = ev.tick - a;
            d.insert_event(new_seq, nl, None, ev)?;
        }
        let first = group
            .first()
            .map(|l| l.id)
            .ok_or_else(|| ModelError::invalid("empty lane group"))?;
        let spec = EventSpec::new(
            a,
            EventBody::Reference {
                seq: new_seq,
                length: None,
                loop_count,
                transpose: 0,
            },
        );
        insert_specs(d, seq, first, &[spec], r)?;
    }
    Ok(())
}

pub(crate) fn apply(
    d: &mut Document,
    ctx: Context<'_>,
    cmd: &ReferenceCmd,
    r: &mut Report,
) -> Result<()> {
    match cmd {
        ReferenceCmd::Insert {
            seq,
            lane,
            tick,
            target,
            length,
            loop_count,
            transpose,
        } => {
            let spec = EventSpec::new(
                *tick,
                EventBody::Reference {
                    seq: *target,
                    length: *length,
                    loop_count: *loop_count,
                    transpose: *transpose,
                },
            );
            insert_specs(d, *seq, *lane, &[spec], r)?;
        }
        ReferenceCmd::FromSelection {
            seq,
            per_lane,
            round_to_bars,
            loop_count,
        } => from_selection(d, ctx, *seq, *per_lane, *round_to_bars, *loop_count, r)?,
        ReferenceCmd::RePoint { event, target } => {
            target_of(d, *event)?;
            set_target(d, *event, *target)?;
        }
        ReferenceCmd::MakePrivateCopy { event } => {
            let target = target_of(d, *event)?;
            let name = format!("{} (copy)", d.sequence(target)?.name);
            let copy = clone_sequence(d, target, name)?;
            r.created.push(AnyId::Seq(copy));
            set_target(d, *event, copy)?;
        }
        ReferenceCmd::Flatten { event } => flatten_ref(d, *event, r)?,
        ReferenceCmd::SetParams {
            event,
            length,
            loop_count,
            transpose,
        } => {
            target_of(d, *event)?;
            let mut ev = d.event(*event)?.0.clone();
            if let EventBody::Reference {
                length: l,
                loop_count: lc,
                transpose: t,
                ..
            } = &mut ev.body
            {
                *l = *length;
                *lc = *loop_count;
                *t = *transpose;
            }
            check_event_fields(d, &ev.spec())?;
            d.replace_event(ev)?;
        }
    }
    Ok(())
}
```


- [ ] **Step 4: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_reference
```

Expected: PASS — `cmd_reference`: `test result: ok. 9 passed`.

- [ ] **Step 5: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/command/mod.rs crates/core-model/src/command/reference.rs crates/core-model/tests/cmd_reference.rs
git commit -m "feat(core-model): reference commands"
```

---

### Task 13: Audio-region, destination, output and media commands

**Goal:** Insert/trim/split audio regions and set gain and fades; add/rename/edit/remove destinations and outputs (remove needs a replacement when in use); add/remove media.

**Files:**
- Create: `crates/core-model/src/command/audio.rs`
- Modify: `crates/core-model/src/command/mod.rs`
- Create: `crates/core-model/src/command/routing.rs`
- Create (test): `crates/core-model/tests/cmd_audio_routing.rs`

**Interfaces:**
- Consumes: Task 6 plumbing; `insert_specs` (Task 8); `rules::{check_event_fields, check_group_channel}` (Task 5).
- Produces:
  - `Command::Audio(AudioCmd)`; `AudioCmd::{Insert { seq, lane, tick, media, source_offset, length, gain_db, fade_in, fade_out }, Trim { event, front_frames: i64, back_frames: i64 }, Split { event, at }, SetGain { event, gain_db }, SetFades { event, fade_in, fade_out }}`.
  - `Command::Routing(RoutingCmd)`; `RoutingCmd::{AddDestination { name, kind, default_patch }, RenameDestination { dest, name }, EditDestination { dest, kind, default_patch }, RemoveDestination { dest, replacement }, AddOutput { name, reference }, RenameOutput { output, name }, EditOutput { output, reference }, RemoveOutput { output, replacement }, AddMedia { path, sample_rate, channels, length_frames, content_hash }, RemoveMedia { media, replacement }}`.

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/cmd_audio_routing.rs`:

```rust
use core_model::ModelError;
use core_model::command::{AudioCmd, Command, EventCmd, LaneCmd, RoutingCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Document, Patch};
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::ids::{AnyId, DestId, EventId, LaneId, MediaId, OutputPathId, SeqId};
use core_model::query::resolve_event;
use core_model::rules::validate;
use core_model::time::{PPQ, Tick};

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn routing(c: RoutingCmd) -> Command {
    Command::Routing(c)
}

fn audio(c: AudioCmd) -> Command {
    Command::Audio(c)
}

fn endpoint(group: u8, channel: u8) -> DestKind {
    DestKind::MidiEndpoint {
        endpoint_ref: "ep".into(),
        group,
        channel,
    }
}

/// seq0/lane0; dest0 (g1 c2), dest1 (plugin); out0, out1; media0 and media1
/// (96 000 frames at 48 kHz = 2 s = 4 beats at 120 BPM).
fn base() -> Document {
    let mut d = run(
        &Document::new(),
        Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap();
    for c in [
        RoutingCmd::AddDestination {
            name: "Ext".into(),
            kind: endpoint(1, 2),
            default_patch: None,
        },
        RoutingCmd::AddDestination {
            name: "Plug".into(),
            kind: DestKind::PluginInstrument {
                plugin_ref: "p".into(),
            },
            default_patch: Some(Patch {
                program: 3,
                bank: Some((0, 1)),
            }),
        },
        RoutingCmd::AddOutput {
            name: "Main".into(),
            reference: "bus:0".into(),
        },
        RoutingCmd::AddOutput {
            name: "Aux".into(),
            reference: "bus:1".into(),
        },
        RoutingCmd::AddMedia {
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 96_000,
            content_hash: None,
        },
        RoutingCmd::AddMedia {
            path: "Media/b.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 96_000,
            content_hash: None,
        },
    ] {
        d = run(&d, routing(c)).unwrap();
    }
    d
}

fn with_region(d: &Document) -> Document {
    run(
        d,
        audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(PPQ),
            media: MediaId(0),
            source_offset: 0,
            length: 96_000,
            gain_db: 0.0,
            fade_in: Fade {
                length: Tick(10),
                shape: FadeShape::Linear,
            },
            fade_out: Fade {
                length: Tick(20),
                shape: FadeShape::SCurve,
            },
        }),
    )
    .unwrap()
}

fn region(d: &Document, id: u64) -> (Tick, u64, u64, Fade, Fade) {
    let (e, _) = d.event(EventId(id)).unwrap();
    match e.body {
        EventBody::AudioRegion {
            source_offset,
            length,
            fade_in,
            fade_out,
            ..
        } => (e.tick, source_offset, length, fade_in, fade_out),
        _ => panic!("not a region"),
    }
}

#[test]
fn add_items_allocate_ids_and_validate() {
    let d = base();
    assert_eq!(d.instruments().len(), 2);
    assert_eq!(d.outputs().len(), 2);
    assert_eq!(d.media().len(), 2);
    assert!(
        run(
            &d,
            routing(RoutingCmd::AddDestination {
                name: "x".into(),
                kind: endpoint(16, 0),
                default_patch: None
            })
        )
        .is_err()
    );
    assert!(
        run(
            &d,
            routing(RoutingCmd::AddMedia {
                path: "x".into(),
                sample_rate: 0,
                channels: 1,
                length_frames: 1,
                content_hash: None
            })
        )
        .is_err()
    );
    assert!(validate(&d).is_empty());
}

#[test]
fn editing_a_destination_reroutes_its_users() {
    let d = run(
        &base(),
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(DestId(0)),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![EventSpec::new(Tick(0), EventBody::note(60, 1, Tick(1)))],
        }),
    )
    .unwrap();
    assert_eq!(
        resolve_event(&d, EventId(0)).unwrap().group_channel,
        Some((1, 2))
    );
    let d = run(
        &d,
        routing(RoutingCmd::EditDestination {
            dest: DestId(0),
            kind: endpoint(3, 4),
            default_patch: None,
        }),
    )
    .unwrap();
    assert_eq!(
        resolve_event(&d, EventId(0)).unwrap().group_channel,
        Some((3, 4))
    );
    let d = run(
        &d,
        routing(RoutingCmd::RenameDestination {
            dest: DestId(0),
            name: "New".into(),
        }),
    )
    .unwrap();
    assert_eq!(d.instruments().get(&DestId(0)).unwrap().name, "New");
}

#[test]
fn removing_a_destination_in_use_needs_a_replacement() {
    let d = run(
        &base(),
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(DestId(0)),
        }),
    )
    .unwrap();
    let mut spec = EventSpec::new(Tick(0), EventBody::note(60, 1, Tick(1)));
    spec.dest = Some(DestId(0));
    let d = run(
        &d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![spec],
        }),
    )
    .unwrap();
    assert_eq!(
        run(
            &d,
            routing(RoutingCmd::RemoveDestination {
                dest: DestId(0),
                replacement: None
            })
        ),
        Err(ModelError::InUse {
            users: vec![AnyId::Lane(LaneId(0)), AnyId::Event(EventId(0))]
        })
    );
    assert!(
        run(
            &d,
            routing(RoutingCmd::RemoveDestination {
                dest: DestId(0),
                replacement: Some(DestId(0))
            })
        )
        .is_err()
    );
    assert!(
        run(
            &d,
            routing(RoutingCmd::RemoveDestination {
                dest: DestId(0),
                replacement: Some(DestId(9))
            })
        )
        .is_err()
    );
    let d = run(
        &d,
        routing(RoutingCmd::RemoveDestination {
            dest: DestId(0),
            replacement: Some(DestId(1)),
        }),
    )
    .unwrap();
    assert_eq!(
        d.lane(SeqId(0), LaneId(0)).unwrap().midi_dest,
        Some(DestId(1))
    );
    assert_eq!(d.event(EventId(0)).unwrap().0.dest, Some(DestId(1)));
    assert!(validate(&d).is_empty());
    // Unused destinations go without a replacement.
    assert!(
        run(
            &base(),
            routing(RoutingCmd::RemoveDestination {
                dest: DestId(1),
                replacement: None
            })
        )
        .is_ok()
    );
}

#[test]
fn removing_outputs_and_media_in_use() {
    let d = run(
        &base(),
        Command::Lane(LaneCmd::SetOutput {
            seq: SeqId(0),
            lane: LaneId(0),
            output: Some(OutputPathId(0)),
        }),
    )
    .unwrap();
    assert!(matches!(
        run(
            &d,
            routing(RoutingCmd::RemoveOutput {
                output: OutputPathId(0),
                replacement: None
            })
        ),
        Err(ModelError::InUse { .. })
    ));
    let d2 = run(
        &d,
        routing(RoutingCmd::RemoveOutput {
            output: OutputPathId(0),
            replacement: Some(OutputPathId(1)),
        }),
    )
    .unwrap();
    assert_eq!(
        d2.lane(SeqId(0), LaneId(0)).unwrap().audio_out,
        Some(OutputPathId(1))
    );
    let d = with_region(&d);
    assert_eq!(
        run(
            &d,
            routing(RoutingCmd::RemoveMedia {
                media: MediaId(0),
                replacement: None
            })
        ),
        Err(ModelError::InUse {
            users: vec![AnyId::Event(EventId(0))]
        })
    );
    let d = run(
        &d,
        routing(RoutingCmd::RemoveMedia {
            media: MediaId(0),
            replacement: Some(MediaId(1)),
        }),
    )
    .unwrap();
    assert!(matches!(
        d.event(EventId(0)).unwrap().0.body,
        EventBody::AudioRegion {
            media: MediaId(1),
            ..
        }
    ));
    assert!(validate(&d).is_empty());
}

#[test]
fn insert_region_checks_media_bounds() {
    let bad = audio(AudioCmd::Insert {
        seq: SeqId(0),
        lane: LaneId(0),
        tick: Tick(0),
        media: MediaId(0),
        source_offset: 1,
        length: 96_000,
        gain_db: 0.0,
        fade_in: Fade::default(),
        fade_out: Fade::default(),
    });
    assert!(matches!(
        run(&base(), bad),
        Err(ModelError::OutOfRange {
            field: "region.source_end",
            ..
        })
    ));
}

#[test]
fn trim_moves_start_and_offset_together() {
    let d = with_region(&base());
    // 24 000 frames = 0.5 s = 1 beat at 120 BPM.
    let d = run(
        &d,
        audio(AudioCmd::Trim {
            event: EventId(0),
            front_frames: 24_000,
            back_frames: 24_000,
        }),
    )
    .unwrap();
    let (tick, off, len, _, _) = region(&d, 0);
    assert_eq!((tick, off, len), (Tick(2 * PPQ), 24_000, 48_000));
    assert!(
        run(
            &d,
            audio(AudioCmd::Trim {
                event: EventId(0),
                front_frames: -30_000,
                back_frames: 0
            })
        )
        .is_err()
    );
    assert!(
        run(
            &d,
            audio(AudioCmd::Trim {
                event: EventId(0),
                front_frames: 0,
                back_frames: 48_000
            })
        )
        .is_err()
    );
}

#[test]
fn split_divides_frames_and_fades() {
    let d = with_region(&base());
    let a = apply(
        &d,
        &audio(AudioCmd::Split {
            event: EventId(0),
            at: Tick(2 * PPQ),
        }),
    )
    .unwrap();
    assert_eq!(a.report.created, vec![AnyId::Event(EventId(1))]);
    let (t0, o0, l0, fi0, fo0) = region(&a.doc, 0);
    let (t1, o1, l1, fi1, fo1) = region(&a.doc, 1);
    assert_eq!((t0, o0, l0), (Tick(PPQ), 0, 24_000));
    assert_eq!((t1, o1, l1), (Tick(2 * PPQ), 24_000, 72_000));
    assert_eq!(
        (fi0.length, fo0, fi1, fo1.length),
        (Tick(10), Fade::default(), Fade::default(), Tick(20))
    );
    assert!(
        run(
            &d,
            audio(AudioCmd::Split {
                event: EventId(0),
                at: Tick(PPQ)
            })
        )
        .is_err()
    );
    assert!(
        run(
            &d,
            audio(AudioCmd::Split {
                event: EventId(0),
                at: Tick(9 * PPQ)
            })
        )
        .is_err()
    );
}

#[test]
fn gain_and_fades() {
    let d = with_region(&base());
    let d = run(
        &d,
        audio(AudioCmd::SetGain {
            event: EventId(0),
            gain_db: -12.5,
        }),
    )
    .unwrap();
    assert!(
        matches!(d.event(EventId(0)).unwrap().0.body, EventBody::AudioRegion { gain_db, .. } if gain_db == -12.5)
    );
    assert!(
        run(
            &d,
            audio(AudioCmd::SetGain {
                event: EventId(0),
                gain_db: f32::INFINITY
            })
        )
        .is_err()
    );
    let f = Fade {
        length: Tick(99),
        shape: FadeShape::Exponential,
    };
    let d = run(
        &d,
        audio(AudioCmd::SetFades {
            event: EventId(0),
            fade_in: f,
            fade_out: f,
        }),
    )
    .unwrap();
    assert_eq!(region(&d, 0).3, f);
    assert!(
        run(
            &d,
            audio(AudioCmd::SetFades {
                event: EventId(0),
                fade_in: Fade {
                    length: Tick(-1),
                    shape: FadeShape::Linear
                },
                fade_out: f
            })
        )
        .is_err()
    );
}
```


- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_audio_routing
```

Expected: FAIL to compile — `error[E0432]: unresolved imports `core_model::command::AudioCmd`, `core_model::command::RoutingCmd``.

- [ ] **Step 3: Implement**

Create `crates/core-model/src/command/audio.rs`:

```rust
//! Audio-region commands. Frame/tick conversion uses the owning sequence's
//! tempo map and the media item's sample rate.

use super::Report;
use super::event::insert_specs;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::event::{Event, EventBody, EventSpec, Fade};
use crate::ids::{AnyId, EventId, LaneId, MediaId, SeqId};
use crate::rules::check_event_fields;
use crate::time::Tick;

/// Audio-region edits.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioCmd {
    /// Insert a region into a lane's active take.
    Insert {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Start tick.
        tick: Tick,
        /// Media item.
        media: MediaId,
        /// Start within the media (frames).
        source_offset: u64,
        /// Length (frames, >= 1).
        length: u64,
        /// Gain in dB.
        gain_db: f32,
        /// Fade in.
        fade_in: Fade,
        /// Fade out.
        fade_out: Fade,
    },
    /// Trim frames from the front and back (negative values extend).
    Trim {
        /// The region.
        event: EventId,
        /// Frames removed from the start (the region start moves later).
        front_frames: i64,
        /// Frames removed from the end.
        back_frames: i64,
    },
    /// Split a region in two at `at` (strictly inside it). The second part
    /// gets a new ID, reported as created.
    Split {
        /// The region.
        event: EventId,
        /// Split position.
        at: Tick,
    },
    /// Set gain.
    SetGain {
        /// The region.
        event: EventId,
        /// Gain in dB.
        gain_db: f32,
    },
    /// Set fades.
    SetFades {
        /// The region.
        event: EventId,
        /// Fade in.
        fade_in: Fade,
        /// Fade out.
        fade_out: Fade,
    },
}

impl AudioCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            AudioCmd::Insert { .. } => "Insert Audio Region",
            AudioCmd::Trim { .. } => "Trim Region",
            AudioCmd::Split { .. } => "Split Region",
            AudioCmd::SetGain { .. } => "Set Region Gain",
            AudioCmd::SetFades { .. } => "Set Region Fades",
        }
    }
}

/// The region event, its owning sequence and the media sample rate.
fn region(d: &Document, id: EventId) -> Result<(Event, SeqId, f64)> {
    let (ev, loc) = d.event(id)?;
    let EventBody::AudioRegion { media, .. } = ev.body else {
        return Err(ModelError::invalid(format!("{id} is not an audio region")));
    };
    let rate = d
        .media()
        .get(&media)
        .map(|m| f64::from(m.sample_rate.max(1)))
        .ok_or(ModelError::UnknownId(AnyId::Media(media)))?;
    Ok((ev.clone(), loc.seq, rate))
}

fn store(d: &mut Document, ev: Event) -> Result<()> {
    check_event_fields(d, &ev.spec())?;
    d.replace_event(ev)?;
    Ok(())
}

pub(crate) fn apply(d: &mut Document, cmd: &AudioCmd, r: &mut Report) -> Result<()> {
    match cmd {
        AudioCmd::Insert {
            seq,
            lane,
            tick,
            media,
            source_offset,
            length,
            gain_db,
            fade_in,
            fade_out,
        } => {
            let spec = EventSpec::new(
                *tick,
                EventBody::AudioRegion {
                    media: *media,
                    source_offset: *source_offset,
                    length: *length,
                    gain_db: *gain_db,
                    fade_in: *fade_in,
                    fade_out: *fade_out,
                },
            );
            insert_specs(d, *seq, *lane, &[spec], r)?;
        }
        AudioCmd::Trim {
            event,
            front_frames,
            back_frames,
        } => {
            let (mut ev, seq, rate) = region(d, *event)?;
            let tempo = d.sequence(seq)?.tempo.clone();
            if let EventBody::AudioRegion {
                source_offset,
                length,
                ..
            } = &mut ev.body
            {
                let new_offset = i128::from(*source_offset) + i128::from(*front_frames);
                let new_len =
                    i128::from(*length) - i128::from(*front_frames) - i128::from(*back_frames);
                *source_offset = u64::try_from(new_offset)
                    .map_err(|_| ModelError::range("region.source_offset", new_offset))?;
                *length = u64::try_from(new_len)
                    .map_err(|_| ModelError::range("region.length", new_len))?;
                let secs = tempo.tick_to_seconds(ev.tick) + *front_frames as f64 / rate;
                ev.tick = tempo.seconds_to_tick(secs);
            }
            store(d, ev)?;
        }
        AudioCmd::Split { event, at } => {
            let (ev, seq, rate) = region(d, *event)?;
            let tempo = d.sequence(seq)?.tempo.clone();
            let EventBody::AudioRegion {
                media,
                source_offset,
                length,
                gain_db,
                fade_in,
                fade_out,
            } = ev.body.clone()
            else {
                return Ok(());
            };
            let secs = tempo.tick_to_seconds(*at) - tempo.tick_to_seconds(ev.tick);
            let first = (secs * rate).round();
            if *at <= ev.tick || first < 1.0 || first >= length as f64 {
                return Err(ModelError::range("split.at", at.0));
            }
            let first = first as u64;
            let mut a = ev.clone();
            a.body = EventBody::AudioRegion {
                media,
                source_offset,
                length: first,
                gain_db,
                fade_in,
                fade_out: Fade::default(),
            };
            store(d, a)?;
            let id = d.counters_mut().alloc_event();
            let b = Event {
                id,
                tick: *at,
                dest: ev.dest,
                group_channel: ev.group_channel,
                mute: ev.mute,
                body: EventBody::AudioRegion {
                    media,
                    source_offset: source_offset + first,
                    length: length - first,
                    gain_db,
                    fade_in: Fade::default(),
                    fade_out,
                },
            };
            check_event_fields(d, &b.spec())?;
            let loc = d.locate(*event)?;
            d.insert_event(loc.seq, loc.lane, Some(loc.take), b)?;
            r.created.push(AnyId::Event(id));
        }
        AudioCmd::SetGain { event, gain_db } => {
            let (mut ev, _, _) = region(d, *event)?;
            if let EventBody::AudioRegion { gain_db: g, .. } = &mut ev.body {
                *g = *gain_db;
            }
            store(d, ev)?;
        }
        AudioCmd::SetFades {
            event,
            fade_in,
            fade_out,
        } => {
            let (mut ev, _, _) = region(d, *event)?;
            if let EventBody::AudioRegion {
                fade_in: fi,
                fade_out: fo,
                ..
            } = &mut ev.body
            {
                *fi = *fade_in;
                *fo = *fade_out;
            }
            store(d, ev)?;
        }
    }
    Ok(())
}
```

Replace the whole of `crates/core-model/src/command/mod.rs` with:

```rust
//! Commands: every edit is a `Command` applied by the pure [`apply`].

pub mod audio;
pub mod drum;
pub mod event;
pub mod lane;
pub mod reference;
pub mod routing;
pub mod sequence;

pub use audio::AudioCmd;
pub use drum::DrumCmd;
pub use event::EventCmd;
pub use lane::LaneCmd;
pub use reference::ReferenceCmd;
pub use routing::RoutingCmd;
pub use sequence::SequenceCmd;

use crate::document::Document;
use crate::error::Result;
use crate::ids::AnyId;
use crate::selection::{Selection, ViewFilter};

/// What a command did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// IDs created, in creation order.
    pub created: Vec<AnyId>,
    /// IDs removed.
    pub removed: Vec<AnyId>,
    /// Non-fatal notes for the user.
    pub warnings: Vec<String>,
}

/// The result of applying a command.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// The new document.
    pub doc: Document,
    /// What changed.
    pub report: Report,
    /// The new selection, for commands that change it.
    pub selection: Option<Selection>,
}

/// The current selection and view filter, for commands that act on them.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The document's selection.
    pub selection: &'a Selection,
    /// The view filter of the view issuing the command.
    pub filter: &'a ViewFilter,
}

/// Every edit the model supports.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Sequence-level edits.
    Sequence(SequenceCmd),
    /// Lane and take edits.
    Lane(LaneCmd),
    /// Event edits.
    Event(EventCmd),
    /// Drum-lane edits.
    Drum(DrumCmd),
    /// Reference edits.
    Reference(ReferenceCmd),
    /// Audio-region edits.
    Audio(AudioCmd),
    /// Destination, output and media edits.
    Routing(RoutingCmd),
}

impl Command {
    /// The stable name, used for key bindings and undo labels.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Sequence(c) => c.name(),
            Command::Lane(c) => c.name(),
            Command::Event(c) => c.name(),
            Command::Drum(c) => c.name(),
            Command::Reference(c) => c.name(),
            Command::Audio(c) => c.name(),
            Command::Routing(c) => c.name(),
        }
    }
}

/// Applies a command with an empty selection and no view filter.
pub fn apply(doc: &Document, cmd: &Command) -> Result<Applied> {
    let selection = Selection::default();
    let filter = ViewFilter::default();
    apply_in(
        doc,
        Context {
            selection: &selection,
            filter: &filter,
        },
        cmd,
    )
}

/// Applies a command in the context of a selection and view filter. Pure:
/// `doc` is never changed; on error nothing is returned but the error.
pub fn apply_in(doc: &Document, ctx: Context<'_>, cmd: &Command) -> Result<Applied> {
    let mut d = doc.clone();
    let mut report = Report::default();
    let selection = match cmd {
        Command::Sequence(c) => {
            sequence::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Lane(c) => {
            lane::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Event(c) => {
            event::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Drum(c) => {
            drum::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Reference(c) => {
            reference::apply(&mut d, ctx, c, &mut report)?;
            None
        }
        Command::Audio(c) => {
            audio::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Routing(c) => {
            routing::apply(&mut d, c, &mut report)?;
            None
        }
    };
    Ok(Applied {
        doc: d,
        report,
        selection,
    })
}
```

Create `crates/core-model/src/command/routing.rs`:

```rust
//! Instrument-list, output-path and media-pool commands. Removing an item
//! in use is refused unless a replacement is given.

use super::Report;
use crate::document::{DestKind, Destination, Document, MediaItem, OutputPath, Patch};
use crate::error::{ModelError, Result};
use crate::event::EventBody;
use crate::ids::{AnyId, DestId, LaneId, MediaId, OutputPathId, SeqId};
use crate::rules::{check_event_fields, check_group_channel};

/// Destination, output and media edits.
#[derive(Debug, Clone, PartialEq)]
pub enum RoutingCmd {
    /// Add an instrument-list entry.
    AddDestination {
        /// Name.
        name: String,
        /// Kind.
        kind: DestKind,
        /// Default patch.
        default_patch: Option<Patch>,
    },
    /// Rename a destination.
    RenameDestination {
        /// Target.
        dest: DestId,
        /// New name.
        name: String,
    },
    /// Change a destination; every lane and event using it re-routes.
    EditDestination {
        /// Target.
        dest: DestId,
        /// New kind.
        kind: DestKind,
        /// New default patch.
        default_patch: Option<Patch>,
    },
    /// Remove a destination, re-pointing users to `replacement`.
    RemoveDestination {
        /// Target.
        dest: DestId,
        /// Replacement (required when in use).
        replacement: Option<DestId>,
    },
    /// Add an output path.
    AddOutput {
        /// Name.
        name: String,
        /// Opaque engine reference.
        reference: String,
    },
    /// Rename an output path.
    RenameOutput {
        /// Target.
        output: OutputPathId,
        /// New name.
        name: String,
    },
    /// Change an output path's engine reference.
    EditOutput {
        /// Target.
        output: OutputPathId,
        /// New reference.
        reference: String,
    },
    /// Remove an output path, re-pointing lanes to `replacement`.
    RemoveOutput {
        /// Target.
        output: OutputPathId,
        /// Replacement (required when in use).
        replacement: Option<OutputPathId>,
    },
    /// Add a media item.
    AddMedia {
        /// Package-relative path.
        path: String,
        /// Sample rate (> 0).
        sample_rate: u32,
        /// Channels (> 0).
        channels: u16,
        /// Length in frames.
        length_frames: u64,
        /// Optional content hash.
        content_hash: Option<String>,
    },
    /// Remove a media item, re-pointing regions to `replacement`.
    RemoveMedia {
        /// Target.
        media: MediaId,
        /// Replacement (required when in use; must be long enough).
        replacement: Option<MediaId>,
    },
}

impl RoutingCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            RoutingCmd::AddDestination { .. } => "Add Destination",
            RoutingCmd::RenameDestination { .. } => "Rename Destination",
            RoutingCmd::EditDestination { .. } => "Edit Destination",
            RoutingCmd::RemoveDestination { .. } => "Remove Destination",
            RoutingCmd::AddOutput { .. } => "Add Output",
            RoutingCmd::RenameOutput { .. } => "Rename Output",
            RoutingCmd::EditOutput { .. } => "Edit Output",
            RoutingCmd::RemoveOutput { .. } => "Remove Output",
            RoutingCmd::AddMedia { .. } => "Add Media",
            RoutingCmd::RemoveMedia { .. } => "Remove Media",
        }
    }
}

fn check_dest(kind: &DestKind, patch: &Option<Patch>) -> Result<()> {
    if let DestKind::MidiEndpoint { group, channel, .. } = kind {
        check_group_channel(Some((*group, *channel)))?;
    }
    if let Some(p) = patch {
        if p.program > 127 || p.bank.is_some_and(|(m, l)| m > 127 || l > 127) {
            return Err(ModelError::range("default_patch", p.program));
        }
    }
    Ok(())
}

fn all_lanes(d: &Document) -> Vec<(SeqId, LaneId)> {
    d.sequences()
        .values()
        .flat_map(|s| s.lanes.iter().map(move |l| (s.id, l.id)))
        .collect()
}

fn require_replacement<T: Copy + PartialEq>(
    target: T,
    replacement: Option<T>,
    exists: bool,
    users: Vec<AnyId>,
) -> Result<Option<T>> {
    match replacement {
        None if !users.is_empty() => Err(ModelError::InUse { users }),
        Some(r) if r == target => Err(ModelError::invalid("replacement is the item itself")),
        Some(_) if !exists => Err(ModelError::invalid("replacement does not exist")),
        other => Ok(other),
    }
}

fn remove_destination(
    d: &mut Document,
    dest: DestId,
    replacement: Option<DestId>,
    r: &mut Report,
) -> Result<()> {
    if !d.instruments().contains_key(&dest) {
        return Err(ModelError::UnknownId(AnyId::Dest(dest)));
    }
    let mut users = Vec::new();
    for s in d.sequences().values() {
        for l in &s.lanes {
            if l.midi_dest == Some(dest) {
                users.push(AnyId::Lane(l.id));
            }
            if let Some(layout) = &l.drum {
                for row in &layout.rows {
                    if row.dest == Some(dest) {
                        users.push(AnyId::DrumRow(row.id));
                    }
                }
            }
        }
    }
    let events: Vec<_> = d
        .all_events()
        .filter(|(_, e)| e.dest == Some(dest))
        .map(|(_, e)| e.id)
        .collect();
    users.extend(events.iter().map(|e| AnyId::Event(*e)));
    let exists = replacement.is_some_and(|x| d.instruments().contains_key(&x));
    let rep = require_replacement(dest, replacement, exists, users)?;
    for (s, l) in all_lanes(d) {
        let lane = d.lane_mut(s, l)?;
        if lane.midi_dest == Some(dest) {
            lane.midi_dest = rep;
        }
        if let Some(layout) = lane.drum.as_mut() {
            for row in layout.rows.iter_mut() {
                if row.dest == Some(dest) {
                    row.dest = rep;
                }
            }
        }
    }
    for id in events {
        let mut ev = d.event(id)?.0.clone();
        ev.dest = rep;
        d.replace_event(ev)?;
    }
    d.instruments_mut().remove(&dest);
    r.removed.push(AnyId::Dest(dest));
    Ok(())
}

pub(crate) fn apply(d: &mut Document, cmd: &RoutingCmd, r: &mut Report) -> Result<()> {
    match cmd {
        RoutingCmd::AddDestination {
            name,
            kind,
            default_patch,
        } => {
            check_dest(kind, default_patch)?;
            let id = d.counters_mut().alloc_dest();
            d.instruments_mut().insert(
                id,
                Destination {
                    id,
                    name: name.clone(),
                    kind: kind.clone(),
                    default_patch: *default_patch,
                },
            );
            r.created.push(AnyId::Dest(id));
        }
        RoutingCmd::RenameDestination { dest, name } => {
            d.instruments_mut()
                .get_mut(dest)
                .ok_or(ModelError::UnknownId(AnyId::Dest(*dest)))?
                .name = name.clone();
        }
        RoutingCmd::EditDestination {
            dest,
            kind,
            default_patch,
        } => {
            check_dest(kind, default_patch)?;
            let x = d
                .instruments_mut()
                .get_mut(dest)
                .ok_or(ModelError::UnknownId(AnyId::Dest(*dest)))?;
            x.kind = kind.clone();
            x.default_patch = *default_patch;
        }
        RoutingCmd::RemoveDestination { dest, replacement } => {
            remove_destination(d, *dest, *replacement, r)?
        }
        RoutingCmd::AddOutput { name, reference } => {
            let id = d.counters_mut().alloc_output();
            d.outputs_mut().insert(
                id,
                OutputPath {
                    id,
                    name: name.clone(),
                    reference: reference.clone(),
                },
            );
            r.created.push(AnyId::Output(id));
        }
        RoutingCmd::RenameOutput { output, name } => {
            d.outputs_mut()
                .get_mut(output)
                .ok_or(ModelError::UnknownId(AnyId::Output(*output)))?
                .name = name.clone();
        }
        RoutingCmd::EditOutput { output, reference } => {
            d.outputs_mut()
                .get_mut(output)
                .ok_or(ModelError::UnknownId(AnyId::Output(*output)))?
                .reference = reference.clone();
        }
        RoutingCmd::RemoveOutput {
            output,
            replacement,
        } => {
            if !d.outputs().contains_key(output) {
                return Err(ModelError::UnknownId(AnyId::Output(*output)));
            }
            let users: Vec<AnyId> = d
                .sequences()
                .values()
                .flat_map(|s| s.lanes.iter())
                .filter(|l| l.audio_out == Some(*output))
                .map(|l| AnyId::Lane(l.id))
                .collect();
            let exists = replacement.is_some_and(|x| d.outputs().contains_key(&x));
            let rep = require_replacement(*output, *replacement, exists, users)?;
            for (s, l) in all_lanes(d) {
                let lane = d.lane_mut(s, l)?;
                if lane.audio_out == Some(*output) {
                    lane.audio_out = rep;
                }
            }
            d.outputs_mut().remove(output);
            r.removed.push(AnyId::Output(*output));
        }
        RoutingCmd::AddMedia {
            path,
            sample_rate,
            channels,
            length_frames,
            content_hash,
        } => {
            if *sample_rate == 0 || *channels == 0 {
                return Err(ModelError::range("media.format", sample_rate));
            }
            let id = d.counters_mut().alloc_media();
            d.media_mut().insert(
                id,
                MediaItem {
                    id,
                    path: path.clone(),
                    sample_rate: *sample_rate,
                    channels: *channels,
                    length_frames: *length_frames,
                    content_hash: content_hash.clone(),
                },
            );
            r.created.push(AnyId::Media(id));
        }
        RoutingCmd::RemoveMedia { media, replacement } => {
            if !d.media().contains_key(media) {
                return Err(ModelError::UnknownId(AnyId::Media(*media)));
            }
            let regions: Vec<_> = d
                .all_events()
                .filter(|(_, e)| matches!(e.body, EventBody::AudioRegion { media: m, .. } if m == *media))
                .map(|(_, e)| e.id)
                .collect();
            let users = regions.iter().map(|e| AnyId::Event(*e)).collect();
            let exists = replacement.is_some_and(|x| d.media().contains_key(&x));
            let rep = require_replacement(*media, *replacement, exists, users)?;
            if let Some(rep) = rep {
                for id in regions {
                    let mut ev = d.event(id)?.0.clone();
                    if let EventBody::AudioRegion { media: m, .. } = &mut ev.body {
                        *m = rep;
                    }
                    check_event_fields(d, &ev.spec())?;
                    d.replace_event(ev)?;
                }
            }
            d.media_mut().remove(media);
            r.removed.push(AnyId::Media(*media));
        }
    }
    Ok(())
}
```


- [ ] **Step 4: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_audio_routing
```

Expected: PASS — `cmd_audio_routing`: `test result: ok. 8 passed`.

- [ ] **Step 5: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/command/audio.rs crates/core-model/src/command/mod.rs crates/core-model/src/command/routing.rs crates/core-model/tests/cmd_audio_routing.rs
git commit -m "feat(core-model): audio-region and routing commands"
```

---

### Task 14: Rules, select-and-modify, settings commands and templates

**Goal:** Composable `Rule` predicates and `events_matching`; `Modify` operations; `Command::SelectAndModify` (select/add/refine, view-filter aware, optional copies); `SettingsCmd` and named templates stored in document settings.

**Files:**
- Modify: `crates/core-model/src/command/mod.rs`
- Create: `crates/core-model/src/command/select.rs`
- Create: `crates/core-model/src/command/settings.rs`
- Modify: `crates/core-model/src/document.rs`
- Modify: `crates/core-model/src/lib.rs`
- Create: `crates/core-model/src/modify.rs`
- Create: `crates/core-model/src/rule.rs`
- Create (test): `crates/core-model/tests/select_modify.rs`

**Interfaces:**
- Consumes: `edit_events`, `shift_pitch` (Task 8); `resolve_with` (Task 10); `Selection`, `ViewFilter`, `Context` (Task 6).
- Produces:
  - `core_model::rule::{Rule, SelectMode { Select, Add, Refine }, matches(&Document, &Rule, lane_dest: Option<DestId>, &Event, index: usize) -> bool, filter_events(&Document, candidates, &Rule, &ViewFilter) -> Vec<EventId>, events_matching(&Document, lanes: &OrdSet<(SeqId, LaneId)>, &Rule, &ViewFilter) -> Vec<EventId>}`.
  - `core_model::modify::{Modify, Transpose, Quantize, ValueOp, DurationOp, ModifyTemplate { rule, mode, modify }}`; `Modify::name() -> &'static str`; crate-internal `apply_modify(&mut Document, &[EventId], &Modify, &mut Report) -> Result<()>`.
  - `Command::SelectAndModify { rule: Option<Rule>, mode: SelectMode, modify: Option<Modify>, to_copy: bool }` — returns `Applied.selection = Some(new selection)`; `name()` is the modification's name or `"Select"`.
  - `Command::Settings(SettingsCmd)`; `SettingsCmd::{SetTemplate { name, template: Option<ModifyTemplate> }, SetHistoryDepth { depth }, SetAutosaveKeep { keep }}`.
  - `Settings` gains `templates: OrdMap<String, ModifyTemplate>`.

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/select_modify.rs`:

```rust
use core_model::command::{
    Applied, Command, Context, EventCmd, SequenceCmd, SettingsCmd, apply, apply_in,
};
use core_model::document::Document;
use core_model::event::{EventBody, EventKind, EventSpec};
use core_model::ids::{AnyId, EventId, LaneId, SeqId};
use core_model::modify::{DurationOp, Modify, ModifyTemplate, Quantize, Transpose, ValueOp};
use core_model::rule::{Rule, SelectMode, events_matching};
use core_model::selection::{Selection, ViewFilter};
use core_model::time::{PPQ, Tick};

const Q: i64 = PPQ;

fn note(t: i64, pitch: u8, vel: u16, dur: i64) -> EventSpec {
    EventSpec::new(Tick(t), EventBody::note(pitch, vel, Tick(dur)))
}

/// seq0/lane0 with: ev0 note 60 @0, ev1 note 64 @Q+10, ev2 note 67 @2Q,
/// ev3 CC7=100 @3Q, ev4 note 72 @4Q (velocities 10, 20, 30, -, 40).
fn doc() -> Document {
    let d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    let events = vec![
        note(0, 60, 10, Q / 2),
        note(Q + 10, 64, 20, Q / 2),
        note(2 * Q, 67, 30, Q),
        EventSpec::new(
            Tick(3 * Q),
            EventBody::Cc {
                index: 7,
                value: 100,
            },
        ),
        note(4 * Q, 72, 40, 2 * Q),
    ];
    apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events,
        }),
    )
    .unwrap()
    .doc
}

fn lane_sel(events: &[u64]) -> Selection {
    Selection {
        time_range: None,
        lanes: [(SeqId(0), LaneId(0))].into_iter().collect(),
        events: events.iter().map(|e| EventId(*e)).collect(),
    }
}

fn sm(
    d: &Document,
    sel: &Selection,
    filter: &ViewFilter,
    rule: Option<Rule>,
    mode: SelectMode,
    modify: Option<Modify>,
    to_copy: bool,
) -> Applied {
    apply_in(
        d,
        Context {
            selection: sel,
            filter,
        },
        &Command::SelectAndModify {
            rule,
            mode,
            modify,
            to_copy,
        },
    )
    .unwrap()
}

fn ids(s: &Selection) -> Vec<u64> {
    s.events.iter().map(|e| e.0).collect()
}

fn matching(d: &Document, rule: Rule) -> Vec<u64> {
    events_matching(d, &lane_sel(&[]).lanes, &rule, &ViewFilter::default())
        .into_iter()
        .map(|e| e.0)
        .collect()
}

fn body(d: &Document, id: u64) -> EventBody {
    d.event(EventId(id)).unwrap().0.body.clone()
}

#[test]
fn rules_match_the_right_events() {
    let d = doc();
    assert_eq!(
        matching(
            &d,
            Rule::Kind {
                kinds: vec![EventKind::Cc]
            }
        ),
        vec![3]
    );
    assert_eq!(
        matching(&d, Rule::PitchRange { low: 62, high: 70 }),
        vec![1, 2]
    );
    assert_eq!(
        matching(&d, Rule::VelocityRange { low: 15, high: 35 }),
        vec![1, 2]
    );
    assert_eq!(
        matching(
            &d,
            Rule::DurationRange {
                min: Tick(Q),
                max: Tick(4 * Q)
            }
        ),
        vec![2, 4]
    );
    assert_eq!(
        matching(
            &d,
            Rule::TimeRange {
                start: Tick(Q),
                end: Tick(3 * Q)
            }
        ),
        vec![1, 2]
    );
    assert_eq!(
        matching(
            &d,
            Rule::MetricPosition {
                grid: Tick(Q),
                offset: Tick(0),
                tolerance: Tick(0)
            }
        ),
        vec![0, 2, 3, 4]
    );
    assert_eq!(matching(&d, Rule::EveryNth { n: 2, phase: 1 }), vec![1, 3]);
    assert_eq!(matching(&d, Rule::Dest { dest: None }), vec![0, 1, 2, 3, 4]);
    let combo = Rule::All(vec![
        Rule::Kind {
            kinds: vec![EventKind::Note],
        },
        Rule::Not(Box::new(Rule::Any(vec![
            Rule::PitchRange { low: 60, high: 60 },
            Rule::PitchRange { low: 72, high: 72 },
        ]))),
    ]);
    assert_eq!(matching(&d, combo), vec![1, 2]);
}

#[test]
fn select_add_and_refine() {
    let d = doc();
    let f = ViewFilter::default();
    let notes = Rule::Kind {
        kinds: vec![EventKind::Note],
    };
    let a = sm(
        &d,
        &lane_sel(&[3]),
        &f,
        Some(notes.clone()),
        SelectMode::Select,
        None,
        false,
    );
    assert_eq!(ids(&a.selection.unwrap()), vec![0, 1, 2, 4]);
    let a = sm(
        &d,
        &lane_sel(&[3]),
        &f,
        Some(Rule::PitchRange { low: 72, high: 72 }),
        SelectMode::Add,
        None,
        false,
    );
    assert_eq!(ids(&a.selection.unwrap()), vec![3, 4]);
    let a = sm(
        &d,
        &lane_sel(&[0, 1, 3]),
        &f,
        Some(notes),
        SelectMode::Refine,
        None,
        false,
    );
    assert_eq!(ids(&a.selection.unwrap()), vec![0, 1]);
    // Selection-only: the document is unchanged.
    assert_eq!(a.doc, d);
}

#[test]
fn hidden_events_cannot_be_selected_or_changed() {
    let d = doc();
    let mut f = ViewFilter::default();
    f.hidden_kinds.insert(EventKind::Cc);
    let a = sm(
        &d,
        &lane_sel(&[0, 3]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::ControllerValue(ValueOp::Set(1))),
        false,
    );
    assert_eq!(ids(&a.selection.unwrap()), vec![0]);
    assert_eq!(
        body(&a.doc, 3),
        EventBody::Cc {
            index: 7,
            value: 100
        }
    );
}

#[test]
fn transpose_interval_and_scale() {
    let d = doc();
    let f = ViewFilter::default();
    let a = sm(
        &d,
        &lane_sel(&[0, 4]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Transpose(Transpose::Interval { semitones: 60 })),
        false,
    );
    assert_eq!(body(&a.doc, 0), EventBody::note(120, 10, Tick(Q / 2)));
    assert_eq!(body(&a.doc, 4), EventBody::note(127, 40, Tick(2 * Q)));
    // C major pentatonic (C D E G A): 66 (F#) snaps to the nearest member, 67 (G).
    let penta = Transpose::ToScale {
        root: 0,
        mask: 0b0010_1001_0101,
    };
    let a = sm(
        &d,
        &lane_sel(&[1]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Transpose(Transpose::Interval { semitones: 2 })),
        false,
    );
    let a = sm(
        &a.doc,
        &lane_sel(&[1]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Transpose(penta)),
        false,
    );
    assert_eq!(body(&a.doc, 1), EventBody::note(67, 20, Tick(Q / 2)));
    // 61 (C#) is equidistant from C and D: ties go down.
    let a = sm(
        &d,
        &lane_sel(&[0]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Transpose(Transpose::Interval { semitones: 1 })),
        false,
    );
    let a = sm(
        &a.doc,
        &lane_sel(&[0]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Transpose(penta)),
        false,
    );
    assert_eq!(body(&a.doc, 0), EventBody::note(60, 10, Tick(Q / 2)));
}

#[test]
fn quantize_grid_and_groove() {
    let d = doc();
    let f = ViewFilter::default();
    let a = sm(
        &d,
        &lane_sel(&[1]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Quantize(Quantize::Grid {
            grid: Tick(Q),
            strength: 100,
        })),
        false,
    );
    assert_eq!(a.doc.event(EventId(1)).unwrap().0.tick, Tick(Q));
    let groove = Quantize::Groove {
        grid: Tick(Q),
        offsets: vec![Tick(0), Tick(30)],
        strength: 100,
    };
    let a = sm(
        &d,
        &lane_sel(&[1]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Quantize(groove)),
        false,
    );
    assert_eq!(a.doc.event(EventId(1)).unwrap().0.tick, Tick(Q + 30));
    let bad = Quantize::Grid {
        grid: Tick(0),
        strength: 100,
    };
    assert!(
        apply_in(
            &d,
            Context {
                selection: &lane_sel(&[1]),
                filter: &f
            },
            &Command::SelectAndModify {
                rule: None,
                mode: SelectMode::Select,
                modify: Some(Modify::Quantize(bad)),
                to_copy: false
            }
        )
        .is_err()
    );
}

#[test]
fn velocity_duration_legato_scale_move_dest() {
    let d = doc();
    let f = ViewFilter::default();
    let s = lane_sel(&[0, 1, 2]);
    let a = sm(
        &d,
        &s,
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Velocity(ValueOp::ScalePercent(200))),
        false,
    );
    assert_eq!(body(&a.doc, 2), EventBody::note(67, 60, Tick(Q)));
    let a = sm(
        &a.doc,
        &s,
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Velocity(ValueOp::Add(-100))),
        false,
    );
    assert_eq!(body(&a.doc, 2), EventBody::note(67, 0, Tick(Q)));
    let a = sm(
        &d,
        &s,
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Duration(DurationOp::Set(Tick(7)))),
        false,
    );
    assert_eq!(body(&a.doc, 0), EventBody::note(60, 10, Tick(7)));
    let a = sm(
        &d,
        &s,
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Legato),
        false,
    );
    assert_eq!(body(&a.doc, 0), EventBody::note(60, 10, Tick(Q + 10)));
    assert_eq!(body(&a.doc, 2), EventBody::note(67, 30, Tick(Q)));
    let a = sm(
        &d,
        &lane_sel(&[2]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::ScaleTime {
            num: 1,
            den: 2,
            anchor: Tick(0),
        }),
        false,
    );
    let (e, _) = a.doc.event(EventId(2)).unwrap();
    assert_eq!(
        (e.tick, e.body.clone()),
        (Tick(Q), EventBody::note(67, 30, Tick(Q / 2)))
    );
    let a = sm(
        &d,
        &lane_sel(&[2]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Move { delta: Tick(-Q) }),
        false,
    );
    assert_eq!(a.doc.event(EventId(2)).unwrap().0.tick, Tick(Q));
    let unknown = apply_in(
        &d,
        Context {
            selection: &lane_sel(&[2]),
            filter: &f,
        },
        &Command::SelectAndModify {
            rule: None,
            mode: SelectMode::Select,
            modify: Some(Modify::ReassignDest {
                dest: Some(core_model::ids::DestId(5)),
            }),
            to_copy: false,
        },
    );
    assert!(unknown.is_err());
}

#[test]
fn modify_copies_leaves_originals() {
    let d = doc();
    let a = sm(
        &d,
        &lane_sel(&[0]),
        &ViewFilter::default(),
        None,
        SelectMode::Select,
        Some(Modify::Transpose(Transpose::Interval { semitones: 12 })),
        true,
    );
    assert_eq!(a.report.created, vec![AnyId::Event(EventId(5))]);
    assert_eq!(ids(&a.selection.unwrap()), vec![5]);
    assert_eq!(body(&a.doc, 0), EventBody::note(60, 10, Tick(Q / 2)));
    assert_eq!(body(&a.doc, 5), EventBody::note(72, 10, Tick(Q / 2)));
}

#[test]
fn templates_are_stored_in_settings() {
    let t = ModifyTemplate {
        rule: Some(Rule::Kind {
            kinds: vec![EventKind::Note],
        }),
        mode: SelectMode::Select,
        modify: Some(Modify::Quantize(Quantize::Grid {
            grid: Tick(Q / 4),
            strength: 80,
        })),
    };
    let d = apply(
        &doc(),
        &Command::Settings(SettingsCmd::SetTemplate {
            name: "Q16".into(),
            template: Some(t.clone()),
        }),
    )
    .unwrap()
    .doc;
    assert_eq!(d.settings().templates.get("Q16"), Some(&t));
    let json = serde_json::to_string(&d).unwrap();
    let back: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(back, d);
    let d = apply(
        &d,
        &Command::Settings(SettingsCmd::SetTemplate {
            name: "Q16".into(),
            template: None,
        }),
    )
    .unwrap()
    .doc;
    assert!(d.settings().templates.is_empty());
    assert!(
        apply(
            &d,
            &Command::Settings(SettingsCmd::SetHistoryDepth { depth: 0 })
        )
        .is_err()
    );
}

#[test]
fn undo_label_names_the_modification() {
    let c = Command::SelectAndModify {
        rule: None,
        mode: SelectMode::Select,
        modify: Some(Modify::Legato),
        to_copy: false,
    };
    assert_eq!(c.name(), "Legato");
    let c = Command::SelectAndModify {
        rule: None,
        mode: SelectMode::Select,
        modify: None,
        to_copy: false,
    };
    assert_eq!(c.name(), "Select");
}
```


- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test select_modify
```

Expected: FAIL to compile — `error[E0432]: unresolved imports `core_model::modify`, `core_model::rule`, `core_model::command::SettingsCmd``.

- [ ] **Step 3: Implement**

Replace the whole of `crates/core-model/src/command/mod.rs` with:

```rust
//! Commands: every edit is a `Command` applied by the pure [`apply`].

pub mod audio;
pub mod drum;
pub mod event;
pub mod lane;
pub mod reference;
pub mod routing;
mod select;
pub mod sequence;
pub mod settings;

pub use audio::AudioCmd;
pub use drum::DrumCmd;
pub use event::EventCmd;
pub use lane::LaneCmd;
pub use reference::ReferenceCmd;
pub use routing::RoutingCmd;
pub use sequence::SequenceCmd;
pub use settings::SettingsCmd;

use crate::document::Document;
use crate::error::Result;
use crate::ids::AnyId;
use crate::modify::Modify;
use crate::rule::{Rule, SelectMode};
use crate::selection::{Selection, ViewFilter};

/// What a command did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// IDs created, in creation order.
    pub created: Vec<AnyId>,
    /// IDs removed.
    pub removed: Vec<AnyId>,
    /// Non-fatal notes for the user.
    pub warnings: Vec<String>,
}

/// The result of applying a command.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// The new document.
    pub doc: Document,
    /// What changed.
    pub report: Report,
    /// The new selection, for commands that change it.
    pub selection: Option<Selection>,
}

/// The current selection and view filter, for commands that act on them.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The document's selection.
    pub selection: &'a Selection,
    /// The view filter of the view issuing the command.
    pub filter: &'a ViewFilter,
}

/// Every edit the model supports.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Sequence-level edits.
    Sequence(SequenceCmd),
    /// Lane and take edits.
    Lane(LaneCmd),
    /// Event edits.
    Event(EventCmd),
    /// Drum-lane edits.
    Drum(DrumCmd),
    /// Reference edits.
    Reference(ReferenceCmd),
    /// Audio-region edits.
    Audio(AudioCmd),
    /// Destination, output and media edits.
    Routing(RoutingCmd),
    /// Document-settings edits.
    Settings(SettingsCmd),
    /// Select events by rule, then optionally modify them (or copies).
    SelectAndModify {
        /// Rule; `None` uses the current selection as is.
        rule: Option<Rule>,
        /// How the rule combines with the selection.
        mode: SelectMode,
        /// Modification; `None` only changes the selection.
        modify: Option<Modify>,
        /// Modify copies inserted beside the originals.
        to_copy: bool,
    },
}

impl Command {
    /// The stable name, used for key bindings and undo labels.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Sequence(c) => c.name(),
            Command::Lane(c) => c.name(),
            Command::Event(c) => c.name(),
            Command::Drum(c) => c.name(),
            Command::Reference(c) => c.name(),
            Command::Audio(c) => c.name(),
            Command::Routing(c) => c.name(),
            Command::Settings(c) => c.name(),
            Command::SelectAndModify { modify, .. } => {
                modify.as_ref().map_or("Select", Modify::name)
            }
        }
    }
}

/// Applies a command with an empty selection and no view filter.
pub fn apply(doc: &Document, cmd: &Command) -> Result<Applied> {
    let selection = Selection::default();
    let filter = ViewFilter::default();
    apply_in(
        doc,
        Context {
            selection: &selection,
            filter: &filter,
        },
        cmd,
    )
}

/// Applies a command in the context of a selection and view filter. Pure:
/// `doc` is never changed; on error nothing is returned but the error.
pub fn apply_in(doc: &Document, ctx: Context<'_>, cmd: &Command) -> Result<Applied> {
    let mut d = doc.clone();
    let mut report = Report::default();
    let selection = match cmd {
        Command::Sequence(c) => {
            sequence::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Lane(c) => {
            lane::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Event(c) => {
            event::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Drum(c) => {
            drum::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Reference(c) => {
            reference::apply(&mut d, ctx, c, &mut report)?;
            None
        }
        Command::Audio(c) => {
            audio::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Routing(c) => {
            routing::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Settings(c) => {
            settings::apply(&mut d, c, &mut report)?;
            None
        }
        Command::SelectAndModify {
            rule,
            mode,
            modify,
            to_copy,
        } => Some(select::apply(
            &mut d,
            ctx,
            rule.as_ref(),
            *mode,
            modify.as_ref(),
            *to_copy,
            &mut report,
        )?),
    };
    Ok(Applied {
        doc: d,
        report,
        selection,
    })
}
```

Create `crates/core-model/src/command/select.rs`:

```rust
//! Select-and-modify.

use super::{Context, Report};
use crate::document::Document;
use crate::error::Result;
use crate::ids::{AnyId, EventId};
use crate::modify::{Modify, apply_modify};
use crate::rule::{Rule, SelectMode, events_matching, filter_events};
use crate::selection::Selection;
use im::OrdSet;

/// Runs a select-and-modify and returns the new selection.
pub(crate) fn apply(
    d: &mut Document,
    ctx: Context<'_>,
    rule: Option<&Rule>,
    mode: SelectMode,
    modify: Option<&Modify>,
    to_copy: bool,
    r: &mut Report,
) -> Result<Selection> {
    let base = ctx.selection.retain_existing(d);
    let everything = Rule::All(Vec::new());
    let visible: OrdSet<EventId> =
        filter_events(d, base.events.iter().copied(), &everything, ctx.filter)
            .into_iter()
            .collect();
    let chosen: OrdSet<EventId> = match rule {
        None => visible,
        Some(rule) => match mode {
            SelectMode::Select => events_matching(d, &base.lanes, rule, ctx.filter)
                .into_iter()
                .collect(),
            SelectMode::Add => visible.union(
                events_matching(d, &base.lanes, rule, ctx.filter)
                    .into_iter()
                    .collect(),
            ),
            SelectMode::Refine => filter_events(d, visible.iter().copied(), rule, ctx.filter)
                .into_iter()
                .collect(),
        },
    };
    let mut targets: Vec<EventId> = chosen.iter().copied().collect();
    if let Some(m) = modify {
        if to_copy {
            let mut copies = Vec::with_capacity(targets.len());
            for id in &targets {
                let (ev, loc) = d.event(*id)?;
                let spec = ev.spec();
                let new_id = d.counters_mut().alloc_event();
                d.insert_event(loc.seq, loc.lane, Some(loc.take), spec.with_id(new_id))?;
                r.created.push(AnyId::Event(new_id));
                copies.push(new_id);
            }
            targets = copies;
        }
        apply_modify(d, &targets, m, r)?;
    }
    Ok(Selection {
        time_range: base.time_range,
        lanes: base.lanes,
        events: targets.into_iter().collect(),
    })
}
```

Create `crates/core-model/src/command/settings.rs`:

```rust
//! Document-settings commands.

use super::Report;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::modify::ModifyTemplate;

/// Settings edits.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsCmd {
    /// Store (Some) or delete (None) a named select-and-modify template.
    SetTemplate {
        /// Template name.
        name: String,
        /// The template.
        template: Option<ModifyTemplate>,
    },
    /// Undo depth limit (>= 1).
    SetHistoryDepth {
        /// Steps kept.
        depth: u32,
    },
    /// Autosave copies kept (>= 1).
    SetAutosaveKeep {
        /// Copies kept.
        keep: u32,
    },
}

impl SettingsCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            SettingsCmd::SetTemplate { .. } => "Set Modify Template",
            SettingsCmd::SetHistoryDepth { .. } => "Set Undo Depth",
            SettingsCmd::SetAutosaveKeep { .. } => "Set Autosave Count",
        }
    }
}

pub(crate) fn apply(d: &mut Document, cmd: &SettingsCmd, _r: &mut Report) -> Result<()> {
    let s = d.settings_mut();
    match cmd {
        SettingsCmd::SetTemplate { name, template } => match template {
            Some(t) => {
                s.templates.insert(name.clone(), t.clone());
            }
            None => {
                s.templates.remove(name);
            }
        },
        SettingsCmd::SetHistoryDepth { depth } => {
            if *depth == 0 {
                return Err(ModelError::range("history_depth", depth));
            }
            s.history_depth = *depth;
        }
        SettingsCmd::SetAutosaveKeep { keep } => {
            if *keep == 0 {
                return Err(ModelError::range("autosave_keep", keep));
            }
            s.autosave_keep = *keep;
        }
    }
    Ok(())
}
```

Edit `crates/core-model/src/document.rs` exactly as in this diff (only these lines change):

```diff
--- a/crates/core-model/src/document.rs
+++ b/crates/core-model/src/document.rs
@@ -5,6 +5,7 @@ use crate::error::{ModelError, Result};
 use crate::event::Event;
 use crate::ids::{AnyId, Counters, DestId, EventId, LaneId, MediaId, OutputPathId, SeqId, TakeId};
 use crate::lane::Lane;
+use crate::modify::ModifyTemplate;
 use crate::sequence::Sequence;
 use crate::time::Tick;
 use im::OrdMap;
@@ -18,6 +19,8 @@ pub struct Settings {
     pub history_depth: u32,
     /// Autosave copies to keep (default 10).
     pub autosave_keep: u32,
+    /// Named select-and-modify templates.
+    pub templates: OrdMap<String, ModifyTemplate>,
 }
 
 impl Default for Settings {
@@ -25,6 +28,7 @@ impl Default for Settings {
         Settings {
             history_depth: 200,
             autosave_keep: 10,
+            templates: OrdMap::new(),
         }
     }
 }
```

Replace the whole of `crates/core-model/src/lib.rs` with:

```rust
//! `core-model`: the VizDAW project model.
//!
//! A pure-Rust crate holding a VizDAW project and every edit that can be made
//! to it. It has no audio, MIDI I/O, UI or OS dependencies. All edits are
//! [`command::Command`] values applied by the pure function
//! [`command::apply`]; documents are immutable values that share structure.
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod command;
pub mod document;
pub mod error;
pub mod event;
pub mod ids;
pub mod lane;
pub mod meter;
pub mod modify;
pub mod query;
pub mod rule;
pub mod rules;
pub mod selection;
pub mod sequence;
pub mod tempo;
pub mod time;

pub use error::{ModelError, Result};
```

Create `crates/core-model/src/modify.rs`:

```rust
//! Modifications applied by select-and-modify.

use crate::command::Report;
use crate::command::event::{edit_events, shift_pitch};
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::event::{Event, EventBody};
use crate::ids::{DestId, EventId};
use crate::rule::{Rule, SelectMode};
use crate::time::Tick;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A change applied to every selected event it fits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modify {
    /// Change note and poly-pressure pitches.
    Transpose(Transpose),
    /// Move note starts toward a grid or groove.
    Quantize(Quantize),
    /// Change note velocities (clamped to 0..=65535).
    Velocity(ValueOp),
    /// Change note durations.
    Duration(DurationOp),
    /// Extend each note to the next selected note's start in its lane.
    Legato,
    /// Set the destination override.
    ReassignDest {
        /// New override.
        dest: Option<DestId>,
    },
    /// Scale positions (and note durations) around `anchor` by `num/den`.
    ScaleTime {
        /// Numerator (>= 1).
        num: u32,
        /// Denominator (>= 1).
        den: u32,
        /// Fixed point.
        anchor: Tick,
    },
    /// Move by `delta`.
    Move {
        /// Offset.
        delta: Tick,
    },
    /// Change CC, pitch-bend and pressure values (clamped to u32).
    ControllerValue(ValueOp),
}

/// Pitch changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transpose {
    /// By an interval (clamped at 0 and 127).
    Interval {
        /// Semitones.
        semitones: i8,
    },
    /// Snap to the nearest pitch of a scale (ties go down). Bit `i` of
    /// `mask` enables pitch class `root + i`.
    ToScale {
        /// Root pitch class 0–11.
        root: u8,
        /// 12-bit scale mask.
        mask: u16,
    },
}

/// Quantize kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quantize {
    /// Snap to `grid` with `strength` percent.
    Grid {
        /// Grid (> 0).
        grid: Tick,
        /// Strength 0–100.
        strength: u8,
    },
    /// Snap to `grid`, then add `offsets[k % len]` for grid line `k`.
    Groove {
        /// Grid (> 0).
        grid: Tick,
        /// Per-line offsets (non-empty).
        offsets: Vec<Tick>,
        /// Strength 0–100.
        strength: u8,
    },
}

/// Integer value changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueOp {
    /// Set to a value.
    Set(u32),
    /// Scale by a percentage.
    ScalePercent(u32),
    /// Add a signed amount.
    Add(i64),
}

/// Duration changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurationOp {
    /// Set to a length (>= 0).
    Set(Tick),
    /// Scale by a percentage.
    ScalePercent(u32),
}

/// A named select-and-modify template, stored in document settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModifyTemplate {
    /// Selection rule.
    pub rule: Option<Rule>,
    /// Selection mode.
    pub mode: SelectMode,
    /// Modification.
    pub modify: Option<Modify>,
}

impl Modify {
    /// Stable name, used as the undo label.
    pub fn name(&self) -> &'static str {
        match self {
            Modify::Transpose(_) => "Transpose",
            Modify::Quantize(_) => "Quantize",
            Modify::Velocity(_) => "Change Velocity",
            Modify::Duration(_) => "Change Duration",
            Modify::Legato => "Legato",
            Modify::ReassignDest { .. } => "Reassign Destination",
            Modify::ScaleTime { .. } => "Scale Time",
            Modify::Move { .. } => "Move",
            Modify::ControllerValue(_) => "Change Controller Values",
        }
    }
}

fn apply_op(v: u64, op: ValueOp, max: u64) -> u64 {
    let r: i128 = match op {
        ValueOp::Set(x) => i128::from(x),
        ValueOp::ScalePercent(p) => i128::from(v) * i128::from(p) / 100,
        ValueOp::Add(a) => i128::from(v) + i128::from(a),
    };
    u64::try_from(r.clamp(0, i128::from(max))).unwrap_or(max)
}

fn snap_to_scale(p: u8, root: u8, mask: u16) -> u8 {
    if mask & 0x0fff == 0 {
        return p;
    }
    let in_scale = |q: i32| {
        let pc = (q - i32::from(root % 12)).rem_euclid(12);
        mask & (1 << pc) != 0
    };
    for dist in 0..12 {
        for q in [i32::from(p) - dist, i32::from(p) + dist] {
            if (0..=127).contains(&q) && in_scale(q) {
                return shift_pitch(0, q);
            }
        }
    }
    p
}

fn quantize_tick(t: Tick, q: &Quantize) -> Result<Tick> {
    let (grid, strength, offsets): (Tick, u8, &[Tick]) = match q {
        Quantize::Grid { grid, strength } => (*grid, *strength, &[]),
        Quantize::Groove {
            grid,
            offsets,
            strength,
        } => {
            if offsets.is_empty() {
                return Err(ModelError::invalid("groove has no offsets"));
            }
            (*grid, *strength, offsets.as_slice())
        }
    };
    if grid.0 <= 0 || strength > 100 {
        return Err(ModelError::range("quantize.grid", grid.0));
    }
    let g = grid.0;
    let k = t.0.saturating_add(g / 2).div_euclid(g);
    let mut target = k.saturating_mul(g);
    if !offsets.is_empty() {
        let len = i64::try_from(offsets.len()).unwrap_or(1);
        let idx = usize::try_from(k.rem_euclid(len)).unwrap_or(0);
        target = target.saturating_add(offsets.get(idx).map_or(0, |o| o.0));
    }
    let delta = i128::from(target - t.0) * i128::from(strength) / 100;
    Ok(Tick(t.0.saturating_add(i64::try_from(delta).unwrap_or(0))))
}

fn scale(t: i64, num: u32, den: u32) -> i64 {
    let r = i128::from(t) * i128::from(num) / i128::from(den.max(1));
    i64::try_from(r).unwrap_or(if r < 0 { i64::MIN } else { i64::MAX })
}

fn modify_one(e: &mut Event, m: &Modify) -> Result<()> {
    match m {
        Modify::Transpose(t) => match &mut e.body {
            EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } => {
                *pitch = match t {
                    Transpose::Interval { semitones } => shift_pitch(*pitch, i32::from(*semitones)),
                    Transpose::ToScale { root, mask } => snap_to_scale(*pitch, *root, *mask),
                }
            }
            _ => {}
        },
        Modify::Quantize(q) => {
            if let EventBody::Note { .. } = e.body {
                e.tick = quantize_tick(e.tick, q)?;
            }
        }
        Modify::Velocity(op) => {
            if let EventBody::Note { velocity, .. } = &mut e.body {
                let v = apply_op(u64::from(*velocity), *op, u64::from(u16::MAX));
                *velocity = u16::try_from(v).unwrap_or(u16::MAX);
            }
        }
        Modify::Duration(op) => {
            if let EventBody::Note { duration, .. } = &mut e.body {
                *duration = match op {
                    DurationOp::Set(t) => *t,
                    DurationOp::ScalePercent(p) => Tick(scale(duration.0, *p, 100)),
                };
            }
        }
        Modify::Legato => {}
        Modify::ReassignDest { dest } => e.dest = *dest,
        Modify::ScaleTime { num, den, anchor } => {
            if *num == 0 || *den == 0 {
                return Err(ModelError::range("scale_time", format!("{num}/{den}")));
            }
            e.tick = *anchor + Tick(scale((e.tick - *anchor).0, *num, *den));
            if let EventBody::Note { duration, .. } = &mut e.body {
                *duration = Tick(scale(duration.0, *num, *den));
            }
        }
        Modify::Move { delta } => e.tick = e.tick + *delta,
        Modify::ControllerValue(op) => match &mut e.body {
            EventBody::Cc { value, .. }
            | EventBody::PitchBend { value }
            | EventBody::ChannelPressure { value }
            | EventBody::PolyPressure { value, .. } => {
                let v = apply_op(u64::from(*value), *op, u64::from(u32::MAX));
                *value = u32::try_from(v).unwrap_or(u32::MAX);
            }
            _ => {}
        },
    }
    Ok(())
}

/// Applies `m` to the events `ids`.
pub(crate) fn apply_modify(
    d: &mut Document,
    ids: &[EventId],
    m: &Modify,
    r: &mut Report,
) -> Result<()> {
    if let Modify::Legato = m {
        // Group notes by lane, then extend each to the next start.
        let mut by_lane: BTreeMap<_, Vec<(Tick, EventId)>> = BTreeMap::new();
        for id in ids {
            let (ev, loc) = d.event(*id)?;
            if let EventBody::Note { .. } = ev.body {
                by_lane
                    .entry((loc.seq, loc.lane))
                    .or_default()
                    .push((ev.tick, *id));
            }
        }
        for notes in by_lane.values_mut() {
            notes.sort();
            for pair in notes.windows(2) {
                if let [(t0, id), (t1, _)] = pair {
                    if t1 > t0 {
                        let len = *t1 - *t0;
                        edit_events(d, &[*id], r, |e| {
                            if let EventBody::Note { duration, .. } = &mut e.body {
                                *duration = len;
                            }
                            Ok(())
                        })?;
                    }
                }
            }
        }
        return Ok(());
    }
    edit_events(d, ids, r, |e| modify_one(e, m))
}
```

Create `crates/core-model/src/rule.rs`:

```rust
//! Composable event predicates for select-and-modify.

use crate::document::Document;
use crate::event::{Event, EventBody, EventKind};
use crate::ids::{DestId, EventId, LaneId, SeqId};
use crate::query::routing::resolve_with;
use crate::selection::ViewFilter;
use crate::time::Tick;
use im::OrdSet;
use serde::{Deserialize, Serialize};

/// A predicate over events. Ranges are inclusive except `TimeRange`
/// (`[start, end)`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// Tick in `[start, end)`.
    TimeRange {
        /// Start.
        start: Tick,
        /// End (exclusive).
        end: Tick,
    },
    /// Kind is one of these.
    Kind {
        /// Accepted kinds.
        kinds: Vec<EventKind>,
    },
    /// Note or poly-pressure pitch in range.
    PitchRange {
        /// Lowest.
        low: u8,
        /// Highest.
        high: u8,
    },
    /// Note velocity in range.
    VelocityRange {
        /// Lowest.
        low: u16,
        /// Highest.
        high: u16,
    },
    /// Note duration in range.
    DurationRange {
        /// Shortest.
        min: Tick,
        /// Longest.
        max: Tick,
    },
    /// Resolved destination equals `dest` (None = unresolved).
    Dest {
        /// Destination.
        dest: Option<DestId>,
    },
    /// `(tick - offset) mod grid` within `tolerance` of a grid line.
    MetricPosition {
        /// Grid (> 0).
        grid: Tick,
        /// Grid offset.
        offset: Tick,
        /// Allowed distance from the line.
        tolerance: Tick,
    },
    /// Every `n`th candidate (in `(tick, id)` order), starting at `phase`.
    EveryNth {
        /// Period (>= 1).
        n: u32,
        /// Index of the first match.
        phase: u32,
    },
    /// All sub-rules match.
    All(Vec<Rule>),
    /// Any sub-rule matches.
    Any(Vec<Rule>),
    /// The sub-rule does not match.
    Not(Box<Rule>),
}

/// How a rule combines with the current selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectMode {
    /// Replace the selected events with the matches in the selected lanes.
    Select,
    /// Add the matches in the selected lanes.
    Add,
    /// Keep only the selected events that match.
    Refine,
}

/// Whether `ev` (on a lane with destination `lane_dest`, at position
/// `index` among the candidates) matches `rule`.
pub fn matches(
    doc: &Document,
    rule: &Rule,
    lane_dest: Option<DestId>,
    ev: &Event,
    index: usize,
) -> bool {
    match rule {
        Rule::TimeRange { start, end } => ev.tick >= *start && ev.tick < *end,
        Rule::Kind { kinds } => kinds.contains(&ev.body.kind()),
        Rule::PitchRange { low, high } => match ev.body {
            EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } => {
                (*low..=*high).contains(&pitch)
            }
            _ => false,
        },
        Rule::VelocityRange { low, high } => {
            matches!(ev.body, EventBody::Note { velocity, .. } if (*low..=*high).contains(&velocity))
        }
        Rule::DurationRange { min, max } => {
            matches!(ev.body, EventBody::Note { duration, .. } if duration >= *min && duration <= *max)
        }
        Rule::Dest { dest } => resolve_with(doc, lane_dest, ev).dest == *dest,
        Rule::MetricPosition {
            grid,
            offset,
            tolerance,
        } => {
            if grid.0 <= 0 {
                return false;
            }
            let m = (ev.tick.0.saturating_sub(offset.0)).rem_euclid(grid.0);
            m <= tolerance.0 || grid.0 - m <= tolerance.0
        }
        Rule::EveryNth { n, phase } => {
            let n = usize::try_from((*n).max(1)).unwrap_or(1);
            let phase = usize::try_from(*phase).unwrap_or(0);
            index >= phase && (index - phase) % n == 0
        }
        Rule::All(rs) => rs.iter().all(|r| matches(doc, r, lane_dest, ev, index)),
        Rule::Any(rs) => rs.iter().any(|r| matches(doc, r, lane_dest, ev, index)),
        Rule::Not(r) => !matches(doc, r, lane_dest, ev, index),
    }
}

/// Filters `candidates` (ordered by `(tick, id)` first) to those visible
/// through `filter` and matching `rule`.
pub fn filter_events(
    doc: &Document,
    candidates: impl IntoIterator<Item = EventId>,
    rule: &Rule,
    filter: &ViewFilter,
) -> Vec<EventId> {
    let mut cands: Vec<(Tick, EventId, Option<DestId>, Event)> = candidates
        .into_iter()
        .filter_map(|id| {
            let (ev, loc) = doc.event(id).ok()?;
            let lane = doc.lane(loc.seq, loc.lane).ok()?;
            filter
                .allows(loc.lane, ev.body.kind())
                .then(|| (ev.tick, id, lane.midi_dest, ev.clone()))
        })
        .collect();
    cands.sort_by_key(|c| (c.0, c.1));
    cands
        .iter()
        .enumerate()
        .filter(|(i, c)| matches(doc, rule, c.2, &c.3, *i))
        .map(|(_, c)| c.1)
        .collect()
}

/// Active-take events of `lanes` that are visible and match `rule`.
pub fn events_matching(
    doc: &Document,
    lanes: &OrdSet<(SeqId, LaneId)>,
    rule: &Rule,
    filter: &ViewFilter,
) -> Vec<EventId> {
    let mut ids = Vec::new();
    for (s, l) in lanes {
        if let Ok(lane) = doc.lane(*s, *l) {
            if let Some(t) = lane.active() {
                ids.extend(t.events.values().map(|e| e.id));
            }
        }
    }
    filter_events(doc, ids, rule, filter)
}
```


- [ ] **Step 4: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test select_modify
```

Expected: PASS — `select_modify`: `test result: ok. 9 passed`.

- [ ] **Step 5: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/command/mod.rs crates/core-model/src/command/select.rs crates/core-model/src/command/settings.rs crates/core-model/src/document.rs crates/core-model/src/lib.rs crates/core-model/src/modify.rs crates/core-model/src/rule.rs crates/core-model/tests/select_modify.rs
git commit -m "feat(core-model): select-and-modify, rules and templates"
```

---

### Task 15: History: undo/redo, transactions, selection and Modify Again

**Goal:** `History` keeps versions with labels and timestamps, applies commands through `apply_in` with the document's selection, groups commands into transactions, enforces the depth limit and replays the last modification.

**Files:**
- Create: `crates/core-model/src/history.rs`
- Modify: `crates/core-model/src/lib.rs`
- Create (test): `crates/core-model/tests/history.rs`

**Interfaces:**
- Consumes: `apply_in`, `Context`, `Command`, `Report` (Task 6); `Modify`, `SelectMode` (Task 14); `Settings::history_depth`.
- Produces:
  - `core_model::history::{Timestamp(pub u64), Version { doc, label, at }, History}`; `Timestamp::now()`.
  - `History::{new(Document), doc(), present(), selection(), set_selection(Selection), last_modify(), undo_depth(), undo_label() -> Option<&str>, redo_label() -> Option<&str>, in_transaction(), execute(&Command, &ViewFilter) -> Result<Report>, modify_again(&ViewFilter) -> Result<Report>, undo() -> Result<()>, redo() -> Result<()>, begin(label) -> Result<()>, commit() -> Result<()>, abort() -> Result<()>}`.

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/history.rs`:

```rust
use core_model::command::{Command, EventCmd, SequenceCmd, SettingsCmd};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::history::History;
use core_model::ids::{EventId, LaneId, SeqId};
use core_model::modify::{Modify, Transpose};
use core_model::rule::{Rule, SelectMode};
use core_model::selection::{Selection, ViewFilter};
use core_model::time::{PPQ, Tick};

fn f() -> ViewFilter {
    ViewFilter::default()
}

fn new_seq(name: &str) -> Command {
    Command::Sequence(SequenceCmd::New { name: name.into() })
}

fn rename(name: &str) -> Command {
    Command::Sequence(SequenceCmd::Rename {
        seq: SeqId(0),
        name: name.into(),
    })
}

fn insert_note(pitch: u8) -> Command {
    Command::Event(EventCmd::Insert {
        seq: SeqId(0),
        lane: LaneId(0),
        events: vec![EventSpec::new(
            Tick(0),
            EventBody::note(pitch, 100, Tick(PPQ)),
        )],
    })
}

fn name(h: &History) -> String {
    h.doc().sequence(SeqId(0)).unwrap().name.clone()
}

/// History whose present has seq0 "S" and nothing to undo.
fn hist() -> History {
    let mut h = History::new(Document::new());
    h.execute(&new_seq("S"), &f()).unwrap();
    History::new(h.doc().clone())
}

#[test]
fn undo_and_redo_walk_versions_with_labels() {
    let mut h = hist();
    let original = h.doc().clone();
    h.execute(&rename("A"), &f()).unwrap();
    h.execute(&rename("B"), &f()).unwrap();
    assert_eq!(h.undo_label(), Some("Rename Sequence"));
    h.undo().unwrap();
    assert_eq!(name(&h), "A");
    assert_eq!(h.redo_label(), Some("Rename Sequence"));
    h.undo().unwrap();
    assert_eq!(h.doc(), &original);
    assert!(h.undo().is_err());
    h.redo().unwrap();
    h.redo().unwrap();
    assert_eq!(name(&h), "B");
    assert!(h.redo().is_err());
}

#[test]
fn new_command_clears_redo() {
    let mut h = hist();
    h.execute(&rename("A"), &f()).unwrap();
    h.undo().unwrap();
    h.execute(&rename("C"), &f()).unwrap();
    assert!(h.redo().is_err());
}

#[test]
fn failed_command_changes_nothing() {
    let mut h = hist();
    let before = h.doc().clone();
    assert!(h.execute(&insert_note(200), &f()).is_err());
    assert_eq!(h.doc(), &before);
    assert_eq!(h.undo_depth(), 0);
}

#[test]
fn depth_limit_drops_oldest() {
    let mut h = hist();
    h.execute(
        &Command::Settings(SettingsCmd::SetHistoryDepth { depth: 3 }),
        &f(),
    )
    .unwrap();
    for i in 0..5 {
        h.execute(&rename(&format!("n{i}")), &f()).unwrap();
    }
    assert_eq!(h.undo_depth(), 3);
    for _ in 0..3 {
        h.undo().unwrap();
    }
    assert_eq!(name(&h), "n1");
    assert!(h.undo().is_err());
}

#[test]
fn transaction_is_one_undo_step() {
    let mut h = hist();
    let before = h.doc().clone();
    h.begin("Record").unwrap();
    h.execute(&insert_note(60), &f()).unwrap();
    h.execute(&insert_note(62), &f()).unwrap();
    // Inside the pass, steps undo individually.
    h.undo().unwrap();
    h.redo().unwrap();
    h.commit().unwrap();
    assert_eq!(h.undo_label(), Some("Record"));
    let after = h.doc().clone();
    h.undo().unwrap();
    assert_eq!(h.doc(), &before);
    h.redo().unwrap();
    assert_eq!(h.doc(), &after);
}

// Review focus #5: undo after a failed command inside a transaction.
#[test]
fn undo_after_failed_command_in_transaction() {
    let mut h = hist();
    let before = h.doc().clone();
    h.begin("Drag").unwrap();
    h.execute(&insert_note(60), &f()).unwrap();
    let mid = h.doc().clone();
    assert!(h.execute(&insert_note(255), &f()).is_err());
    assert_eq!(h.doc(), &mid);
    h.commit().unwrap();
    assert_eq!(h.undo_depth(), 1);
    h.undo().unwrap();
    assert_eq!(h.doc(), &before);
    h.redo().unwrap();
    assert_eq!(h.doc(), &mid);
}

#[test]
fn undo_cannot_cross_transaction_start_and_abort_restores() {
    let mut h = hist();
    h.execute(&rename("A"), &f()).unwrap();
    let before = h.doc().clone();
    h.begin("Gesture").unwrap();
    assert!(h.begin("Nested").is_err());
    assert!(h.undo().is_err());
    h.execute(&rename("B"), &f()).unwrap();
    h.abort().unwrap();
    assert_eq!(h.doc(), &before);
    assert_eq!(h.undo_label(), Some("Rename Sequence"));
    assert!(h.commit().is_err());
    // An empty transaction adds no step.
    h.begin("Empty").unwrap();
    h.commit().unwrap();
    assert_eq!(h.undo_depth(), 1);
}

#[test]
fn selection_changes_are_not_undo_steps_and_modify_again_replays() {
    let mut h = hist();
    h.execute(&insert_note(60), &f()).unwrap();
    let depth = h.undo_depth();
    h.set_selection(Selection {
        time_range: None,
        lanes: [(SeqId(0), LaneId(0))].into_iter().collect(),
        events: Default::default(),
    });
    h.execute(
        &Command::SelectAndModify {
            rule: Some(Rule::All(vec![])),
            mode: SelectMode::Select,
            modify: None,
            to_copy: false,
        },
        &f(),
    )
    .unwrap();
    assert_eq!(h.undo_depth(), depth);
    assert_eq!(h.selection().events.len(), 1);
    let up = Modify::Transpose(Transpose::Interval { semitones: 1 });
    h.execute(
        &Command::SelectAndModify {
            rule: None,
            mode: SelectMode::Select,
            modify: Some(up.clone()),
            to_copy: false,
        },
        &f(),
    )
    .unwrap();
    assert_eq!(h.last_modify(), Some(&up));
    h.modify_again(&f()).unwrap();
    assert_eq!(
        h.doc().event(EventId(0)).unwrap().0.body,
        EventBody::note(62, 100, Tick(PPQ))
    );
    assert_eq!(h.undo_label(), Some("Transpose"));
    // Undoing past the insert drops the vanished event from the selection.
    h.undo().unwrap();
    h.undo().unwrap();
    h.undo().unwrap();
    assert!(h.selection().events.is_empty());
}

#[test]
fn modify_again_without_history_is_invalid() {
    let mut h = hist();
    assert!(h.modify_again(&f()).is_err());
}
```


- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test history
```

Expected: FAIL to compile — `error[E0432]: unresolved import `core_model::history``.

- [ ] **Step 3: Implement**

Create `crates/core-model/src/history.rs`:

```rust
//! Multi-level undo with transactions, the document selection and
//! "Modify Again". History is never saved with the project.

use crate::command::{Command, Context, Report, apply_in};
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::modify::Modify;
use crate::rule::SelectMode;
use crate::selection::{Selection, ViewFilter};

/// Milliseconds since the Unix epoch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(pub u64);

impl Timestamp {
    /// The current time (0 if the clock is before the epoch).
    pub fn now() -> Timestamp {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        Timestamp(u64::try_from(ms).unwrap_or(u64::MAX))
    }
}

/// One document state with the label of the step that produced it.
#[derive(Debug, Clone, PartialEq)]
pub struct Version {
    /// The document.
    pub doc: Document,
    /// Undo label ("Quantize", or a transaction label).
    pub label: String,
    /// When it was made.
    pub at: Timestamp,
}

#[derive(Debug, Clone)]
struct Txn {
    label: String,
    start: usize,
    base: Version,
    base_future: Vec<Version>,
}

/// Undo/redo stacks plus the selection and the last modification.
#[derive(Debug, Clone)]
pub struct History {
    past: Vec<Version>,
    present: Version,
    future: Vec<Version>,
    last_modify: Option<Modify>,
    selection: Selection,
    txn: Option<Txn>,
}

impl History {
    /// A history whose present is `doc` with nothing to undo.
    pub fn new(doc: Document) -> History {
        History {
            past: Vec::new(),
            present: Version {
                doc,
                label: String::new(),
                at: Timestamp::now(),
            },
            future: Vec::new(),
            last_modify: None,
            selection: Selection::default(),
            txn: None,
        }
    }

    /// The current document.
    pub fn doc(&self) -> &Document {
        &self.present.doc
    }

    /// The current version.
    pub fn present(&self) -> &Version {
        &self.present
    }

    /// The selection.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Replaces the selection (not an undo step).
    pub fn set_selection(&mut self, s: Selection) {
        self.selection = s.retain_existing(&self.present.doc);
    }

    /// The last modification applied by select-and-modify.
    pub fn last_modify(&self) -> Option<&Modify> {
        self.last_modify.as_ref()
    }

    /// Number of steps that can be undone.
    pub fn undo_depth(&self) -> usize {
        self.past.len()
    }

    /// Label of the step undo would revert.
    pub fn undo_label(&self) -> Option<&str> {
        self.past.last().map(|_| self.present.label.as_str())
    }

    /// Label of the step redo would re-apply.
    pub fn redo_label(&self) -> Option<&str> {
        self.future.last().map(|v| v.label.as_str())
    }

    /// Whether a transaction is open.
    pub fn in_transaction(&self) -> bool {
        self.txn.is_some()
    }

    fn trim(&mut self) {
        if self.txn.is_some() {
            return;
        }
        let depth =
            usize::try_from(self.present.doc.settings().history_depth.max(1)).unwrap_or(usize::MAX);
        if self.past.len() > depth {
            let excess = self.past.len() - depth;
            self.past.drain(..excess);
        }
    }

    /// Applies a command. On error nothing changes. A selection-only
    /// select-and-modify updates the selection without an undo step.
    pub fn execute(&mut self, cmd: &Command, filter: &ViewFilter) -> Result<Report> {
        let applied = apply_in(
            &self.present.doc,
            Context {
                selection: &self.selection,
                filter,
            },
            cmd,
        )?;
        let selection_only = matches!(cmd, Command::SelectAndModify { modify: None, .. });
        if let Command::SelectAndModify {
            modify: Some(m), ..
        } = cmd
        {
            self.last_modify = Some(m.clone());
        }
        if !selection_only {
            let next = Version {
                doc: applied.doc,
                label: cmd.name().to_string(),
                at: Timestamp::now(),
            };
            self.past.push(std::mem::replace(&mut self.present, next));
            self.future.clear();
            self.trim();
        }
        let sel = applied.selection.unwrap_or_else(|| self.selection.clone());
        self.selection = sel.retain_existing(&self.present.doc);
        Ok(applied.report)
    }

    /// Replays the last modification on the current selection.
    pub fn modify_again(&mut self, filter: &ViewFilter) -> Result<Report> {
        let m = self
            .last_modify
            .clone()
            .ok_or_else(|| ModelError::invalid("nothing to modify again"))?;
        self.execute(
            &Command::SelectAndModify {
                rule: None,
                mode: SelectMode::Select,
                modify: Some(m),
                to_copy: false,
            },
            filter,
        )
    }

    /// Steps back one version. Inside a transaction, only the transaction's
    /// own steps can be undone.
    pub fn undo(&mut self) -> Result<()> {
        let floor = self.txn.as_ref().map_or(0, |t| t.start);
        if self.past.len() <= floor {
            return Err(ModelError::invalid("nothing to undo"));
        }
        let prev = self
            .past
            .pop()
            .ok_or_else(|| ModelError::invalid("nothing to undo"))?;
        self.future.push(std::mem::replace(&mut self.present, prev));
        self.selection = self.selection.retain_existing(&self.present.doc);
        Ok(())
    }

    /// Steps forward one version.
    pub fn redo(&mut self) -> Result<()> {
        let next = self
            .future
            .pop()
            .ok_or_else(|| ModelError::invalid("nothing to redo"))?;
        self.past.push(std::mem::replace(&mut self.present, next));
        self.selection = self.selection.retain_existing(&self.present.doc);
        Ok(())
    }

    /// Opens a transaction: commands until [`History::commit`] become one
    /// undo step named `label`.
    pub fn begin(&mut self, label: impl Into<String>) -> Result<()> {
        if self.txn.is_some() {
            return Err(ModelError::invalid("a transaction is already open"));
        }
        self.txn = Some(Txn {
            label: label.into(),
            start: self.past.len(),
            base: self.present.clone(),
            base_future: self.future.clone(),
        });
        Ok(())
    }

    /// Closes the transaction, collapsing its steps into one.
    pub fn commit(&mut self) -> Result<()> {
        let t = self
            .txn
            .take()
            .ok_or_else(|| ModelError::invalid("no open transaction"))?;
        if self.past.len() > t.start {
            self.past.truncate(t.start);
            self.past.push(t.base);
            self.present.label = t.label;
            self.present.at = Timestamp::now();
        }
        self.trim();
        Ok(())
    }

    /// Closes the transaction and restores the state from before it.
    pub fn abort(&mut self) -> Result<()> {
        let t = self
            .txn
            .take()
            .ok_or_else(|| ModelError::invalid("no open transaction"))?;
        self.past.truncate(t.start);
        self.present = t.base;
        self.future = t.base_future;
        self.selection = self.selection.retain_existing(&self.present.doc);
        Ok(())
    }
}
```

Replace the whole of `crates/core-model/src/lib.rs` with:

```rust
//! `core-model`: the VizDAW project model.
//!
//! A pure-Rust crate holding a VizDAW project and every edit that can be made
//! to it. It has no audio, MIDI I/O, UI or OS dependencies. All edits are
//! [`command::Command`] values applied by the pure function
//! [`command::apply`]; documents are immutable values that share structure.
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod command;
pub mod document;
pub mod error;
pub mod event;
pub mod history;
pub mod ids;
pub mod lane;
pub mod meter;
pub mod modify;
pub mod query;
pub mod rule;
pub mod rules;
pub mod selection;
pub mod sequence;
pub mod tempo;
pub mod time;

pub use error::{ModelError, Result};
```


- [ ] **Step 4: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test history
```

Expected: PASS — `history`: `test result: ok. 9 passed`.

- [ ] **Step 5: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/history.rs crates/core-model/src/lib.rs crates/core-model/tests/history.rs
git commit -m "feat(core-model): undo history with transactions"
```

---

### Task 16: Project file: atomic save, autosave, migrations, validation, golden files

**Goal:** The `.vizdaw` package (`project.json`, `Media/`, `Autosave/`): versioned JSON with a one-pass fast path, step-by-step migration scaffold, refusal of newer versions and unknown fields, full validation on load, atomic save with fsync, and rolling autosaves; four hand-checked golden fixtures with insta snapshots.

**Files:**
- Create: `crates/core-model/src/file.rs`
- Modify: `crates/core-model/src/lib.rs`
- Create (test): `crates/core-model/tests/file.rs`
- Create (test): `crates/core-model/tests/fixtures/drum_lane.json`
- Create (test): `crates/core-model/tests/fixtures/minimal.json`
- Create (test): `crates/core-model/tests/fixtures/mixed_lanes.json`
- Create (test): `crates/core-model/tests/fixtures/nested_references.json`
- Create (test): `crates/core-model/tests/snapshots/file__drum_lane.snap`
- Create (test): `crates/core-model/tests/snapshots/file__minimal.snap`
- Create (test): `crates/core-model/tests/snapshots/file__mixed_lanes.snap`
- Create (test): `crates/core-model/tests/snapshots/file__nested_references.snap`

**Interfaces:**
- Consumes: `Document` serde (Task 4); `rules::validate` (Task 5); `Timestamp` (Task 15); all commands (fixtures were produced with them); `flatten`, `seq_length` (summaries).
- Produces:
  - `core_model::file::{FORMAT = "vizdaw-project", VERSION = 1, FIRST_VERSION = 1, PROJECT_FILE = "project.json", Migration = fn(Value) -> Result<Value>, MIGRATIONS: &[Migration] (empty)}`.
  - `core_model::file::{to_json(&Document) -> Result<String>, from_json(&str) -> Result<Document>, run_migrations(Value, found, target, first, &[Migration]) -> Result<Value>, write_atomic(&Path, &[u8]) -> Result<()>, save(&Document, package: &Path) -> Result<()>, load(package: &Path) -> Result<Document>, autosave(&Document, package: &Path, Timestamp) -> Result<PathBuf>, list_autosaves(package: &Path) -> Result<Vec<PathBuf>>}`.

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/file.rs`:

```rust
use core_model::ModelError;
use core_model::command::{Command, EventCmd, SequenceCmd, SettingsCmd, apply};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::file::{
    FORMAT, Migration, VERSION, autosave, from_json, list_autosaves, load, run_migrations, save,
    to_json,
};
use core_model::history::Timestamp;
use core_model::ids::{LaneId, SeqId};
use core_model::query::{flatten, seq_length};
use core_model::selection::ViewFilter;
use core_model::time::{PPQ, Tick};
use serde_json::{Value, json};
use std::path::PathBuf;

/// A fresh, empty directory under the system temp dir.
fn temp_package(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let p = std::env::temp_dir().join(format!(
        "vizdaw-test-{tag}-{}-{nanos}.vizdaw",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&p);
    p
}

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.json"));
    std::fs::read_to_string(path).unwrap()
}

fn small_doc() -> Document {
    let d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![EventSpec::new(Tick(0), EventBody::note(60, 1, Tick(PPQ)))],
        }),
    )
    .unwrap()
    .doc
}

#[test]
fn save_then_load_is_identical_and_leaves_no_temp_file() {
    let pkg = temp_package("save");
    let d = small_doc();
    save(&d, &pkg).unwrap();
    assert!(pkg.join("Media").is_dir());
    assert!(pkg.join("Autosave").is_dir());
    assert!(!pkg.join("project.json.tmp").exists());
    assert_eq!(load(&pkg).unwrap(), d);
    // Saving again replaces the file atomically.
    let d2 = apply(
        &d,
        &Command::Sequence(SequenceCmd::Rename {
            seq: SeqId(0),
            name: "T".into(),
        }),
    )
    .unwrap()
    .doc;
    save(&d2, &pkg).unwrap();
    assert_eq!(load(&pkg).unwrap(), d2);
    std::fs::remove_dir_all(&pkg).unwrap();
}

#[test]
fn top_level_fields_and_newer_version() {
    let text = to_json(&small_doc()).unwrap();
    let mut v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["format"], FORMAT);
    assert_eq!(v["version"], VERSION);
    v["version"] = json!(VERSION + 1);
    assert_eq!(
        from_json(&v.to_string()),
        Err(ModelError::NewerFormat { found: VERSION + 1 })
    );
    v["version"] = json!(0);
    assert!(matches!(
        from_json(&v.to_string()),
        Err(ModelError::Corrupt { .. })
    ));
    v["version"] = json!(VERSION);
    v["format"] = json!("something-else");
    assert!(matches!(
        from_json(&v.to_string()),
        Err(ModelError::Corrupt { .. })
    ));
    assert!(matches!(
        from_json("not json"),
        Err(ModelError::Corrupt { .. })
    ));
}

#[test]
fn unknown_fields_are_rejected_at_every_level() {
    let text = to_json(&small_doc()).unwrap();
    for pointer in [
        "",
        "/document",
        "/document/sequences/0",
        "/document/sequences/0/lanes/0/takes/0/events/0/body",
    ] {
        let mut v: Value = serde_json::from_str(&text).unwrap();
        v.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(1));
        assert!(
            matches!(from_json(&v.to_string()), Err(ModelError::Corrupt { .. })),
            "{pointer}"
        );
    }
}

#[test]
fn loading_reports_every_integrity_problem() {
    let text = to_json(&small_doc()).unwrap();
    let mut v: Value = serde_json::from_str(&text).unwrap();
    let ev = v
        .pointer_mut("/document/sequences/0/lanes/0/takes/0/events/0")
        .unwrap();
    ev["body"]["pitch"] = json!(130);
    ev["dest"] = json!(42);
    v["document"]["sequences"][0]["meter"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(json!({"tick": 100, "meter": {"num": 3, "den": 4}, "key": {"sharps": 0, "minor": false}}));
    match from_json(&v.to_string()) {
        Err(ModelError::Corrupt { problems }) => {
            assert_eq!(problems.len(), 2, "{problems:?}");
            assert!(problems.iter().any(|p| p.contains("barline")));
            assert!(problems.iter().any(|p| p.contains("dest42")));
        }
        other => panic!("expected Corrupt, got {other:?}"),
    }
}

#[test]
fn migrations_run_step_by_step() {
    fn add_a(mut v: Value) -> core_model::Result<Value> {
        v["a"] = json!(true);
        Ok(v)
    }
    fn add_b(mut v: Value) -> core_model::Result<Value> {
        assert_eq!(v["version"], 2);
        v["b"] = json!(true);
        Ok(v)
    }
    let steps: &[Migration] = &[add_a, add_b];
    let out = run_migrations(json!({"version": 1}), 1, 3, 1, steps).unwrap();
    assert_eq!(out, json!({"version": 3, "a": true, "b": true}));
    let out = run_migrations(json!({"version": 2}), 2, 3, 1, steps).unwrap();
    assert_eq!(out, json!({"version": 3, "b": true}));
    assert!(run_migrations(json!({}), 1, 4, 1, steps).is_err());
    assert!(run_migrations(json!({}), 0, 3, 1, steps).is_err());
}

#[test]
fn autosave_keeps_the_newest_n() {
    let pkg = temp_package("autosave");
    let d = apply(
        &small_doc(),
        &Command::Settings(SettingsCmd::SetAutosaveKeep { keep: 3 }),
    )
    .unwrap()
    .doc;
    for ms in [5_u64, 1, 4, 2, 3] {
        autosave(&d, &pkg, Timestamp(ms * 1000)).unwrap();
    }
    let names: Vec<String> = list_autosaves(&pkg)
        .unwrap()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec![
            "project-00000000000000005000.json",
            "project-00000000000000004000.json",
            "project-00000000000000003000.json"
        ]
    );
    // Same timestamp twice does not overwrite.
    let p = autosave(&d, &pkg, Timestamp(5000)).unwrap();
    assert!(p.ends_with("project-00000000000000005001.json"));
    assert_eq!(from_json(&std::fs::read_to_string(&p).unwrap()).unwrap(), d);
    std::fs::remove_dir_all(&pkg).unwrap();
}

/// A readable summary of a fixture: lengths and the flattened first sequence.
fn summary(d: &Document) -> Value {
    let lengths: Vec<Value> = d
        .sequences()
        .values()
        .map(|s| json!([s.name, seq_length(d, s.id).unwrap().0]))
        .collect();
    let first = *d.sequences().keys().next().unwrap();
    let events: Vec<String> = flatten(d, first, None, &ViewFilter::default())
        .unwrap()
        .map(|r| {
            let what = match &r.body {
                EventBody::Note { pitch, .. } => format!("note {pitch}"),
                other => format!("{:?}", other.kind()),
            };
            format!(
                "{} {} dest={:?} gc={:?} out={:?}",
                r.tick.0,
                what,
                r.dest.map(|x| x.0),
                r.group_channel,
                r.audio_out.map(|x| x.0)
            )
        })
        .collect();
    json!({ "lengths": lengths, "flattened": events })
}

fn golden(name: &str) {
    let text = fixture(name);
    let d = from_json(&text).unwrap();
    // Canonical: re-serializing reproduces the fixture byte for byte.
    assert_eq!(to_json(&d).unwrap() + "\n", text);
    insta::assert_json_snapshot!(name, summary(&d));
}

#[test]
fn golden_minimal() {
    golden("minimal");
}

#[test]
fn golden_nested_references() {
    golden("nested_references");
}

#[test]
fn golden_mixed_lanes() {
    golden("mixed_lanes");
}

#[test]
fn golden_drum_lane() {
    golden("drum_lane");
}
```


- [ ] **Step 2: Add the golden fixtures and their accepted snapshots**

These fixtures are the reviewed, canonical `project.json` texts (each ends with a newline). The snapshot files are what `insta` must find; create them verbatim so the run is deterministic (if you regenerate them with `INSTA_UPDATE=always`, diff against these before committing).

`crates/core-model/tests/fixtures/drum_lane.json`:

```json
{
  "format": "vizdaw-project",
  "version": 1,
  "document": {
    "settings": {
      "history_depth": 200,
      "autosave_keep": 10,
      "templates": {}
    },
    "instruments": [
      {
        "id": 0,
        "name": "Kit",
        "kind": {
          "midi_endpoint": {
            "endpoint_ref": "ep-2",
            "group": 0,
            "channel": 9
          }
        },
        "default_patch": null
      },
      {
        "id": 1,
        "name": "Perc",
        "kind": {
          "plugin_instrument": {
            "plugin_ref": "clap:perc"
          }
        },
        "default_patch": null
      }
    ],
    "outputs": [],
    "media": [],
    "sequences": [
      {
        "id": 0,
        "name": "Beat",
        "kind": "normal",
        "keyboard_slot": null,
        "tempo": {
          "points": [
            {
              "tick": 0,
              "bpm": 120000,
              "ramp": false
            }
          ]
        },
        "meter": {
          "entries": [
            {
              "tick": 0,
              "meter": {
                "num": 4,
                "den": 4
              },
              "key": {
                "sharps": 0,
                "minor": false
              }
            }
          ]
        },
        "markers": [],
        "comment": "",
        "start_offset": 0,
        "length": "derived",
        "lanes": [
          {
            "id": 0,
            "name": "Lane 1",
            "midi_dest": 0,
            "audio_out": null,
            "mute": false,
            "solo": false,
            "arm": false,
            "length": null,
            "looped": false,
            "length_locked": false,
            "play_quantize": null,
            "play_shift": 0,
            "drum": {
              "rows": [
                {
                  "id": 0,
                  "name": "C2",
                  "note": 36,
                  "dest": null,
                  "show_velocity": false
                },
                {
                  "id": 1,
                  "name": "D2",
                  "note": 38,
                  "dest": null,
                  "show_velocity": false
                },
                {
                  "id": 2,
                  "name": "A#4",
                  "note": 70,
                  "dest": 1,
                  "show_velocity": false
                }
              ]
            },
            "takes": [
              {
                "id": 0,
                "name": "Take 1",
                "events": [
                  {
                    "id": 0,
                    "tick": 0,
                    "dest": null,
                    "group_channel": null,
                    "mute": false,
                    "body": {
                      "type": "note",
                      "pitch": 36,
                      "velocity": 40000,
                      "release_velocity": 0,
                      "duration": 7680,
                      "attribute": null,
                      "per_note": []
                    }
                  },
                  {
                    "id": 1,
                    "tick": 15360,
                    "dest": null,
                    "group_channel": null,
                    "mute": false,
                    "body": {
                      "type": "note",
                      "pitch": 38,
                      "velocity": 40000,
                      "release_velocity": 0,
                      "duration": 7680,
                      "attribute": null,
                      "per_note": []
                    }
                  },
                  {
                    "id": 4,
                    "tick": 15360,
                    "dest": 1,
                    "group_channel": null,
                    "mute": false,
                    "body": {
                      "type": "note",
                      "pitch": 70,
                      "velocity": 40000,
                      "release_velocity": 0,
                      "duration": 7680,
                      "attribute": null,
                      "per_note": []
                    }
                  },
                  {
                    "id": 2,
                    "tick": 30720,
                    "dest": null,
                    "group_channel": null,
                    "mute": false,
                    "body": {
                      "type": "note",
                      "pitch": 36,
                      "velocity": 40000,
                      "release_velocity": 0,
                      "duration": 7680,
                      "attribute": null,
                      "per_note": []
                    }
                  },
                  {
                    "id": 3,
                    "tick": 46080,
                    "dest": null,
                    "group_channel": null,
                    "mute": false,
                    "body": {
                      "type": "note",
                      "pitch": 38,
                      "velocity": 40000,
                      "release_velocity": 0,
                      "duration": 7680,
                      "attribute": null,
                      "per_note": []
                    }
                  }
                ]
              }
            ],
            "active_take": 0
          }
        ]
      }
    ],
    "counters": {
      "seq": 1,
      "lane": 1,
      "event": 5,
      "dest": 2,
      "output": 0,
      "media": 0,
      "take": 1,
      "drum_row": 3
    }
  }
}
```

`crates/core-model/tests/fixtures/minimal.json`:

```json
{
  "format": "vizdaw-project",
  "version": 1,
  "document": {
    "settings": {
      "history_depth": 200,
      "autosave_keep": 10,
      "templates": {}
    },
    "instruments": [],
    "outputs": [],
    "media": [],
    "sequences": [
      {
        "id": 0,
        "name": "Song",
        "kind": "normal",
        "keyboard_slot": null,
        "tempo": {
          "points": [
            {
              "tick": 0,
              "bpm": 120000,
              "ramp": false
            }
          ]
        },
        "meter": {
          "entries": [
            {
              "tick": 0,
              "meter": {
                "num": 4,
                "den": 4
              },
              "key": {
                "sharps": 0,
                "minor": false
              }
            }
          ]
        },
        "markers": [],
        "comment": "",
        "start_offset": 0,
        "length": "derived",
        "lanes": [
          {
            "id": 0,
            "name": "Lane 1",
            "midi_dest": null,
            "audio_out": null,
            "mute": false,
            "solo": false,
            "arm": false,
            "length": null,
            "looped": false,
            "length_locked": false,
            "play_quantize": null,
            "play_shift": 0,
            "drum": null,
            "takes": [
              {
                "id": 0,
                "name": "Take 1",
                "events": [
                  {
                    "id": 0,
                    "tick": 0,
                    "dest": null,
                    "group_channel": null,
                    "mute": false,
                    "body": {
                      "type": "note",
                      "pitch": 60,
                      "velocity": 40000,
                      "release_velocity": 0,
                      "duration": 7680,
                      "attribute": null,
                      "per_note": []
                    }
                  }
                ]
              }
            ],
            "active_take": 0
          }
        ]
      }
    ],
    "counters": {
      "seq": 1,
      "lane": 1,
      "event": 1,
      "dest": 0,
      "output": 0,
      "media": 0,
      "take": 1,
      "drum_row": 0
    }
  }
}
```

`crates/core-model/tests/fixtures/mixed_lanes.json`:

```json
{
  "format": "vizdaw-project",
  "version": 1,
  "document": {
    "settings": {
      "history_depth": 200,
      "autosave_keep": 10,
      "templates": {}
    },
    "instruments": [
      {
        "id": 0,
        "name": "Piano",
        "kind": {
          "plugin_instrument": {
            "plugin_ref": "au:piano"
          }
        },
        "default_patch": {
          "program": 0,
          "bank": null
        }
      }
    ],
    "outputs": [
      {
        "id": 0,
        "name": "Main",
        "reference": "bus:main"
      }
    ],
    "media": [
      {
        "id": 0,
        "path": "Media/vox.wav",
        "sample_rate": 48000,
        "channels": 1,
        "length_frames": 96000,
        "content_hash": null
      }
    ],
    "sequences": [
      {
        "id": 0,
        "name": "Mixed",
        "kind": "normal",
        "keyboard_slot": null,
        "tempo": {
          "points": [
            {
              "tick": 0,
              "bpm": 120000,
              "ramp": false
            }
          ]
        },
        "meter": {
          "entries": [
            {
              "tick": 0,
              "meter": {
                "num": 4,
                "den": 4
              },
              "key": {
                "sharps": 0,
                "minor": false
              }
            }
          ]
        },
        "markers": [],
        "comment": "",
        "start_offset": 0,
        "length": "derived",
        "lanes": [
          {
            "id": 0,
            "name": "Lane 1",
            "midi_dest": 0,
            "audio_out": 0,
            "mute": false,
            "solo": false,
            "arm": false,
            "length": null,
            "looped": false,
            "length_locked": false,
            "play_quantize": null,
            "play_shift": 0,
            "drum": null,
            "takes": [
              {
                "id": 0,
                "name": "Take 1",
                "events": [
                  {
                    "id": 0,
                    "tick": 0,
                    "dest": null,
                    "group_channel": null,
                    "mute": false,
                    "body": {
                      "type": "note",
                      "pitch": 60,
                      "velocity": 40000,
                      "release_velocity": 0,
                      "duration": 7680,
                      "attribute": null,
                      "per_note": []
                    }
                  },
                  {
                    "id": 1,
                    "tick": 0,
                    "dest": null,
                    "group_channel": null,
                    "mute": false,
                    "body": {
                      "type": "cc",
                      "index": 64,
                      "value": 4294967295
                    }
                  },
                  {
                    "id": 2,
                    "tick": 15360,
                    "dest": null,
                    "group_channel": null,
                    "mute": false,
                    "body": {
                      "type": "audio_region",
                      "media": 0,
                      "source_offset": 0,
                      "length": 48000,
                      "gain_db": -3.0,
                      "fade_in": {
                        "length": 960,
                        "shape": "linear"
                      },
                      "fade_out": {
                        "length": 0,
                        "shape": "linear"
                      }
                    }
                  }
                ]
              }
            ],
            "active_take": 0
          }
        ]
      }
    ],
    "counters": {
      "seq": 1,
      "lane": 1,
      "event": 3,
      "dest": 1,
      "output": 1,
      "media": 1,
      "take": 1,
      "drum_row": 0
    }
  }
}
```

`crates/core-model/tests/fixtures/nested_references.json`:

```json
{
  "format": "vizdaw-project",
  "version": 1,
  "document": {
    "settings": {
      "history_depth": 200,
      "autosave_keep": 10,
      "templates": {}
    },
    "instruments": [
      {
        "id": 0,
        "name": "Synth",
        "kind": {
          "midi_endpoint": {
            "endpoint_ref": "ep-1",
            "group": 0,
            "channel": 2
          }
        },
        "default_patch": null
      }
    ],
    "outputs": [],
    "media": [],
    "sequences": [
      {
        "id": 0,
        "name": "Song",
        "kind": "normal",
        "keyboard_slot": null,
        "tempo": {
          "points": [
            {
              "tick": 0,
              "bpm": 120000,
              "ramp": false
            }
          ]
        },
        "meter": {
          "entries": [
            {
              "tick": 0,
              "meter": {
                "num": 4,
                "den": 4
              },
              "key": {
                "sharps": 0,
                "minor": false
              }
            }
          ]
        },
        "markers": [],
        "comment": "",
        "start_offset": 0,
        "length": "derived",
        "lanes": [
          {
            "id": 0,
            "name": "Lane 1",
            "midi_dest": null,
            "audio_out": null,
            "mute": false,
            "solo": false,
            "arm": false,
            "length": null,
            "looped": false,
            "length_locked": false,
            "play_quantize": null,
            "play_shift": 0,
            "drum": null,
            "takes": [
              {
                "id": 0,
                "name": "Take 1",
                "events": [
                  {
                    "id": 3,
                    "tick": 0,
                    "dest": null,
                    "group_channel": null,
                    "mute": false,
                    "body": {
                      "type": "reference",
                      "seq": 1,
                      "length": null,
                      "loop_count": {
                        "count": 1
                      },
                      "transpose": 0
                    }
                  },
                  {
                    "id": 4,
                    "tick": 61440,
                    "dest": null,
                    "group_channel": null,
                    "mute": false,
                    "body": {
                      "type": "reference",
                      "seq": 1,
                      "length": null,
                      "loop_count": {
                        "count": 1
                      },
                      "transpose": 5
                    }
                  }
                ]
              }
            ],
            "active_take": 0
          }
        ]
      },
      {
        "id": 1,
        "name": "Verse",
        "kind": "normal",
        "keyboard_slot": null,
        "tempo": {
          "points": [
            {
              "tick": 0,
              "bpm": 120000,
              "ramp": false
            }
          ]
        },
        "meter": {
          "entries": [
            {
              "tick": 0,
              "meter": {
                "num": 4,
                "den": 4
              },
              "key": {
                "sharps": 0,
                "minor": false
              }
            }
          ]
        },
        "markers": [],
        "comment": "",
        "start_offset": 0,
        "length": "derived",
        "lanes": [
          {
            "id": 1,
            "name": "Lane 1",
            "midi_dest": null,
            "audio_out": null,
            "mute": false,
            "solo": false,
            "arm": false,
            "length": null,
            "looped": false,
            "length_locked": false,
            "play_quantize": null,
            "play_shift": 0,
            "drum": null,
            "takes": [
              {
                "id": 1,
                "name": "Take 1",
                "events": [
                  {
                    "id": 2,
                    "tick": 0,
                    "dest": null,
                    "group_channel": null,
                    "mute": false,
                    "body": {
                      "type": "reference",
                      "seq": 2,
                      "length": 30720,
                      "loop_count": {
                        "count": 2
                      },
                      "transpose": 0
                    }
                  }
                ]
              }
            ],
            "active_take": 1
          }
        ]
      },
      {
        "id": 2,
        "name": "Riff",
        "kind": "normal",
        "keyboard_slot": null,
        "tempo": {
          "points": [
            {
              "tick": 0,
              "bpm": 120000,
              "ramp": false
            }
          ]
        },
        "meter": {
          "entries": [
            {
              "tick": 0,
              "meter": {
                "num": 4,
                "den": 4
              },
              "key": {
                "sharps": 0,
                "minor": false
              }
            }
          ]
        },
        "markers": [],
        "comment": "",
        "start_offset": 0,
        "length": "derived",
        "lanes": [
          {
            "id": 2,
            "name": "Lane 1",
            "midi_dest": 0,
            "audio_out": null,
            "mute": false,
            "solo": false,
            "arm": false,
            "length": null,
            "looped": false,
            "length_locked": false,
            "play_quantize": null,
            "play_shift": 0,
            "drum": null,
            "takes": [
              {
                "id": 2,
                "name": "Take 1",
                "events": [
                  {
                    "id": 0,
                    "tick": 0,
                    "dest": null,
                    "group_channel": null,
                    "mute": false,
                    "body": {
                      "type": "note",
                      "pitch": 48,
                      "velocity": 40000,
                      "release_velocity": 0,
                      "duration": 7680,
                      "attribute": null,
                      "per_note": []
                    }
                  },
                  {
                    "id": 1,
                    "tick": 15360,
                    "dest": null,
                    "group_channel": null,
                    "mute": false,
                    "body": {
                      "type": "note",
                      "pitch": 50,
                      "velocity": 40000,
                      "release_velocity": 0,
                      "duration": 7680,
                      "attribute": null,
                      "per_note": []
                    }
                  }
                ]
              }
            ],
            "active_take": 2
          }
        ]
      }
    ],
    "counters": {
      "seq": 3,
      "lane": 3,
      "event": 5,
      "dest": 1,
      "output": 0,
      "media": 0,
      "take": 3,
      "drum_row": 0
    }
  }
}
```

`crates/core-model/tests/snapshots/file__drum_lane.snap`:

```text
---
source: crates/core-model/tests/file.rs
expression: summary(&d)
---
{
  "flattened": [
    "0 note 36 dest=Some(0) gc=Some((0, 9)) out=None",
    "15360 note 38 dest=Some(0) gc=Some((0, 9)) out=None",
    "15360 note 70 dest=Some(1) gc=Some((0, 0)) out=None",
    "30720 note 36 dest=Some(0) gc=Some((0, 9)) out=None",
    "46080 note 38 dest=Some(0) gc=Some((0, 9)) out=None"
  ],
  "lengths": [
    [
      "Beat",
      61440
    ]
  ]
}
```

`crates/core-model/tests/snapshots/file__minimal.snap`:

```text
---
source: crates/core-model/tests/file.rs
expression: summary(&d)
---
{
  "flattened": [
    "0 note 60 dest=None gc=None out=None"
  ],
  "lengths": [
    [
      "Song",
      61440
    ]
  ]
}
```

`crates/core-model/tests/snapshots/file__mixed_lanes.snap`:

```text
---
source: crates/core-model/tests/file.rs
expression: summary(&d)
---
{
  "flattened": [
    "0 note 60 dest=Some(0) gc=Some((0, 0)) out=None",
    "0 Cc dest=Some(0) gc=Some((0, 0)) out=None",
    "15360 AudioRegion dest=None gc=None out=Some(0)"
  ],
  "lengths": [
    [
      "Mixed",
      61440
    ]
  ]
}
```

`crates/core-model/tests/snapshots/file__nested_references.snap`:

```text
---
source: crates/core-model/tests/file.rs
expression: summary(&d)
---
{
  "flattened": [
    "0 note 48 dest=Some(0) gc=Some((0, 2)) out=None",
    "15360 note 50 dest=Some(0) gc=Some((0, 2)) out=None",
    "30720 note 48 dest=Some(0) gc=Some((0, 2)) out=None",
    "46080 note 50 dest=Some(0) gc=Some((0, 2)) out=None",
    "61440 note 53 dest=Some(0) gc=Some((0, 2)) out=None",
    "76800 note 55 dest=Some(0) gc=Some((0, 2)) out=None",
    "92160 note 53 dest=Some(0) gc=Some((0, 2)) out=None",
    "107520 note 55 dest=Some(0) gc=Some((0, 2)) out=None"
  ],
  "lengths": [
    [
      "Song",
      122880
    ],
    [
      "Verse",
      61440
    ],
    [
      "Riff",
      61440
    ]
  ]
}
```


- [ ] **Step 3: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test file
```

Expected: FAIL to compile — `error[E0432]: unresolved import `core_model::file``.

- [ ] **Step 4: Implement**

Create `crates/core-model/src/file.rs`:

```rust
//! The `.vizdaw` package: `project.json`, `Media/` and `Autosave/`.
//!
//! `project.json` is `{ "format": "vizdaw-project", "version": 1,
//! "document": { … } }`. Loading migrates older versions step by step,
//! refuses newer ones, rejects unknown fields and runs every integrity
//! check. Saving writes a temporary file, syncs it, renames it over the old
//! file and syncs the directory.

use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::history::Timestamp;
use crate::rules::validate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The `format` tag.
pub const FORMAT: &str = "vizdaw-project";
/// The version this build writes.
pub const VERSION: u64 = 1;
/// The oldest version this build can migrate from.
pub const FIRST_VERSION: u64 = 1;
/// The project file name inside a package.
pub const PROJECT_FILE: &str = "project.json";

/// A migration from version N to N+1, on raw JSON.
pub type Migration = fn(Value) -> Result<Value>;

/// Migrations indexed by `from - FIRST_VERSION`. Version 1 is the first
/// format, so the table is empty; add `migrate_v1_to_v2` here when version
/// 2 is introduced.
pub const MIGRATIONS: &[Migration] = &[];

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
```

Replace the whole of `crates/core-model/src/lib.rs` with:

```rust
//! `core-model`: the VizDAW project model.
//!
//! A pure-Rust crate holding a VizDAW project and every edit that can be made
//! to it. It has no audio, MIDI I/O, UI or OS dependencies. All edits are
//! [`command::Command`] values applied by the pure function
//! [`command::apply`]; documents are immutable values that share structure.
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod command;
pub mod document;
pub mod error;
pub mod event;
pub mod file;
pub mod history;
pub mod ids;
pub mod lane;
pub mod meter;
pub mod modify;
pub mod query;
pub mod rule;
pub mod rules;
pub mod selection;
pub mod sequence;
pub mod tempo;
pub mod time;

pub use error::{ModelError, Result};
```


- [ ] **Step 5: Run the test to verify it passes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test file
```

Expected: PASS — `file`: `test result: ok. 10 passed`.

- [ ] **Step 6: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/file.rs crates/core-model/src/lib.rs crates/core-model/tests/file.rs crates/core-model/tests/fixtures/drum_lane.json crates/core-model/tests/fixtures/minimal.json crates/core-model/tests/fixtures/mixed_lanes.json crates/core-model/tests/fixtures/nested_references.json crates/core-model/tests/snapshots/file__drum_lane.snap crates/core-model/tests/snapshots/file__minimal.snap crates/core-model/tests/snapshots/file__mixed_lanes.snap crates/core-model/tests/snapshots/file__nested_references.snap
git commit -m "feat(core-model): project file, autosave and golden fixtures"
```

---

### Task 17: Property tests over generated command sequences

**Goal:** Spec §8 properties on generated documents and command sequences: undo N, redo, save/load, no cycles, use counts vs brute force, flatten vs a naive implementation, private copy/flatten keep playback. (Tick ↔ bar·beat·unit ↔ seconds round trips are already pinned in Tasks 2–3.)

**Files:**
- Create (test): `crates/core-model/tests/properties.rs`

**Interfaces:**
- Consumes: Everything public: commands, `History`, `file::{to_json, from_json}`, `query::{flatten, seq_length, use_counts}`, `rules::{find_cycle, validate}`.
- Produces:
  Tests only.

- [ ] **Step 1: Write the failing test**

`crates/core-model/tests/properties.rs`:

```rust
//! Property tests over generated documents and command sequences.

use core_model::command::{
    Command, EventCmd, LaneCmd, ReferenceCmd, RoutingCmd, SequenceCmd, apply,
};
use core_model::document::{DestKind, Document};
use core_model::event::{EventBody, EventSpec, LoopCount};
use core_model::file::{from_json, to_json};
use core_model::history::History;
use core_model::ids::{DestId, EventId, LaneId, SeqId};
use core_model::lane::Lane;
use core_model::query::{flatten, seq_length, use_counts};
use core_model::rules::{find_cycle, validate};
use core_model::selection::ViewFilter;
use core_model::sequence::Sequence;
use core_model::time::{PPQ, Tick};
use proptest::prelude::*;
use serde_json::Value;
use std::collections::BTreeMap;

const Q: i64 = PPQ;

/// An abstract edit; indices are resolved modulo the current ID lists so
/// every generated op names something that exists.
#[derive(Debug, Clone)]
enum Op {
    NewSeq,
    AddLane(usize),
    Note {
        seq: usize,
        lane: usize,
        beat: i64,
        pitch: u8,
        beats: i64,
    },
    Ref {
        seq: usize,
        lane: usize,
        target: usize,
        beat: i64,
        len: Option<i64>,
        loops: u32,
        fill: bool,
        transpose: i8,
    },
    RePoint {
        ev: usize,
        target: usize,
    },
    Delete {
        ev: usize,
    },
    Move {
        ev: usize,
        beats: i64,
    },
    DeleteSeq {
        seq: usize,
        confirm: bool,
    },
    PrivateCopy {
        ev: usize,
    },
    Flatten {
        ev: usize,
    },
    LaneLoop {
        seq: usize,
        lane: usize,
        beats: i64,
        looped: bool,
    },
    LaneMute {
        seq: usize,
        lane: usize,
        mute: bool,
        solo: bool,
    },
    SetDest {
        seq: usize,
        lane: usize,
        dest: bool,
    },
    Transpose {
        ev: usize,
        semis: i8,
    },
}

fn arb_op() -> impl Strategy<Value = Op> {
    let i = || 0usize..8;
    prop_oneof![
        1 => Just(Op::NewSeq),
        1 => i().prop_map(Op::AddLane),
        6 => (i(), i(), 0i64..16, 0u8..=127, 1i64..4)
            .prop_map(|(seq, lane, beat, pitch, beats)| Op::Note { seq, lane, beat, pitch, beats }),
        4 => (i(), i(), i(), 0i64..16, prop::option::of(1i64..12), 1u32..4, any::<bool>(), -24i8..24)
            .prop_map(|(seq, lane, target, beat, len, loops, fill, transpose)| Op::Ref { seq, lane, target, beat, len, loops, fill, transpose }),
        2 => (i(), i()).prop_map(|(ev, target)| Op::RePoint { ev, target }),
        2 => i().prop_map(|ev| Op::Delete { ev }),
        2 => (i(), -4i64..4).prop_map(|(ev, beats)| Op::Move { ev, beats }),
        1 => (i(), any::<bool>()).prop_map(|(seq, confirm)| Op::DeleteSeq { seq, confirm }),
        1 => i().prop_map(|ev| Op::PrivateCopy { ev }),
        1 => i().prop_map(|ev| Op::Flatten { ev }),
        1 => (i(), i(), 1i64..8, any::<bool>()).prop_map(|(seq, lane, beats, looped)| Op::LaneLoop { seq, lane, beats, looped }),
        1 => (i(), i(), any::<bool>(), any::<bool>()).prop_map(|(seq, lane, mute, solo)| Op::LaneMute { seq, lane, mute, solo }),
        1 => (i(), i(), any::<bool>()).prop_map(|(seq, lane, dest)| Op::SetDest { seq, lane, dest }),
        1 => (i(), -12i8..12).prop_map(|(ev, semis)| Op::Transpose { ev, semis }),
    ]
}

fn pick<T: Copy>(v: &[T], i: usize) -> Option<T> {
    if v.is_empty() {
        None
    } else {
        v.get(i % v.len()).copied()
    }
}

fn seqs(d: &Document) -> Vec<SeqId> {
    d.sequences().keys().copied().collect()
}

fn lane_of(d: &Document, s: SeqId, i: usize) -> Option<LaneId> {
    let ids: Vec<LaneId> = d.sequence(s).ok()?.lanes.iter().map(|l| l.id).collect();
    pick(&ids, i)
}

fn events(d: &Document, refs_only: bool) -> Vec<EventId> {
    d.all_events()
        .filter(|(_, e)| !refs_only || matches!(e.body, EventBody::Reference { .. }))
        .map(|(_, e)| e.id)
        .collect()
}

/// Turns an abstract op into a concrete command, if it can name targets.
fn concrete(d: &Document, op: &Op) -> Option<Command> {
    let s = seqs(d);
    Some(match op.clone() {
        Op::NewSeq => Command::Sequence(SequenceCmd::New { name: "gen".into() }),
        Op::AddLane(i) => Command::Lane(LaneCmd::Add {
            seq: pick(&s, i)?,
            name: "l".into(),
            index: None,
        }),
        Op::Note {
            seq,
            lane,
            beat,
            pitch,
            beats,
        } => {
            let seq = pick(&s, seq)?;
            Command::Event(EventCmd::Insert {
                seq,
                lane: lane_of(d, seq, lane)?,
                events: vec![EventSpec::new(
                    Tick(beat * Q),
                    EventBody::note(pitch, 100, Tick(beats * Q)),
                )],
            })
        }
        Op::Ref {
            seq,
            lane,
            target,
            beat,
            len,
            loops,
            fill,
            transpose,
        } => {
            let seq = pick(&s, seq)?;
            Command::Reference(ReferenceCmd::Insert {
                seq,
                lane: lane_of(d, seq, lane)?,
                tick: Tick(beat * Q),
                target: pick(&s, target)?,
                length: len.map(|b| Tick(b * Q)),
                loop_count: if fill {
                    LoopCount::Fill
                } else {
                    LoopCount::Count(loops)
                },
                transpose,
            })
        }
        Op::RePoint { ev, target } => Command::Reference(ReferenceCmd::RePoint {
            event: pick(&events(d, true), ev)?,
            target: pick(&s, target)?,
        }),
        Op::Delete { ev } => Command::Event(EventCmd::Delete {
            events: vec![pick(&events(d, false), ev)?],
        }),
        Op::Move { ev, beats } => Command::Event(EventCmd::Move {
            events: vec![pick(&events(d, false), ev)?],
            delta: Tick(beats * Q),
            to_lane: None,
        }),
        Op::DeleteSeq { seq, confirm } => Command::Sequence(SequenceCmd::Delete {
            seq: pick(&s, seq)?,
            delete_references: confirm,
        }),
        Op::PrivateCopy { ev } => Command::Reference(ReferenceCmd::MakePrivateCopy {
            event: pick(&events(d, true), ev)?,
        }),
        Op::Flatten { ev } => Command::Reference(ReferenceCmd::Flatten {
            event: pick(&events(d, true), ev)?,
        }),
        Op::LaneLoop {
            seq,
            lane,
            beats,
            looped,
        } => {
            let seq = pick(&s, seq)?;
            Command::Lane(LaneCmd::SetLength {
                seq,
                lane: lane_of(d, seq, lane)?,
                length: Some(Tick(beats * Q)),
                looped,
                locked: false,
            })
        }
        Op::LaneMute {
            seq,
            lane,
            mute,
            solo,
        } => {
            let seq = pick(&s, seq)?;
            Command::Lane(LaneCmd::SetFlags {
                seq,
                lane: lane_of(d, seq, lane)?,
                mute: Some(mute),
                solo: Some(solo),
                arm: None,
            })
        }
        Op::SetDest { seq, lane, dest } => {
            let seq = pick(&s, seq)?;
            Command::Lane(LaneCmd::SetDest {
                seq,
                lane: lane_of(d, seq, lane)?,
                dest: dest.then_some(DestId(0)),
            })
        }
        Op::Transpose { ev, semis } => Command::Event(EventCmd::Nudge {
            events: vec![pick(&events(d, false), ev)?],
            ticks: Tick(0),
            semitones: semis,
        }),
    })
}

/// Two sequences and one MIDI destination to start from.
fn seed() -> Document {
    let mut d = Document::new();
    for c in [
        Command::Routing(RoutingCmd::AddDestination {
            name: "d".into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: "e".into(),
                group: 1,
                channel: 2,
            },
            default_patch: None,
        }),
        Command::Sequence(SequenceCmd::New { name: "A".into() }),
        Command::Sequence(SequenceCmd::New { name: "B".into() }),
    ] {
        d = apply(&d, &c).unwrap().doc;
    }
    d
}

/// Applies the ops through a History, skipping refused commands. Returns the
/// history and the number of applied steps.
fn build(ops: &[Op]) -> (History, usize) {
    let mut h = History::new(seed());
    let mut n = 0;
    for op in ops {
        if let Some(cmd) = concrete(h.doc(), op) {
            if h.execute(&cmd, &ViewFilter::default()).is_ok() {
                n += 1;
            }
        }
    }
    (h, n)
}

fn ops() -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(arb_op(), 1..40)
}

/// Counts reference events per target by walking the serialized JSON.
fn brute_use_counts(d: &Document) -> BTreeMap<u64, usize> {
    fn walk(v: &Value, out: &mut BTreeMap<u64, usize>) {
        match v {
            Value::Object(m) => {
                if m.get("type").and_then(Value::as_str) == Some("reference") {
                    if let Some(s) = m.get("seq").and_then(Value::as_u64) {
                        *out.entry(s).or_insert(0) += 1;
                    }
                }
                m.values().for_each(|x| walk(x, out));
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out: BTreeMap<u64, usize> = d.sequences().keys().map(|k| (k.0, 0)).collect();
    walk(&serde_json::to_value(d).unwrap(), &mut out);
    out
}

/// Naive reference implementation of flatten: expand every pass of every
/// reference recursively, then filter by window, without pruning.
fn naive(
    d: &Document,
    s: &Sequence,
    offset: i64,
    lo: i64,
    hi: i64,
    tr: i32,
    out: &mut Vec<String>,
) {
    let any_solo = s.lanes.iter().any(|l| l.solo);
    for lane in s.lanes.iter().filter(|l| !l.mute && (!any_solo || l.solo)) {
        let Some(take) = lane.active() else { continue };
        let (period, limit) = match lane.length {
            Some(l) if lane.looped => (Some(l.0), l.0),
            Some(l) => (None, l.0),
            None => (None, i64::MAX),
        };
        let passes = match period {
            Some(p) => (hi.max(0) + p - 1) / p,
            None => 1,
        };
        for k in 0..passes.max(1) {
            let pass_end = (k * period.unwrap_or(0))
                .saturating_add(limit)
                .saturating_add(lane.play_shift.0);
            let hi = hi.min(pass_end);
            let lo = match period {
                Some(p) => lo.max(k * p + lane.play_shift.0),
                None => lo,
            };
            for e in take.events.values() {
                if e.mute || e.tick.0 >= limit {
                    continue;
                }
                let lt = e.tick.0 + k * period.unwrap_or(0) + lane.play_shift.0;
                let is_ref = matches!(e.body, EventBody::Reference { .. });
                if (lt < lo && !is_ref) || lt >= hi {
                    continue;
                }
                naive_event(d, lane, e, lt, offset, lo, hi, tr, out);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn naive_event(
    d: &Document,
    lane: &Lane,
    e: &core_model::event::Event,
    lt: i64,
    offset: i64,
    win_lo: i64,
    hi: i64,
    tr: i32,
    out: &mut Vec<String>,
) {
    match &e.body {
        EventBody::Reference {
            seq,
            length,
            loop_count,
            transpose,
        } => {
            let Ok(t) = d.sequence(*seq) else { return };
            let b = seq_length(d, *seq).unwrap().0;
            let use_len = length.map_or(b, |l| l.0);
            let (iter, total) = match loop_count {
                LoopCount::Count(n) => (use_len, use_len * i64::from(*n)),
                LoopCount::Fill => (b, use_len),
            };
            let end = (lt + total).min(hi);
            let mut start = lt;
            let mut first = true;
            while start < end {
                let lo = (if first { -t.start_offset.0 } else { 0 }).max(win_lo - start);
                naive(
                    d,
                    t,
                    offset + start,
                    lo,
                    iter.min(end - start),
                    tr + i32::from(*transpose),
                    out,
                );
                start += iter;
                first = false;
            }
        }
        EventBody::Note {
            pitch, duration, ..
        } => {
            let p = (i32::from(*pitch) + tr).clamp(0, 127);
            let dur = duration.0.min(hi - lt);
            let dest = e.dest.or(lane.midi_dest).map(|x| x.0);
            out.push(format!("{} note {p} {dur} {dest:?}", offset + lt));
        }
        _ => {}
    }
}

fn played(d: &Document, s: SeqId) -> Vec<String> {
    let mut v: Vec<String> = flatten(d, s, None, &ViewFilter::default())
        .unwrap()
        .filter_map(|r| match r.body {
            EventBody::Note {
                pitch, duration, ..
            } => Some(format!(
                "{} note {pitch} {} {:?}",
                r.tick.0,
                duration.0,
                r.dest.map(|x| x.0)
            )),
            _ => None,
        })
        .collect();
    v.sort();
    v
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn apply_n_then_undo_n_restores_the_original(ops in ops()) {
        let (mut h, n) = build(&ops);
        let after = h.doc().clone();
        for _ in 0..n { h.undo().unwrap(); }
        prop_assert_eq!(h.doc(), &seed());
        prop_assert!(h.undo().is_err());
        for _ in 0..n { h.redo().unwrap(); }
        prop_assert_eq!(h.doc(), &after);
    }

    #[test]
    fn redo_after_undo_returns_the_same_document(ops in ops()) {
        let (mut h, n) = build(&ops);
        prop_assume!(n > 0);
        let after = h.doc().clone();
        h.undo().unwrap();
        h.redo().unwrap();
        prop_assert_eq!(h.doc(), &after);
    }

    #[test]
    fn save_then_load_is_identical(ops in ops()) {
        let (h, _) = build(&ops);
        let text = to_json(h.doc()).unwrap();
        prop_assert_eq!(&from_json(&text).unwrap(), h.doc());
    }

    #[test]
    fn no_command_sequence_creates_a_cycle(ops in ops()) {
        let (h, _) = build(&ops);
        prop_assert_eq!(find_cycle(h.doc()), None);
        prop_assert!(validate(h.doc()).is_empty(), "{:?}", validate(h.doc()));
    }

    #[test]
    fn use_counts_match_a_brute_force_recount(ops in ops()) {
        let (h, _) = build(&ops);
        let derived: BTreeMap<u64, usize> = use_counts(h.doc()).into_iter().map(|(k, v)| (k.0, v)).collect();
        prop_assert_eq!(derived, brute_use_counts(h.doc()));
    }

    #[test]
    fn flatten_matches_the_naive_implementation(ops in ops()) {
        let (h, _) = build(&ops);
        let d = h.doc();
        for s in d.sequences().values() {
            let mut expected = Vec::new();
            let len = seq_length(d, s.id).unwrap().0;
            naive(d, s, 0, -s.start_offset.0, len, 0, &mut expected);
            expected.sort();
            prop_assert_eq!(played(d, s.id), expected);
        }
    }

    #[test]
    fn private_copy_and_flatten_keep_playback(ops in ops(), pick_ev in 0usize..8) {
        let (h, _) = build(&ops);
        let d = h.doc();
        let refs = events(d, true);
        prop_assume!(!refs.is_empty());
        let ev = refs[pick_ev % refs.len()];
        let owner = d.locate(ev).unwrap().seq;
        let before = played(d, owner);
        // Flatten makes destinations explicit; events that had none follow
        // the owning lane afterwards (and the report warns), so compare
        // without destinations in that case.
        let strip = |v: Vec<String>| -> Vec<String> {
            v.into_iter().map(|x| x.rsplit_once(' ').map_or(x.clone(), |(a, _)| a.to_string())).collect()
        };
        let unresolved = before.iter().any(|x| x.ends_with("None"));
        for cmd in [ReferenceCmd::MakePrivateCopy { event: ev }, ReferenceCmd::Flatten { event: ev }] {
            let a = apply(d, &Command::Reference(cmd)).unwrap();
            let after = played(&a.doc, owner);
            if unresolved {
                prop_assert_eq!(strip(after), strip(before.clone()));
            } else {
                prop_assert_eq!(after, before.clone());
            }
            prop_assert!(validate(&a.doc).is_empty());
        }
    }
}
```


- [ ] **Step 2: Run the property tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test properties
```

Expected: PASS — `properties`: `test result: ok. 7 passed`. These tests exercise code from earlier tasks; a failure here is a real bug. Shrink it with the printed minimal input, reproduce it as a unit test in the owning task's test file, fix the owning module, and re-run. Then run them a few more times (`for i in 1 2 3 4 5; do cargo test -p core-model --release --test properties || break; done`) because proptest seeds are random; delete any `proptest-regressions` file proptest writes before committing.

- [ ] **Step 3: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 4: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/tests/properties.rs
git commit -m "test(core-model): property tests for undo, files, cycles, counts and flatten"
```

---

### Task 18: Benchmarks, CI workflow and the final lint gate

**Goal:** Criterion benchmarks for the three §8 targets, a GitHub Actions workflow running fmt/clippy/test on macOS and Linux (written, not run), and removal of the temporary `dead_code` allowance.

**Files:**
- Create: `.github/workflows/ci.yml`
- Modify: `crates/core-model/Cargo.toml`
- Create: `crates/core-model/benches/model.rs`
- Modify: `crates/core-model/src/document.rs`

**Interfaces:**
- Consumes: `apply`, `flatten`, `file::{to_json, from_json}`, `Document::from_parts`.
- Produces:
  `crates/core-model/benches/model.rs` (bench target `model`), `.github/workflows/ci.yml`.

- [ ] **Step 1: Implement**

Create `.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
  pull_request:

env:
  CARGO_TERM_COLOR: always

jobs:
  rust:
    name: ${{ matrix.os }}
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [macos-14, ubuntu-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      - name: Format
        run: cargo fmt --all -- --check
      - name: Clippy
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: Test
        run: cargo test --workspace
      - name: Benchmarks compile
        run: cargo bench --workspace --no-run
```

Edit `crates/core-model/Cargo.toml` exactly as in this diff (only these lines change):

```diff
--- a/crates/core-model/Cargo.toml
+++ b/crates/core-model/Cargo.toml
@@ -15,3 +15,7 @@ thiserror = "2"
 proptest = "1"
 criterion = "0.5"
 insta = { version = "1", features = ["json"] }
+
+[[bench]]
+name = "model"
+harness = false
```

Create `crates/core-model/benches/model.rs`:

```rust
//! Benchmarks for the spec §8 targets:
//! one edit on a 100k-event document < 1 ms; flattening a 64-bar song with
//! three levels of references < 5 ms; loading a 100k-event project < 200 ms.

use core_model::command::{Command, EventCmd, apply};
use core_model::document::{Document, DocumentRepr, Settings};
use core_model::event::{EventBody, EventSpec, LoopCount};
use core_model::file::{from_json, to_json};
use core_model::ids::{Counters, EventId, LaneId, SeqId, TakeId};
use core_model::lane::Lane;
use core_model::query::flatten;
use core_model::selection::ViewFilter;
use core_model::sequence::Sequence;
use core_model::time::{PPQ, Tick};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use im::Vector;

/// One sequence with 10 lanes of 10 000 sixteenth notes each.
fn big_doc() -> Document {
    let mut lanes = Vector::new();
    let mut next = 0u64;
    for l in 0..10u64 {
        let mut lane = Lane::new(LaneId(l), TakeId(l), format!("L{l}"));
        let take = lane.takes.get_mut(0).unwrap();
        for i in 0..10_000i64 {
            let ev = EventSpec::new(
                Tick(i * PPQ / 4),
                EventBody::note((i % 88) as u8 + 21, 30_000, Tick(PPQ / 8)),
            )
            .with_id(EventId(next));
            next += 1;
            take.events.insert(ev.key(), ev);
        }
        lanes.push_back(lane);
    }
    Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![],
        outputs: vec![],
        media: vec![],
        sequences: vec![Sequence::new(SeqId(0), "Big", lanes)],
        counters: Counters {
            seq: 1,
            lane: 10,
            take: 10,
            event: next,
            ..Counters::default()
        },
    })
    .unwrap()
}

/// Song (64 bars) -> 8 x Verse (8 bars) -> 4 x Riff (2 bars, 2 lanes of
/// 16th notes); each level also has a lane of its own notes.
fn nested_song() -> Document {
    let mut next = 0u64;
    let mut ev = |t: i64, body: EventBody| {
        let e = EventSpec::new(Tick(t), body).with_id(EventId(next));
        next += 1;
        e
    };
    let bar = PPQ * 4;
    let notes_lane =
        |id: u64, bars: i64, ev: &mut dyn FnMut(i64, EventBody) -> core_model::event::Event| {
            let mut lane = Lane::new(LaneId(id), TakeId(id), "notes");
            for i in 0..bars * 16 {
                let e = ev(
                    i * PPQ / 4,
                    EventBody::note(60 + (i % 12) as u8, 20_000, Tick(PPQ / 8)),
                );
                lane.takes.get_mut(0).unwrap().events.insert(e.key(), e);
            }
            lane
        };
    let riff = Sequence::new(
        SeqId(2),
        "Riff",
        Vector::from(vec![notes_lane(0, 2, &mut ev), notes_lane(1, 2, &mut ev)]),
    );
    let mut verse_refs = Lane::new(LaneId(2), TakeId(2), "refs");
    for i in 0..4 {
        let e = ev(i * 2 * bar, EventBody::reference(SeqId(2)));
        verse_refs
            .takes
            .get_mut(0)
            .unwrap()
            .events
            .insert(e.key(), e);
    }
    let verse = Sequence::new(
        SeqId(1),
        "Verse",
        Vector::from(vec![verse_refs, notes_lane(3, 8, &mut ev)]),
    );
    let mut song_refs = Lane::new(LaneId(4), TakeId(4), "refs");
    for i in 0..8 {
        let e = ev(
            i * 8 * bar,
            EventBody::Reference {
                seq: SeqId(1),
                length: None,
                loop_count: LoopCount::Count(1),
                transpose: (i % 3) as i8,
            },
        );
        song_refs
            .takes
            .get_mut(0)
            .unwrap()
            .events
            .insert(e.key(), e);
    }
    let song = Sequence::new(
        SeqId(0),
        "Song",
        Vector::from(vec![song_refs, notes_lane(5, 64, &mut ev)]),
    );
    Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![],
        outputs: vec![],
        media: vec![],
        sequences: vec![song, verse, riff],
        counters: Counters {
            seq: 3,
            lane: 6,
            take: 6,
            event: next,
            ..Counters::default()
        },
    })
    .unwrap()
}

fn benches(c: &mut Criterion) {
    let big = big_doc();
    let edit = Command::Event(EventCmd::SetVelocity {
        events: vec![EventId(54_321)],
        velocity: 1,
    });
    c.bench_function("one edit on 100k events", |b| {
        b.iter(|| apply(black_box(&big), black_box(&edit)).unwrap())
    });

    let song = nested_song();
    let filter = ViewFilter::default();
    c.bench_function("flatten 64 bars, 3 levels", |b| {
        b.iter(|| {
            flatten(black_box(&song), SeqId(0), None, &filter)
                .unwrap()
                .count()
        })
    });

    let text = to_json(&big).unwrap();
    let mut g = c.benchmark_group("load");
    g.sample_size(10);
    g.bench_function("load 100k events", |b| {
        b.iter(|| from_json(black_box(&text)).unwrap())
    });
    g.finish();
}

criterion_group!(model, benches);
criterion_main!(model);
```

Edit `crates/core-model/src/document.rs` exactly as in this diff (only these lines change):

```diff
--- a/crates/core-model/src/document.rs
+++ b/crates/core-model/src/document.rs
@@ -305,9 +305,7 @@ impl Document {
     }
 }
 
-/// Crate-internal mutation, used only by commands. (`dead_code` is allowed
-/// until the command tasks use every function; Task 18 removes the allow.)
-#[allow(dead_code)]
+/// Crate-internal mutation, used only by commands.
 impl Document {
     pub(crate) fn settings_mut(&mut self) -> &mut Settings {
         &mut self.settings
```


- [ ] **Step 2: Run the benchmarks and compare with the spec targets**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo bench -p core-model --bench model
```

Expected (reference machine, Apple Silicon, release): `one edit on 100k events` ≈ 9 µs (target < 1 ms); `flatten 64 bars, 3 levels` ≈ 0.35 ms (target < 5 ms); `load/load 100k events` ≈ 115 ms (target < 200 ms). If load exceeds 200 ms on your machine, profile `from_json` first (the fast path must not build a `serde_json::Value`).

- [ ] **Step 3: Full gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy warnings, every test target `ok`.

- [ ] **Step 4: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add .github/workflows/ci.yml crates/core-model/Cargo.toml crates/core-model/benches/model.rs crates/core-model/src/document.rs
git commit -m "chore(core-model): benchmarks, CI workflow, final lint gate"
```

---

## Self-Review

**Spec coverage** (spec section → task):

| Spec | Requirement | Task |
|---|---|---|
| §2 | edition 2024, deps limited, lint denies, `deny(missing_docs)` | 1 (gates), 18 (final gate) |
| §3 Identity/time | typed IDs, counters, `Tick` 15,360 PPQ, `BarBeatUnit` with bar 0 | 1, 2, 3 |
| §3 Document | settings, instruments, outputs, media, sequences, counters | 4 |
| §3 Sequence/Lane/Event | every field and `EventBody` variant, takes, drum layout, value ranges | 4 (types), 5 (ranges) |
| §4.1 | no cycles on insert/re-point/move | 5, 8, 12 |
| §4.2 | delete-in-use needs confirmation; dest/output/media removal needs replacement | 6, 13 |
| §4.3 | derived use counts, lengths, drum membership, group/channel | 10, 9 |
| §4.4 | routing resolution; unresolved events reported | 10 |
| §4.5 | nested timing (playing sequence's tempo governs) | 11 |
| §4.6 | barline meter changes, 5–999 BPM | 2, 3 |
| §5 Commands | sequences, lanes, events, references, audio, destinations/outputs, select-and-modify | 6, 7, 8, 9, 12, 13, 14 |
| §5 Transactions/Selection/History | begin/commit/abort, single selection, depth limit, Modify Again, templates | 14, 15 |
| §6 Queries | flatten, time conversion, usage, events_matching, drum view | 2, 3, 10, 11, 14 |
| §7 File | package layout, format/version, migrations, NewerFormat, unknown fields, atomic save + fsync, autosave keep N, validation | 16 |
| §8 Errors | single `ModelError` | 1 |
| §8 Property tests | undo N, redo, save/load, no cycles, use counts, time round trips, flatten vs naive | 17 (time round trips in 2, 3) |
| §8 Unit tests | pickups, longer/shorter refs, transpose clamping, private copy vs flatten, routing overrides, drum auto-rows | 3, 10, 11, 12, 9 |
| §8 Golden files | minimal, nested references, mixed lanes, drum lane (insta) | 16 |
| §8 Benchmarks | edit < 1 ms, flatten < 5 ms, load < 200 ms | 18 |
| §8 CI | test, clippy -D warnings, fmt --check on macOS and Linux | 18 |

**Placeholder scan:** every code step contains the complete file or an exact diff; no TBD/TODO.

**Type consistency:** names used across tasks were checked against the code that defines them (e.g. `Walk::reference(…, win: (Tick, Tick), …)` in Task 11 is what Task 12 calls; `edit_events`/`insert_specs`/`ensure_drum_row` from Task 8 are what Tasks 9, 12, 13 and 14 call; `Settings.templates` added in Task 14 is what Task 16's fixtures contain).

**Verification of this plan:** every task's code was compiled and run in a scratch copy of the workspace, one commit per task; at each commit `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` were green.
