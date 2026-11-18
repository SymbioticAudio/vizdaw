//! Private local time records (plan 8f; spec §9): where the composer's
//! time goes, counted in minute blocks per song and category, with
//! friction signals, in a private file under the support folder.
//!
//! Every time is passed in, so nothing here reads the clock. Nothing but
//! the rows Ruling 2 lists is stored: no paths, file names, track,
//! plug-in or request names, and no notes. Plain Unix: it runs on Linux.

pub mod blocks;
pub mod file;
pub mod store;
pub mod summary;

use crate::error::VizError;
use crate::guard::ffi_guard;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use store::{ANOTHER_EDIT, ANOTHER_ITEM, SongKey, Store};

/// The folder under the support folder.
pub const METRICS_DIR: &str = "Metrics";
/// The file in it.
pub const STORE: &str = "time.json";
/// A block's length.
pub const BLOCK_MS: u64 = 60_000;
/// How often the probe samples.
pub const SAMPLE_MS: u64 = 5_000;
/// Undos in one song that make a burst.
pub const UNDO_BURST: u32 = 5;
/// The window those undos fall in.
pub const UNDO_WINDOW_MS: u64 = 60_000;
/// Commits of one label in one song that make a repeat.
pub const REPEAT_COUNT: u32 = 6;
/// The window those commits fall in.
pub const REPEAT_WINDOW_MS: u64 = 600_000;
/// A main-menu session at least this long is a hunt.
pub const HUNT_MS: u64 = 8_000;
/// The most labels of each kind one day holds.
pub const MAX_LABELS: usize = 32;
/// The longest song name kept, in characters.
pub const NAME_MAX: usize = 128;
/// The largest file read.
pub const MAX_STORE_BYTES: u64 = 8 * 1024 * 1024;
/// The most songs a summary shows.
pub const MAX_SONGS_SHOWN: usize = 20;
/// The longest block `open` takes.
const MAX_BLOCK_MS: u64 = 3_600_000;
/// The latest time taken, in ms (later ones are hostile).
const MAX_CLOCK_MS: u64 = 1 << 53;

/// What the composer's time went on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum MetricsCategoryRow {
    /// Writing notes: the piano roll or drum grid with the pencil or step
    /// entry.
    Composing,
    /// Any document recording.
    Recording,
    /// Arranging and editing.
    Editing,
    /// The mixer and plug-ins.
    Mixing,
    /// Settings and the like.
    Other,
}

/// A time, passed in: milliseconds of a monotonic clock, and the local
/// day "YYYY-MM-DD".
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MetricsClockRow {
    /// Milliseconds.
    pub ms: u64,
    /// The local day.
    pub day: String,
}

/// The song in use.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MetricsSongRow {
    /// Its project ID; `None` for a song never saved.
    pub id: Option<String>,
    /// Its own name (the document's title).
    pub name: String,
}

/// Seconds by category.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct MetricsSecondsRow {
    /// Composing.
    pub composing: u64,
    /// Recording.
    pub recording: u64,
    /// Editing.
    pub editing: u64,
    /// Mixing.
    pub mixing: u64,
    /// Other.
    pub other: u64,
}

/// A label and its count.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MetricsCountRow {
    /// A kept label.
    pub label: String,
    /// How many.
    pub count: u32,
}

/// Totals of time and friction.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct MetricsTotalsRow {
    /// Seconds by category.
    pub seconds: MetricsSecondsRow,
    /// Bursts of undos.
    pub undo_bursts: u32,
    /// Repeated tasks, largest first.
    pub repeats: Vec<MetricsCountRow>,
    /// Long menu hunts.
    pub hunts: u32,
    /// What the hunts ended at, largest first.
    pub hunt_ends: Vec<MetricsCountRow>,
}

/// One song's totals.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MetricsSongTotalsRow {
    /// Its name, or "Untitled songs".
    pub name: String,
    /// It is the song asked about.
    pub this_song: bool,
    /// It is every song never saved.
    pub untitled: bool,
    /// Its totals.
    pub totals: MetricsTotalsRow,
}

/// The summary Co-Producer reads (plan 8f Ruling 6).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MetricsSummaryRow {
    /// The first day counted; `None` when nothing is.
    pub since: Option<String>,
    /// The days asked for; `None`: all time.
    pub days: Option<u32>,
    /// Every song together.
    pub all: MetricsTotalsRow,
    /// The most time first, at most [`MAX_SONGS_SHOWN`], always with this
    /// song.
    pub songs: Vec<MetricsSongTotalsRow>,
    /// Songs not shown.
    pub more_songs: u32,
}

/// What Settings shows about the records.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MetricsOverviewRow {
    /// Songs counted, untitled as one.
    pub songs: u32,
    /// The first day counted.
    pub since: Option<String>,
    /// Every second counted.
    pub total_seconds: u64,
}

/// Everything the recorder holds, under one lock.
struct State {
    blocks: blocks::Blocks,
    friction: blocks::Friction,
    /// Counts not yet written.
    pending: Store,
    problem: Option<String>,
    closed: bool,
    /// The song sampled last, which menu hunts belong to.
    last_song: SongKey,
}

/// Counts where the composer's time goes, while counting is on.
#[derive(uniffi::Object)]
pub struct MetricsRecorder {
    support: PathBuf,
    block_ms: u64,
    sample_ms: u64,
    state: Mutex<State>,
}

fn clock_ok(c: &MetricsClockRow) -> bool {
    c.ms <= MAX_CLOCK_MS && summary::is_date(&c.day)
}

impl MetricsRecorder {
    /// Runs `f` on the state, unless the recorder is closed, the clock is
    /// hostile, or the lock is poisoned (the safe default: nothing).
    fn noting(&self, clock: &MetricsClockRow, f: impl FnOnce(&mut State)) {
        let _ = ffi_guard(|| {
            if !clock_ok(clock) {
                return Ok(());
            }
            match self.state.lock() {
                Ok(mut s) if !s.closed => f(&mut s),
                _ => {}
            }
            Ok(())
        });
    }

    /// Merges what is pending into the file and writes it.
    fn write_pending(&self, s: &mut State) -> Result<(), VizError> {
        if s.pending.is_empty() {
            return Ok(());
        }
        let result = (|| {
            let dir =
                file::metrics_dir(&self.support, true, true)?.ok_or_else(|| VizError::Io {
                    message: "The Metrics folder could not be made.".into(),
                })?;
            let mut base = match file::read(&dir)? {
                file::Found::Store(st) => st,
                file::Found::Missing => Store::default(),
                file::Found::Damaged => {
                    let name = file::set_aside(&dir)?;
                    s.problem = Some(file::set_aside_problem(&name));
                    Store::default()
                }
                file::Found::NotAFile => {
                    return Err(VizError::Io {
                        message: "The time records file is not VizDAW's (a link).".into(),
                    });
                }
            };
            base.merge(&s.pending);
            file::write(&dir, &base)
        })();
        match &result {
            Ok(()) => s.pending = Store::default(),
            Err(e) => s.problem = Some(e.to_string()),
        }
        result
    }

    fn fresh(&self) -> (blocks::Blocks, blocks::Friction) {
        (
            blocks::Blocks::new(self.block_ms, self.sample_ms),
            blocks::Friction::default(),
        )
    }
}

#[uniffi::export]
impl MetricsRecorder {
    /// Ruling 5's folder checks and read; `support` is VizDAW's support
    /// folder. A damaged file is set aside (see [`MetricsRecorder::problem`]).
    #[uniffi::constructor]
    pub fn open(support: String, block_ms: u64, sample_ms: u64) -> Result<Arc<Self>, VizError> {
        ffi_guard(|| {
            if sample_ms == 0 || sample_ms > block_ms || block_ms > MAX_BLOCK_MS {
                return Err(VizError::Model {
                    message: "The time counting intervals are out of range.".into(),
                });
            }
            let support = file::support_dir(&support)?;
            let mut problem = None;
            if let Some(dir) = file::metrics_dir(&support, false, true)? {
                file::remove_strays(&dir);
                if let file::Found::Damaged = file::read(&dir)? {
                    problem = Some(file::set_aside_problem(&file::set_aside(&dir)?));
                }
            }
            Ok(Arc::new(MetricsRecorder {
                support,
                block_ms,
                sample_ms,
                state: Mutex::new(State {
                    blocks: blocks::Blocks::new(block_ms, sample_ms),
                    friction: blocks::Friction::default(),
                    pending: Store::default(),
                    problem,
                    closed: false,
                    last_song: SongKey::Untitled,
                }),
            }))
        })
    }

    /// What went wrong with the records, for Settings to show.
    pub fn problem(&self) -> Option<String> {
        ffi_guard(|| Ok(self.state.lock().ok().and_then(|s| s.problem.clone()))).unwrap_or(None)
    }

    /// One sample: `category` `None` is idle. A block that closes with
    /// time or friction pending writes the file.
    pub fn sample(
        &self,
        clock: MetricsClockRow,
        category: Option<MetricsCategoryRow>,
        song: Option<MetricsSongRow>,
    ) {
        self.noting(&clock, |s| {
            let key = SongKey::of(song.as_ref());
            if song.is_some() {
                s.last_song = key.clone();
            }
            let Some(closed) = s.blocks.sample(clock.ms, &clock.day, category, &key) else {
                return;
            };
            if let Some(c) = closed {
                let slot = s.pending.day_mut(&c.song, &c.day).seconds.slot(&c.category);
                *slot = slot.saturating_add(c.seconds).min(store::MAX_DAY_SECONDS);
            }
            let _ = self.write_pending(s);
        });
    }

    /// An undo in `song`.
    pub fn note_undo(&self, clock: MetricsClockRow, song: Option<MetricsSongRow>) {
        self.noting(&clock, |s| {
            let key = SongKey::of(song.as_ref());
            if s.friction.undo(clock.ms, &key) {
                let d = s.pending.day_mut(&key, &clock.day);
                d.undo_bursts = d.undo_bursts.saturating_add(1);
            }
        });
    }

    /// A new step in `song`, with its undo label.
    pub fn note_step(&self, clock: MetricsClockRow, song: Option<MetricsSongRow>, label: String) {
        self.noting(&clock, |s| {
            let Some(label) = store::step_label(&label) else {
                return;
            };
            let key = SongKey::of(song.as_ref());
            if s.friction.step(clock.ms, &key, label) {
                let d = s.pending.day_mut(&key, &clock.day);
                store::add_label(&mut d.repeats, label, 1, ANOTHER_EDIT);
            }
        });
    }

    /// A main-menu session of `duration_ms` that chose `chosen` (its
    /// title), if anything.
    pub fn note_menu(&self, clock: MetricsClockRow, duration_ms: u64, chosen: Option<String>) {
        self.noting(&clock, |s| {
            if duration_ms < HUNT_MS {
                return;
            }
            let label = store::menu_label(chosen.as_deref());
            let key = s.last_song.clone();
            let d = s.pending.day_mut(&key, &clock.day);
            d.hunts = d.hunts.saturating_add(1);
            store::add_label(&mut d.hunt_ends, label, 1, ANOTHER_ITEM);
        });
    }

    /// Writes pending friction and time; keeps the open block.
    pub fn flush(&self) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut s = self.state.lock().map_err(|_| poisoned())?;
            if s.closed {
                return Ok(());
            }
            self.write_pending(&mut s)
        })
    }

    /// Flushes and drops the open block; later calls do nothing.
    pub fn close(&self) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut s = self.state.lock().map_err(|_| poisoned())?;
            if s.closed {
                return Ok(());
            }
            let result = self.write_pending(&mut s);
            s.closed = true;
            (s.blocks, s.friction) = self.fresh();
            s.pending = Store::default();
            result
        })
    }

    /// Drops everything pending: the open block, the friction windows and
    /// counts not yet written (Delete All Time Records…).
    pub fn clear(&self) {
        let _ = ffi_guard(|| {
            if let Ok(mut s) = self.state.lock() {
                (s.blocks, s.friction) = self.fresh();
                s.pending = Store::default();
            }
            Ok(())
        });
    }

    /// Delete All Time Records… while counting is on: drops everything
    /// pending and deletes `<support>/Metrics` under the recorder's own
    /// lock, so no count or write slips between the two. Counting goes on
    /// into a fresh file. With no recorder open, use [`metrics_delete_all`].
    pub fn clear_and_delete_all(&self) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut s = self.state.lock().map_err(|_| poisoned())?;
            (s.blocks, s.friction) = self.fresh();
            s.pending = Store::default();
            file::delete_all(&self.support)
        })
    }
}

fn poisoned() -> VizError {
    VizError::Internal {
        message: "the time records' lock is poisoned".into(),
    }
}

/// The store under `support` for reading only: missing, damaged or a file
/// that is not a file reads as empty, and nothing is moved or changed.
fn read_only(support: &str) -> Result<Store, VizError> {
    let support = file::support_dir(support)?;
    let Some(dir) = file::metrics_dir(&support, false, false)? else {
        return Ok(Store::default());
    };
    Ok(match file::read(&dir)? {
        file::Found::Store(s) => s,
        _ => Store::default(),
    })
}

/// Ruling 6's summary for `days` days ending on `today` ("YYYY-MM-DD";
/// `None`: all time), with `song_id`'s song as "this song". Reads only: a
/// damaged file reads as empty and is not moved.
#[uniffi::export]
pub fn metrics_summary(
    support: String,
    today: String,
    days: Option<u32>,
    song_id: Option<String>,
) -> Result<MetricsSummaryRow, VizError> {
    ffi_guard(|| {
        let Some(today) = summary::parse_date(&today) else {
            return Err(VizError::Model {
                message: "today must be a date, YYYY-MM-DD.".into(),
            });
        };
        if days.is_some_and(|d| !(1..=3650).contains(&d)) {
            return Err(VizError::Model {
                message: "days must be 1 to 3650.".into(),
            });
        }
        let store = read_only(&support)?;
        Ok(summary::summarize(&store, today, days, song_id.as_deref()))
    })
}

/// What Settings shows: songs, since when, and the seconds counted.
#[uniffi::export]
pub fn metrics_overview(support: String) -> Result<MetricsOverviewRow, VizError> {
    ffi_guard(|| Ok(summary::overview(&read_only(&support)?)))
}

/// Deletes every time record: `<support>/Metrics`, never following a link.
#[uniffi::export]
pub fn metrics_delete_all(support: String) -> Result<(), VizError> {
    ffi_guard(|| file::delete_all(&file::support_dir(&support)?))
}
