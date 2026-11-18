//! The time records' format (plan 8f Ruling 2): one JSON object, parsed
//! strictly and checked for bounds; the merge of pending counts into it;
//! and the only words it may hold besides song names.

use super::{MAX_LABELS, MetricsCategoryRow, NAME_MAX};
use core_model::document::check_project_id;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

/// The format's version.
pub const VERSION: u32 = 1;
/// The most seconds a day may hold in one category.
pub const MAX_DAY_SECONDS: u64 = 1_000_000_000;
/// The most songs a store may hold; later songs count as untitled.
pub const MAX_SONGS: usize = 100_000;
/// A step whose label is not one of VizDAW's words.
pub const ANOTHER_EDIT: &str = "another edit";
/// A menu item that is not one of VizDAW's words.
pub const ANOTHER_ITEM: &str = "another item";
/// A menu hunt that chose nothing.
pub const NOTHING_CHOSEN: &str = "nothing chosen";
/// Steps whose label starts so are Co-Producer's, never the composer's.
const CO_PRODUCER_STEP: &str = "Co-Producer:";

/// The whole file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Store {
    /// [`VERSION`].
    pub version: u32,
    /// The first day counted.
    pub since: Option<String>,
    /// Songs by project ID.
    pub songs: BTreeMap<String, Song>,
    /// Songs that have no project ID.
    pub untitled: Untitled,
}

/// One song's records.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Song {
    /// The song's own name, cleaned.
    pub name: String,
    /// Days by "YYYY-MM-DD".
    pub days: BTreeMap<String, Day>,
}

/// The records of songs never saved.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Untitled {
    /// Days by "YYYY-MM-DD".
    pub days: BTreeMap<String, Day>,
}

/// One day of one song.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Day {
    /// Seconds by category.
    pub seconds: Seconds,
    /// Bursts of undos.
    pub undo_bursts: u32,
    /// Repeated tasks by kept label.
    pub repeats: BTreeMap<String, u32>,
    /// Long menu hunts.
    pub hunts: u32,
    /// What the hunts ended at, by kept label.
    pub hunt_ends: BTreeMap<String, u32>,
}

/// Seconds by category.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seconds {
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

impl Default for Store {
    fn default() -> Self {
        Store {
            version: VERSION,
            since: None,
            songs: BTreeMap::new(),
            untitled: Untitled::default(),
        }
    }
}

impl Seconds {
    fn all(&self) -> [u64; 5] {
        [
            self.composing,
            self.recording,
            self.editing,
            self.mixing,
            self.other,
        ]
    }

    /// Every category's seconds together.
    pub fn total(&self) -> u64 {
        self.all().iter().fold(0u64, |a, b| a.saturating_add(*b))
    }

    /// The category's slot.
    pub fn slot(&mut self, c: &MetricsCategoryRow) -> &mut u64 {
        match c {
            MetricsCategoryRow::Composing => &mut self.composing,
            MetricsCategoryRow::Recording => &mut self.recording,
            MetricsCategoryRow::Editing => &mut self.editing,
            MetricsCategoryRow::Mixing => &mut self.mixing,
            MetricsCategoryRow::Other => &mut self.other,
        }
    }

    /// Adds `o`, saturating at `cap`.
    pub fn add(&mut self, o: &Seconds, cap: u64) {
        let add = |a: &mut u64, b: u64| *a = a.saturating_add(b).min(cap);
        add(&mut self.composing, o.composing);
        add(&mut self.recording, o.recording);
        add(&mut self.editing, o.editing);
        add(&mut self.mixing, o.mixing);
        add(&mut self.other, o.other);
    }
}

impl Day {
    /// Adds `o` into this day, keeping the bounds (the store's merge).
    pub fn merge(&mut self, o: &Day) {
        self.seconds.add(&o.seconds, MAX_DAY_SECONDS);
        self.undo_bursts = self.undo_bursts.saturating_add(o.undo_bursts);
        self.hunts = self.hunts.saturating_add(o.hunts);
        for (l, n) in &o.repeats {
            add_label(&mut self.repeats, l, *n, ANOTHER_EDIT);
        }
        for (l, n) in &o.hunt_ends {
            add_label(&mut self.hunt_ends, l, *n, ANOTHER_ITEM);
        }
    }

    /// Nothing is counted in it.
    pub fn is_empty(&self) -> bool {
        *self == Day::default()
    }
}

/// Adds `n` to `label` in `map`; past [`MAX_LABELS`] distinct labels a new
/// one counts under `fallback`.
pub fn add_label(map: &mut BTreeMap<String, u32>, label: &str, n: u32, fallback: &str) {
    let key = if map.contains_key(label) || map.len() < MAX_LABELS {
        label
    } else {
        fallback
    };
    let slot = map.entry(key.to_string()).or_insert(0);
    *slot = slot.saturating_add(n);
}

/// Who a count belongs to: a song by project ID, or untitled.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SongKey {
    /// A saved song: its project ID and cleaned name.
    Song {
        /// The project ID.
        id: String,
        /// The cleaned name.
        name: String,
    },
    /// A song never saved, or none.
    Untitled,
}

impl SongKey {
    /// The key of a song row from Swift: a bad or missing ID is untitled.
    pub fn of(song: Option<&super::MetricsSongRow>) -> SongKey {
        match song {
            Some(super::MetricsSongRow { id: Some(id), name }) if check_project_id(id) => {
                SongKey::Song {
                    id: id.clone(),
                    name: clean_name(name),
                }
            }
            _ => SongKey::Untitled,
        }
    }

    /// Who it is, without the name.
    pub fn same(&self, o: &SongKey) -> bool {
        match (self, o) {
            (SongKey::Song { id: a, .. }, SongKey::Song { id: b, .. }) => a == b,
            (SongKey::Untitled, SongKey::Untitled) => true,
            _ => false,
        }
    }
}

impl Store {
    /// The day `day` of `song`, made if missing (and the song's name
    /// updated). Past [`MAX_SONGS`] a new song counts as untitled.
    pub fn day_mut(&mut self, song: &SongKey, day: &str) -> &mut Day {
        if self.since.as_deref().is_none_or(|s| day < s) {
            self.since = Some(day.to_string());
        }
        let days = match song {
            SongKey::Song { id, name }
                if self.songs.contains_key(id) || self.songs.len() < MAX_SONGS =>
            {
                let s = self.songs.entry(id.clone()).or_default();
                s.name.clone_from(name);
                &mut s.days
            }
            _ => &mut self.untitled.days,
        };
        days.entry(day.to_string()).or_default()
    }

    /// Adds everything in `pending` into this store.
    pub fn merge(&mut self, pending: &Store) {
        for (id, song) in &pending.songs {
            let key = SongKey::Song {
                id: id.clone(),
                name: song.name.clone(),
            };
            for (day, d) in &song.days {
                self.day_mut(&key, day).merge(d);
            }
        }
        for (day, d) in &pending.untitled.days {
            self.day_mut(&SongKey::Untitled, day).merge(d);
        }
    }

    /// Nothing is counted.
    pub fn is_empty(&self) -> bool {
        self.songs.is_empty() && self.untitled.days.is_empty()
    }

    /// The earliest day counted.
    pub fn first_day(&self) -> Option<&str> {
        self.songs
            .values()
            .flat_map(|s| s.days.keys())
            .chain(self.untitled.days.keys())
            .min()
            .map(String::as_str)
    }

    /// Parses `text` strictly and checks every key, date and bound.
    pub fn parse(text: &str) -> Result<Store, String> {
        let s: Store = serde_json::from_str(text).map_err(|e| e.to_string())?;
        s.check()?;
        Ok(s)
    }

    fn check(&self) -> Result<(), String> {
        if self.version != VERSION {
            return Err(format!("version {}", self.version));
        }
        if self
            .since
            .as_deref()
            .is_some_and(|s| !super::summary::is_date(s))
        {
            return Err("since".into());
        }
        if self.songs.len() > MAX_SONGS {
            return Err("songs".into());
        }
        for (id, song) in &self.songs {
            if !check_project_id(id) || !is_clean_text(&song.name) {
                return Err("song".into());
            }
            check_days(&song.days)?;
        }
        check_days(&self.untitled.days)
    }
}

fn check_days(days: &BTreeMap<String, Day>) -> Result<(), String> {
    for (key, d) in days {
        if !super::summary::is_date(key) {
            return Err("day".into());
        }
        if d.seconds.all().iter().any(|n| *n > MAX_DAY_SECONDS) {
            return Err("seconds".into());
        }
        for map in [&d.repeats, &d.hunt_ends] {
            if map.len() > MAX_LABELS + 1 || !map.keys().all(|l| is_clean_text(l)) {
                return Err("labels".into());
            }
        }
    }
    Ok(())
}

/// At most [`NAME_MAX`] characters and no control characters.
fn is_clean_text(s: &str) -> bool {
    s.chars().count() <= NAME_MAX && !s.chars().any(char::is_control)
}

/// A song name as stored: control characters dropped, trimmed, at most
/// [`NAME_MAX`] characters.
pub fn clean_name(name: &str) -> String {
    let kept: String = name.chars().filter(|c| !c.is_control()).collect();
    let cut: String = kept.trim().chars().take(NAME_MAX).collect();
    cut.trim_end().to_string()
}

/// VizDAW's own words: every registry title without its trailing "…".
fn titles() -> &'static HashSet<&'static str> {
    static TITLES: OnceLock<HashSet<&'static str>> = OnceLock::new();
    TITLES.get_or_init(|| {
        core_control::registry::COMMANDS
            .iter()
            .map(|c| bare(c.title))
            .collect()
    })
}

fn bare(s: &str) -> &str {
    let s = s.trim();
    s.strip_suffix('…')
        .or_else(|| s.strip_suffix("..."))
        .unwrap_or(s)
        .trim_end()
}

/// The registry title `label` names, without its "…", if it is one.
fn kept(label: &str) -> Option<&'static str> {
    titles().get(bare(label)).copied()
}

/// A step's label as stored: a registry title, else [`ANOTHER_EDIT`];
/// `None` for Co-Producer's own steps, which are not counted at all.
pub fn step_label(label: &str) -> Option<&'static str> {
    if label.trim_start().starts_with(CO_PRODUCER_STEP) {
        return None;
    }
    Some(kept(label).unwrap_or(ANOTHER_EDIT))
}

/// A menu item as stored: a registry title, else [`ANOTHER_ITEM`];
/// [`NOTHING_CHOSEN`] for none.
pub fn menu_label(chosen: Option<&str>) -> &'static str {
    match chosen {
        None => NOTHING_CHOSEN,
        Some(c) => kept(c).unwrap_or(ANOTHER_ITEM),
    }
}
