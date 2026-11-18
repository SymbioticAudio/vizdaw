//! The read (plan 8f Ruling 6): totals per song and across all songs for
//! all time or the last `n` days, ranked and cut; and the date helpers.

use super::store::{Day, Store};
use super::{
    MAX_SONGS_SHOWN, MetricsCountRow, MetricsOverviewRow, MetricsSecondsRow, MetricsSongTotalsRow,
    MetricsSummaryRow, MetricsTotalsRow,
};
use std::collections::BTreeMap;

/// How untitled songs are shown.
pub const UNTITLED_SONGS: &str = "Untitled songs";
/// The most repeats and hunt ends `all` shows.
const ALL_LABELS: usize = 10;
/// The most repeats and hunt ends each song shows.
const SONG_LABELS: usize = 5;

/// A civil date "YYYY-MM-DD" as (year, month, day), if it is one.
pub fn parse_date(s: &str) -> Option<(i64, u32, u32)> {
    let b = s.as_bytes();
    if b.len() != 10 || b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = s.get(r)?;
        if !part.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let y = i64::from(y);
    (y >= 1 && (1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m)).then_some((y, m, d))
}

/// It is a civil date "YYYY-MM-DD".
pub fn is_date(s: &str) -> bool {
    parse_date(s).is_some()
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a civil date (the proleptic Gregorian
/// calendar's days-from-civil).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day count since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m as u32, d as u32)
}

/// The first day of the window of `days` days ending on `today`.
fn window_start(today: (i64, u32, u32), days: u32) -> String {
    let (y, m, d) = civil_from_days(
        days_from_civil(today.0, today.1, today.2) - i64::from(days.saturating_sub(1)),
    );
    format!("{y:04}-{m:02}-{d:02}")
}

/// Sums of days, before they become rows.
#[derive(Default)]
struct Sum {
    seconds: [u64; 5],
    undo_bursts: u32,
    repeats: BTreeMap<String, u32>,
    hunts: u32,
    hunt_ends: BTreeMap<String, u32>,
}

impl Sum {
    fn add(&mut self, d: &Day) {
        let s = &d.seconds;
        let add = [s.composing, s.recording, s.editing, s.mixing, s.other];
        for (a, b) in self.seconds.iter_mut().zip(add) {
            *a = a.saturating_add(b);
        }
        self.undo_bursts = self.undo_bursts.saturating_add(d.undo_bursts);
        self.hunts = self.hunts.saturating_add(d.hunts);
        for (into, from) in [
            (&mut self.repeats, &d.repeats),
            (&mut self.hunt_ends, &d.hunt_ends),
        ] {
            for (l, n) in from {
                let slot = into.entry(l.clone()).or_insert(0);
                *slot = slot.saturating_add(*n);
            }
        }
    }

    fn total(&self) -> u64 {
        self.seconds.iter().fold(0u64, |a, b| a.saturating_add(*b))
    }

    fn row(&self, cut: usize) -> MetricsTotalsRow {
        let [composing, recording, editing, mixing, other] = self.seconds;
        MetricsTotalsRow {
            seconds: MetricsSecondsRow {
                composing,
                recording,
                editing,
                mixing,
                other,
            },
            undo_bursts: self.undo_bursts,
            repeats: ranked(&self.repeats, cut),
            hunts: self.hunts,
            hunt_ends: ranked(&self.hunt_ends, cut),
        }
    }
}

/// The largest `cut` counts, then by label.
fn ranked(map: &BTreeMap<String, u32>, cut: usize) -> Vec<MetricsCountRow> {
    let mut v: Vec<MetricsCountRow> = map
        .iter()
        .map(|(l, n)| MetricsCountRow {
            label: l.clone(),
            count: *n,
        })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
    v.truncate(cut);
    v
}

/// One song before it is ranked.
struct Entry {
    name: String,
    this_song: bool,
    untitled: bool,
    sum: Sum,
}

/// Ruling 6's totals of `store` for `days` days ending on `today` (`None`:
/// all time); `song_id`'s song is "this song". `today` must be a date.
pub fn summarize(
    store: &Store,
    today: (i64, u32, u32),
    days: Option<u32>,
    song_id: Option<&str>,
) -> MetricsSummaryRow {
    let today_s = format!("{:04}-{:02}-{:02}", today.0, today.1, today.2);
    let start = days.map(|n| window_start(today, n));
    let within = |day: &str| {
        start
            .as_deref()
            .is_none_or(|s| s <= day && day <= today_s.as_str())
    };
    let mut all = Sum::default();
    let mut entries: Vec<Entry> = Vec::new();
    let mut take = |name: &str, untitled: bool, this_song: bool, d: &BTreeMap<String, Day>| {
        let mut sum = Sum::default();
        let mut any = false;
        for day in d.iter().filter(|(k, _)| within(k)).map(|(_, v)| v) {
            sum.add(day);
            all.add(day);
            any = true;
        }
        if any || this_song {
            entries.push(Entry {
                name: name.to_string(),
                this_song,
                untitled,
                sum,
            });
        }
    };
    for (id, song) in &store.songs {
        take(&song.name, false, song_id == Some(id.as_str()), &song.days);
    }
    if !store.untitled.days.is_empty() {
        take(UNTITLED_SONGS, true, false, &store.untitled.days);
    }
    entries.sort_by(|a, b| {
        b.sum
            .total()
            .cmp(&a.sum.total())
            .then_with(|| a.name.cmp(&b.name))
    });
    let count = entries.len();
    let this = entries.iter().position(|e| e.this_song);
    let mut shown: Vec<Entry> = Vec::new();
    for (i, e) in entries.into_iter().enumerate() {
        let room = if this.is_some_and(|t| t >= MAX_SONGS_SHOWN) {
            MAX_SONGS_SHOWN - 1
        } else {
            MAX_SONGS_SHOWN
        };
        if i < room || e.this_song {
            shown.push(e);
        }
    }
    let more_songs = u32::try_from(count - shown.len()).unwrap_or(u32::MAX);
    MetricsSummaryRow {
        since: store.first_day().map(str::to_string),
        days,
        all: all.row(ALL_LABELS),
        songs: shown
            .into_iter()
            .map(|e| MetricsSongTotalsRow {
                name: e.name,
                this_song: e.this_song,
                untitled: e.untitled,
                totals: e.sum.row(SONG_LABELS),
            })
            .collect(),
        more_songs,
    }
}

/// The settings' overview: how many songs (untitled as one), since when,
/// and every second counted.
pub fn overview(store: &Store) -> MetricsOverviewRow {
    let days = store
        .songs
        .values()
        .flat_map(|s| s.days.values())
        .chain(store.untitled.days.values());
    let total_seconds = days.fold(0u64, |a, d| a.saturating_add(d.seconds.total()));
    let songs = store.songs.len() + usize::from(!store.untitled.days.is_empty());
    MetricsOverviewRow {
        songs: u32::try_from(songs).unwrap_or(u32::MAX),
        since: store.first_day().map(str::to_string),
        total_seconds,
    }
}
