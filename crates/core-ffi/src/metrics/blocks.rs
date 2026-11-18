//! Blocks and friction (plan 8f Ruling 4). Pure: every time is passed in,
//! and nothing here touches the file system.

use super::store::{ANOTHER_EDIT, SongKey};
use super::{MetricsCategoryRow, REPEAT_COUNT, REPEAT_WINDOW_MS, UNDO_BURST, UNDO_WINDOW_MS};
use std::collections::HashMap;

/// A block that counted: its whole length goes to one song and category.
#[derive(Debug, Clone, PartialEq)]
pub struct Counted {
    /// Whose time.
    pub song: SongKey,
    /// The day of the block's first sample.
    pub day: String,
    /// What the time was spent on.
    pub category: MetricsCategoryRow,
    /// The block's length in seconds.
    pub seconds: u64,
}

/// How many active samples one (song, category) had in the open block.
struct Tally {
    song: SongKey,
    category: MetricsCategoryRow,
    samples: u32,
    /// The sample number of its latest sample.
    latest: u64,
}

/// The block being filled.
struct Open {
    start: u64,
    day: String,
    active: u32,
    tallies: Vec<Tally>,
}

/// Samples into blocks.
pub struct Blocks {
    block_ms: u64,
    sample_ms: u64,
    open: Option<Open>,
    last: Option<u64>,
    /// Numbers samples, for "the song sampled last".
    serial: u64,
}

/// The tie order: recording, composing, mixing, editing, other.
fn rank(c: &MetricsCategoryRow) -> u8 {
    match c {
        MetricsCategoryRow::Recording => 4,
        MetricsCategoryRow::Composing => 3,
        MetricsCategoryRow::Mixing => 2,
        MetricsCategoryRow::Editing => 1,
        MetricsCategoryRow::Other => 0,
    }
}

impl Blocks {
    /// Blocks of `block_ms`, sampled every `sample_ms` (both checked by
    /// the caller: `0 < sample_ms <= block_ms`).
    pub fn new(block_ms: u64, sample_ms: u64) -> Blocks {
        Blocks {
            block_ms,
            sample_ms: sample_ms.max(1),
            open: None,
            last: None,
            serial: 0,
        }
    }

    /// One sample: `category` `None` is idle. Answers `Some` when the
    /// sample closed a block, holding what it counted, if anything. A
    /// sample earlier than the last is ignored.
    pub fn sample(
        &mut self,
        ms: u64,
        day: &str,
        category: Option<MetricsCategoryRow>,
        song: &SongKey,
    ) -> Option<Option<Counted>> {
        if self.last.is_some_and(|l| ms < l) {
            return None;
        }
        self.last = Some(ms);
        self.serial += 1;
        let closed = match &self.open {
            Some(o) if ms >= o.start.saturating_add(self.block_ms) => Some(self.close()),
            _ => None,
        };
        let open = self.open.get_or_insert_with(|| Open {
            start: ms,
            day: day.to_string(),
            active: 0,
            tallies: Vec::new(),
        });
        if let Some(category) = category {
            open.active = open.active.saturating_add(1);
            match open
                .tallies
                .iter_mut()
                .find(|t| t.category == category && t.song.same(song))
            {
                Some(t) => {
                    t.samples = t.samples.saturating_add(1);
                    t.latest = self.serial;
                    t.song = song.clone();
                }
                None => open.tallies.push(Tally {
                    song: song.clone(),
                    category,
                    samples: 1,
                    latest: self.serial,
                }),
            }
        }
        closed
    }

    /// Closes the open block: what it counts, if at least half its
    /// expected samples were active.
    fn close(&mut self) -> Option<Counted> {
        let open = self.open.take()?;
        let expected = self.block_ms / self.sample_ms;
        if u64::from(open.active) * 2 < expected {
            return None;
        }
        let win = open
            .tallies
            .into_iter()
            .max_by_key(|t| (t.samples, rank(&t.category), t.latest))?;
        Some(Counted {
            song: win.song,
            day: open.day,
            category: win.category,
            seconds: self.block_ms / 1000,
        })
    }
}

/// Recent undos of one song.
#[derive(Default)]
struct Undos {
    times: Vec<u64>,
    /// The latest undo of the burst still going, if one is.
    burst: Option<u64>,
    newest: u64,
}

/// Recent commits of one label in one song.
#[derive(Default)]
struct Steps {
    times: Vec<u64>,
    newest: u64,
}

/// The friction windows: undo bursts and repeated tasks.
#[derive(Default)]
pub struct Friction {
    undos: HashMap<Option<String>, Undos>,
    steps: HashMap<(Option<String>, &'static str), Steps>,
}

fn key(song: &SongKey) -> Option<String> {
    match song {
        SongKey::Song { id, .. } => Some(id.clone()),
        SongKey::Untitled => None,
    }
}

impl Friction {
    /// An undo in `song`: true when it makes a new burst.
    pub fn undo(&mut self, ms: u64, song: &SongKey) -> bool {
        self.undos
            .retain(|_, u| ms.saturating_sub(u.newest) < UNDO_WINDOW_MS);
        let u = self.undos.entry(key(song)).or_default();
        u.newest = u.newest.max(ms);
        if let Some(last) = u.burst {
            if ms.saturating_sub(last) < UNDO_WINDOW_MS {
                u.burst = Some(ms.max(last));
                return false;
            }
            u.burst = None;
        }
        u.times.retain(|t| ms.saturating_sub(*t) < UNDO_WINDOW_MS);
        u.times.push(ms);
        if u.times.len() >= UNDO_BURST as usize {
            u.times.clear();
            u.burst = Some(ms);
            return true;
        }
        false
    }

    /// A new step with kept label `label` in `song`: true when it makes a
    /// repeat. [`ANOTHER_EDIT`] never does.
    pub fn step(&mut self, ms: u64, song: &SongKey, label: &'static str) -> bool {
        if label == ANOTHER_EDIT {
            return false;
        }
        self.steps
            .retain(|_, s| ms.saturating_sub(s.newest) < REPEAT_WINDOW_MS);
        let s = self.steps.entry((key(song), label)).or_default();
        s.newest = s.newest.max(ms);
        s.times.retain(|t| ms.saturating_sub(*t) < REPEAT_WINDOW_MS);
        s.times.push(ms);
        if s.times.len() >= REPEAT_COUNT as usize {
            s.times.clear();
            return true;
        }
        false
    }
}
