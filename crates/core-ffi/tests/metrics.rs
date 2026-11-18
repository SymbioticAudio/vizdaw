//! Private local time records (plan 8f Task 2; Rulings 2, 4, 5 and 6):
//! minute blocks counted by majority, friction signals, a strictly parsed
//! private file under the support folder, and the summary read.
//!
//! Every test makes its own temp support folder (0700) and passes every
//! time in, starting at day "2026-11-16". Nothing reads the clock, and
//! nothing touches the real support folder.

use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::control::{ControlResult, result_json};
use vizdaw_ffi::edit::{EditCommand, SequenceEdit};
use vizdaw_ffi::host::force_offline;
use vizdaw_ffi::metrics::{
    BLOCK_MS, MetricsCategoryRow as Cat, MetricsClockRow, MetricsCountRow, MetricsRecorder,
    MetricsSecondsRow, MetricsSongRow, MetricsSongTotalsRow, MetricsSummaryRow, MetricsTotalsRow,
    SAMPLE_MS, metrics_delete_all, metrics_overview, metrics_summary,
};
use vizdaw_ffi::{Session, VizError};

static SERIAL: Mutex<()> = Mutex::new(());

const A: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const B: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const C: &str = "cccccccc-0000-4000-8000-000000000003";
const DAY: &str = "2026-11-16";
const T0: u64 = 1_000_000_000;
const SET_ASIDE: &str = "The time records could not be read, so they were set aside as \
                         time.damaged.json and counting started afresh.";

/// One test at a time, offline.
fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    g
}

/// A private temp support folder.
struct Support {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

impl Support {
    fn new() -> Support {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Support { _dir: dir, path }
    }
    fn s(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
    fn metrics(&self) -> PathBuf {
        self.path.join("Metrics")
    }
    fn store(&self) -> PathBuf {
        self.metrics().join("time.json")
    }
    fn open(&self) -> Arc<MetricsRecorder> {
        MetricsRecorder::open(self.s(), BLOCK_MS, SAMPLE_MS).unwrap()
    }
    /// The file as JSON, or `Null` when there is none.
    fn json(&self) -> Value {
        match fs::read_to_string(self.store()) {
            Ok(t) => serde_json::from_str(&t).unwrap(),
            Err(_) => Value::Null,
        }
    }
    fn text(&self) -> String {
        fs::read_to_string(self.store()).unwrap_or_default()
    }
    fn write(&self, text: &str) {
        fs::create_dir_all(self.metrics()).unwrap();
        fs::set_permissions(self.metrics(), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(self.store(), text).unwrap();
    }
    fn summary(&self, days: Option<u32>, song: Option<&str>) -> MetricsSummaryRow {
        metrics_summary(self.s(), DAY.into(), days, song.map(str::to_string)).unwrap()
    }
}

fn mode(p: &Path) -> u32 {
    fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777
}

fn at(ms: u64) -> MetricsClockRow {
    on(ms, DAY)
}

fn on(ms: u64, day: &str) -> MetricsClockRow {
    MetricsClockRow {
        ms,
        day: day.into(),
    }
}

fn song(id: &str) -> Option<MetricsSongRow> {
    Some(MetricsSongRow {
        id: Some(id.into()),
        name: format!("Song {}", &id[..1]),
    })
}

/// The `<day>` of song `id` (None: untitled) on `day`, or `Null`.
fn day_of(v: &Value, id: Option<&str>, day: &str) -> Value {
    match id {
        Some(id) => v["songs"][id]["days"][day].clone(),
        None => v["untitled"]["days"][day].clone(),
    }
}

fn secs(v: &Value, id: Option<&str>, day: &str, cat: &str) -> u64 {
    day_of(v, id, day)["seconds"][cat].as_u64().unwrap_or(0)
}

fn bursts(v: &Value, id: Option<&str>) -> u64 {
    day_of(v, id, DAY)["undo_bursts"].as_u64().unwrap_or(0)
}

/// Every second in the file.
fn total_seconds(v: &Value) -> u64 {
    let mut days: Vec<&Value> = Vec::new();
    if let Some(songs) = v["songs"].as_object() {
        for s in songs.values() {
            days.extend(s["days"].as_object().into_iter().flat_map(|d| d.values()));
        }
    }
    days.extend(
        v["untitled"]["days"]
            .as_object()
            .into_iter()
            .flat_map(|d| d.values()),
    );
    days.iter()
        .flat_map(|d| {
            d["seconds"]
                .as_object()
                .into_iter()
                .flat_map(|s| s.values())
        })
        .map(|n| n.as_u64().unwrap())
        .sum()
}

/// Samples `list` 5 s apart from `T0` for one block, then one more idle
/// sample that closes it, and flushes; answers the file.
fn one_block(list: &[(Option<Cat>, Option<&str>)]) -> Value {
    let d = Support::new();
    let r = d.open();
    let mut t = T0;
    for (cat, id) in list {
        r.sample(at(t), *cat, id.and_then(song));
        t += SAMPLE_MS;
    }
    r.sample(at(T0 + BLOCK_MS), None, None);
    r.flush().unwrap();
    d.json()
}

fn repeat<T: Clone>(n: usize, x: T) -> Vec<T> {
    vec![x; n]
}

#[test]
fn blocks_count_by_majority() {
    let _g = serial();
    let mut list = repeat(8, (Some(Cat::Editing), Some(A)));
    list.extend(repeat(4, (Some(Cat::Mixing), Some(A))));
    let v = one_block(&list);
    assert_eq!(secs(&v, Some(A), DAY, "editing"), 60);
    assert_eq!(total_seconds(&v), 60);
    assert_eq!(v["songs"][A]["name"], "Song a");

    let mut list = repeat(5, (Some(Cat::Editing), Some(A)));
    list.extend(repeat(7, (None, Some(A))));
    assert_eq!(total_seconds(&one_block(&list)), 0, "5 of 12 active");

    let mut list = repeat(6, (Some(Cat::Mixing), Some(A)));
    list.extend(repeat(6, (Some(Cat::Composing), Some(A))));
    let v = one_block(&list);
    assert_eq!(
        secs(&v, Some(A), DAY, "composing"),
        60,
        "ties go to composing"
    );
    assert_eq!(total_seconds(&v), 60);

    let mut list = repeat(6, (Some(Cat::Editing), Some(A)));
    list.extend(repeat(6, (Some(Cat::Editing), Some(B))));
    let v = one_block(&list);
    assert_eq!(
        secs(&v, Some(B), DAY, "editing"),
        60,
        "the song sampled last"
    );
    assert_eq!(total_seconds(&v), 60);
}

#[test]
fn blocks_survive_odd_clocks() {
    let _g = serial();
    // A jump of 3 minutes closes the block with what it had.
    let d = Support::new();
    let r = d.open();
    for i in 0..8 {
        r.sample(at(T0 + i * SAMPLE_MS), Some(Cat::Editing), song(A));
    }
    let jump = T0 + 7 * SAMPLE_MS + 180_000;
    r.sample(at(jump), Some(Cat::Mixing), song(A));
    r.sample(at(jump + BLOCK_MS), None, None);
    r.flush().unwrap();
    let v = d.json();
    assert_eq!(secs(&v, Some(A), DAY, "editing"), 60);
    assert_eq!(total_seconds(&v), 60, "the gap counts nothing");

    // A sample earlier than the last is ignored: six mixing samples sent
    // back in time would otherwise tie with the six editing ones and win.
    let d = Support::new();
    let r = d.open();
    for i in 0..6 {
        r.sample(at(T0 + i * SAMPLE_MS), Some(Cat::Editing), song(A));
    }
    for i in 0..6 {
        r.sample(at(T0 + i * 1000), Some(Cat::Mixing), song(A));
    }
    r.sample(at(T0 + BLOCK_MS), None, None);
    r.flush().unwrap();
    let v = d.json();
    assert_eq!(secs(&v, Some(A), DAY, "editing"), 60);
    assert_eq!(secs(&v, Some(A), DAY, "mixing"), 0);

    // A block across midnight lands on the day it started.
    let d = Support::new();
    let r = d.open();
    for i in 0..12 {
        let day = if i < 6 { DAY } else { "2026-11-17" };
        r.sample(on(T0 + i * SAMPLE_MS, day), Some(Cat::Recording), song(A));
    }
    r.sample(on(T0 + BLOCK_MS, "2026-11-17"), None, None);
    r.flush().unwrap();
    let v = d.json();
    assert_eq!(secs(&v, Some(A), DAY, "recording"), 60);
    assert_eq!(secs(&v, Some(A), "2026-11-17", "recording"), 0);

    // Hostile clocks are ignored, with no panic.
    let d = Support::new();
    let r = d.open();
    for i in 0..12 {
        r.sample(at(T0 + i * SAMPLE_MS), Some(Cat::Editing), song(A));
        r.sample(at(u64::MAX), Some(Cat::Mixing), song(A));
        r.sample(
            on(T0 + i * SAMPLE_MS, "16/11/2026"),
            Some(Cat::Mixing),
            song(A),
        );
        r.note_undo(at(u64::MAX), song(A));
        r.note_undo(on(T0, "2026-02-30"), song(A));
        r.note_step(at(u64::MAX), song(A), "New Audio Track".into());
        r.note_menu(on(T0, ""), 9_000, None);
    }
    r.sample(at(T0 + BLOCK_MS), None, None);
    r.flush().unwrap();
    let v = d.json();
    assert_eq!(secs(&v, Some(A), DAY, "editing"), 60);
    assert_eq!(total_seconds(&v), 60);
    assert_eq!(bursts(&v, Some(A)), 0);
    assert_eq!(d.summary(None, None).all.hunts, 0);
}

#[test]
fn close_drops_the_open_block() {
    let _g = serial();
    let d = Support::new();
    let r = d.open();
    for i in 0..11 {
        r.sample(at(T0 + i * SAMPLE_MS), Some(Cat::Editing), song(A));
    }
    for i in 0..5 {
        r.note_undo(at(T0 + i * 1000), song(A));
    }
    r.close().unwrap();
    let v = d.json();
    assert_eq!(total_seconds(&v), 0, "a partial block is never written");
    assert_eq!(bursts(&v, Some(A)), 1, "pending friction is written");
    let before = d.text();
    for i in 0..30 {
        r.sample(at(T0 + (11 + i) * SAMPLE_MS), Some(Cat::Editing), song(A));
        r.note_undo(at(T0 + 100_000 + i), song(A));
    }
    r.flush().unwrap();
    r.close().unwrap();
    assert_eq!(d.text(), before, "nothing after close");
}

#[test]
fn friction_thresholds() {
    let _g = serial();
    let d = Support::new();
    let r = d.open();
    let mut t = T0;
    for _ in 0..4 {
        r.note_undo(at(t), song(A));
        t += 10_000;
    }
    r.flush().unwrap();
    assert_eq!(bursts(&d.json(), Some(A)), 0, "4 undos are no burst");
    r.note_undo(at(t), song(A));
    r.flush().unwrap();
    assert_eq!(bursts(&d.json(), Some(A)), 1, "5 are one");
    for _ in 0..4 {
        t += 10_000;
        r.note_undo(at(t), song(A));
    }
    r.flush().unwrap();
    assert_eq!(
        bursts(&d.json(), Some(A)),
        1,
        "9 undos 10 s apart are one burst"
    );
    t += 120_000;
    for _ in 0..5 {
        r.note_undo(at(t), song(A));
        t += 1_000;
    }
    r.flush().unwrap();
    assert_eq!(bursts(&d.json(), Some(A)), 2, "a second burst later");

    let d = Support::new();
    let r = d.open();
    for (i, id) in [A, B, A, B, A].iter().enumerate() {
        r.note_undo(at(T0 + i as u64 * 1000), song(id));
    }
    r.flush().unwrap();
    let v = d.json();
    assert_eq!(bursts(&v, Some(A)) + bursts(&v, Some(B)), 0, "split songs");

    // Repeats.
    let d = Support::new();
    let r = d.open();
    for i in 0..6 {
        r.note_step(at(T0 + i * 60_000), song(A), "New Audio Track".into());
    }
    r.flush().unwrap();
    let rep = |v: &Value| day_of(v, Some(A), DAY)["repeats"]["New Audio Track"].as_u64();
    assert_eq!(rep(&d.json()), Some(1));
    for i in 6..12 {
        r.note_step(at(T0 + i * 60_000), song(A), "New Audio Track".into());
    }
    r.flush().unwrap();
    assert_eq!(rep(&d.json()), Some(2));

    let d = Support::new();
    let r = d.open();
    for i in 0..6 {
        r.note_step(at(T0 + i * 132_000), song(A), "New Audio Track".into());
    }
    r.flush().unwrap();
    assert_eq!(rep(&d.json()), None, "6 over 11 minutes are none");

    // Hunts.
    let d = Support::new();
    let r = d.open();
    r.note_menu(at(T0), 7_999, Some("Quantize…".into()));
    r.flush().unwrap();
    assert_eq!(d.summary(None, None).all.hunts, 0);
    r.note_menu(at(T0 + 10_000), 8_000, Some("Quantize…".into()));
    r.flush().unwrap();
    let s = d.summary(None, None);
    assert_eq!(s.all.hunts, 1);
    assert_eq!(s.all.hunt_ends, vec![count("Quantize", 1)]);
}

fn count(label: &str, n: u32) -> MetricsCountRow {
    MetricsCountRow {
        label: label.into(),
        count: n,
    }
}

#[test]
fn only_vizdaws_words_are_kept() {
    let _g = serial();
    let d = Support::new();
    let r = d.open();
    let mut t = T0;
    let mut step = |r: &MetricsRecorder, id: &str, label: &str, n: usize| {
        for _ in 0..n {
            r.note_step(at(t), song(id), label.into());
            t += 1_000;
        }
    };
    for label in [
        "Mum's Swing",
        "Rename Lead Vox",
        "x/y.vizdaw",
        "Co-Producer: Tidy Lead Vox",
        "another edit",
    ] {
        step(&r, A, label, 12);
    }
    r.note_menu(at(T0), 8_000, Some("Some Plug-in…".into()));
    r.note_menu(at(T0), 8_000, None);
    r.flush().unwrap();
    let v = d.json();
    assert!(v["songs"].get(A).is_none(), "no repeats: {v}");
    let s = d.summary(None, None);
    let mut ends = s.all.hunt_ends.clone();
    ends.sort_by(|a, b| a.label.cmp(&b.label));
    assert_eq!(
        ends,
        vec![count("another item", 1), count("nothing chosen", 1)]
    );

    // The 33rd distinct label of a day counts under "another edit".
    let mut titles: Vec<&str> = Vec::new();
    for c in core_control::registry::COMMANDS {
        let t = c.title.strip_suffix('…').unwrap_or(c.title);
        if !t.starts_with("Co-Producer") && !titles.contains(&t) {
            titles.push(t);
        }
    }
    for label in titles.iter().take(33) {
        step(&r, B, label, 6);
    }
    r.flush().unwrap();
    let v = d.json();
    let reps = day_of(&v, Some(B), DAY)["repeats"].clone();
    let reps = reps.as_object().unwrap();
    assert_eq!(reps.len(), 33, "{reps:?}");
    assert_eq!(reps["another edit"], 1);
    assert!(!reps.contains_key(titles[32]));
    assert_eq!(reps[titles[0]], 1);

    // Names are cleaned; a bad song id is untitled.
    let named = |id: &str, name: String| {
        Some(MetricsSongRow {
            id: Some(id.into()),
            name,
        })
    };
    let long = "y".repeat(70_000);
    let songs = [
        (A, named(A, "a\nb".into())),
        (B, named(B, long)),
        (C, named(C, " x\0z ".into())),
        ("x", named("not a project id", "Mum's Draft".into())),
    ];
    let mut t = T0 + 10_000_000;
    for (_, s) in &songs {
        for _ in 0..12 {
            r.sample(at(t), Some(Cat::Editing), s.clone());
            t += SAMPLE_MS;
        }
        r.sample(at(t), None, None);
    }
    r.close().unwrap();
    let v = d.json();
    assert_eq!(v["songs"][A]["name"], "ab");
    assert_eq!(v["songs"][B]["name"], "y".repeat(128));
    assert_eq!(v["songs"][C]["name"], "xz");
    assert_eq!(v["songs"].as_object().unwrap().len(), 3);
    assert_eq!(secs(&v, None, DAY, "editing"), 60, "untitled");
    let text = d.text();
    for bad in ["Mum", "Lead Vox", ".vizdaw", "Tidy", "not a project id"] {
        assert!(!text.contains(bad), "{bad} reached the file");
    }
}

#[test]
fn the_store_stays_private() {
    let _g = serial();
    let mut t = T0;
    let mut burst = |r: &MetricsRecorder| {
        t += 120_000;
        for i in 0..5 {
            r.note_undo(at(t + i), song(A));
        }
    };
    let d = Support::new();
    let r = d.open();
    burst(&r);
    r.flush().unwrap();
    assert_eq!(mode(&d.metrics()), 0o700);
    assert_eq!(mode(&d.store()), 0o600);
    fs::set_permissions(d.store(), fs::Permissions::from_mode(0o644)).unwrap();
    burst(&r);
    r.flush().unwrap();
    assert_eq!(mode(&d.store()), 0o600, "0644 ends 0600");

    let d = Support::new();
    fs::create_dir(d.metrics()).unwrap();
    fs::set_permissions(d.metrics(), fs::Permissions::from_mode(0o755)).unwrap();
    let r = d.open();
    burst(&r);
    r.flush().unwrap();
    assert_eq!(mode(&d.metrics()), 0o700, "0755 ends 0700");

    // Metrics as a link.
    let d = Support::new();
    let elsewhere = tempfile::tempdir().unwrap();
    symlink(elsewhere.path(), d.metrics()).unwrap();
    let e = MetricsRecorder::open(d.s(), BLOCK_MS, SAMPLE_MS)
        .err()
        .unwrap();
    assert_eq!(
        e.to_string(),
        "The Metrics folder is not VizDAW's (a link)."
    );
    assert_eq!(fs::read_dir(elsewhere.path()).unwrap().count(), 0);

    // time.json as a link.
    let d = Support::new();
    fs::create_dir(d.metrics()).unwrap();
    let target = tempfile::NamedTempFile::new().unwrap();
    fs::write(target.path(), "keep").unwrap();
    symlink(target.path(), d.store()).unwrap();
    let r = d.open();
    burst(&r);
    assert!(r.flush().is_err());
    assert_eq!(fs::read_to_string(target.path()).unwrap(), "keep");
    assert!(fs::symlink_metadata(d.store()).unwrap().is_symlink());

    // A stray temp file goes at open.
    let d = Support::new();
    fs::create_dir(d.metrics()).unwrap();
    fs::write(d.metrics().join("time.json.tmp-1"), "{").unwrap();
    let _r = d.open();
    assert!(!d.metrics().join("time.json.tmp-1").exists());
}

fn day_json(seconds: [u64; 5], undo_bursts: u32, repeats: &[(&str, u32)], hunts: u32) -> Value {
    let map = |l: &[(&str, u32)]| {
        Value::Object(l.iter().map(|(k, n)| (k.to_string(), json!(n))).collect())
    };
    json!({
        "seconds": {"composing": seconds[0], "recording": seconds[1], "editing": seconds[2],
                    "mixing": seconds[3], "other": seconds[4]},
        "undo_bursts": undo_bursts, "repeats": map(repeats), "hunts": hunts, "hunt_ends": map(&[]),
    })
}

fn valid_store() -> Value {
    json!({
        "version": 1, "since": DAY,
        "songs": {A: {"name": "Night Drive", "days": {DAY: day_json([0, 0, 60, 0, 0], 1, &[], 0)}}},
        "untitled": {"days": {}},
    })
}

#[test]
fn a_damaged_store_is_set_aside() {
    let _g = serial();
    let good = valid_store();
    let d = Support::new();
    d.write(&good.to_string());
    let r = d.open();
    assert_eq!(r.problem(), None, "a valid store reads");
    assert_eq!(d.summary(None, None).all.seconds.editing, 60);

    let mut unknown = good.clone();
    unknown["extra"] = json!(1);
    let mut bad_day = good.clone();
    bad_day["untitled"]["days"]["2026-13-40"] = day_json([0; 5], 0, &[], 0);
    let mut bad_key = good.clone();
    bad_key["songs"]["x"] = good["songs"][A].clone();
    let huge_seconds = good
        .to_string()
        .replace("\"editing\":60", "\"editing\":1e10");
    assert!(huge_seconds.contains("1e10"));
    let mut too_many = good.clone();
    too_many["songs"][A]["days"][DAY]["seconds"]["mixing"] = json!(10_000_000_000u64);
    let big = " ".repeat(9 * 1024 * 1024) + &good.to_string();
    let truncated = good.to_string()[..40].to_string();
    for text in [
        truncated,
        big,
        unknown.to_string(),
        bad_day.to_string(),
        bad_key.to_string(),
        huge_seconds,
        too_many.to_string(),
    ] {
        let d = Support::new();
        d.write(&text);
        let s = d.summary(None, None);
        assert_eq!(s.since, None, "a damaged file reads as empty");
        assert!(d.store().exists(), "the summary moves nothing");
        let r = d.open();
        assert_eq!(r.problem().as_deref(), Some(SET_ASIDE));
        assert_eq!(
            fs::read_to_string(d.metrics().join("time.damaged.json")).unwrap(),
            text
        );
        assert!(!d.store().exists());
        for i in 0..5 {
            r.note_undo(at(T0 + i), song(B));
        }
        r.flush().unwrap();
        let v = d.json();
        assert_eq!(bursts(&v, Some(B)), 1, "counting starts afresh");
        assert!(v["songs"].get(A).is_none());
    }

    let d = Support::new();
    d.write("{");
    fs::write(d.metrics().join("time.damaged.json"), "older").unwrap();
    let r = d.open();
    assert_eq!(
        r.problem().as_deref(),
        Some(
            "The time records could not be read, so they were set aside as \
             time.damaged 2.json and counting started afresh."
        )
    );
    assert_eq!(
        fs::read_to_string(d.metrics().join("time.damaged.json")).unwrap(),
        "older"
    );
    assert_eq!(
        fs::read_to_string(d.metrics().join("time.damaged 2.json")).unwrap(),
        "{"
    );
}

fn song_json(name: &str, days: &[(&str, Value)]) -> Value {
    json!({
        "name": name,
        "days": Value::Object(days.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()),
    })
}

/// 3 songs, untitled time, days 2026-11-01 to 2026-11-16.
fn ranked_store() -> Value {
    json!({
        "version": 1, "since": "2026-11-01",
        "songs": {
            A: song_json("Night Drive", &[
                ("2026-11-01", day_json([0, 0, 3600, 0, 0], 2, &[("New Audio Track", 1)], 1)),
                ("2026-11-16", day_json([0, 0, 0, 600, 0], 1, &[], 0)),
            ]),
            B: song_json("Morning", &[
                ("2026-11-10", day_json([1200, 0, 0, 0, 0], 0, &[], 0)),
                ("2026-11-09", day_json([0, 1800, 0, 0, 0], 0, &[], 0)),
            ]),
            C: song_json("Third", &[
                ("2026-11-12", day_json([0, 0, 0, 0, 2000], 0, &[], 3)),
            ]),
        },
        "untitled": {"days": {"2026-11-05": day_json([0, 0, 2500, 0, 0], 0, &[], 0)}},
    })
}

fn seconds_of(t: &MetricsTotalsRow) -> [u64; 5] {
    let s: &MetricsSecondsRow = &t.seconds;
    [s.composing, s.recording, s.editing, s.mixing, s.other]
}

fn names(s: &MetricsSummaryRow) -> Vec<&str> {
    s.songs.iter().map(|x| x.name.as_str()).collect()
}

#[test]
fn summaries_total_and_rank() {
    let _g = serial();
    let d = Support::new();
    d.write(&ranked_store().to_string());

    let s = d.summary(None, None);
    assert_eq!(s.since.as_deref(), Some("2026-11-01"));
    assert_eq!(s.days, None);
    assert_eq!(seconds_of(&s.all), [1200, 1800, 6100, 600, 2000]);
    assert_eq!(s.all.undo_bursts, 3);
    assert_eq!(s.all.hunts, 4);
    assert_eq!(s.all.repeats, vec![count("New Audio Track", 1)]);
    assert_eq!(
        names(&s),
        vec!["Night Drive", "Morning", "Untitled songs", "Third"]
    );
    let untitled: &MetricsSongTotalsRow = &s.songs[2];
    assert!(untitled.untitled && !untitled.this_song);
    assert!(s.songs.iter().all(|x| !x.this_song));
    assert_eq!(s.more_songs, 0);

    let s = d.summary(Some(7), Some(C));
    assert_eq!(s.days, Some(7));
    assert_eq!(s.since.as_deref(), Some("2026-11-01"));
    assert_eq!(seconds_of(&s.all), [1200, 0, 0, 600, 2000]);
    assert_eq!(names(&s), vec!["Third", "Morning", "Night Drive"]);
    assert!(s.songs[0].this_song);
    assert!(!s.songs[1].this_song && !s.songs[2].this_song);

    // 30 days back from 2026-11-16 is 2026-10-18.
    let mut v = ranked_store();
    v["songs"][A]["days"]["2026-10-18"] = day_json([0, 0, 200, 0, 0], 0, &[], 0);
    v["songs"][A]["days"]["2026-10-17"] = day_json([0, 0, 100, 0, 0], 0, &[], 0);
    d.write(&v.to_string());
    let s = d.summary(Some(30), None);
    assert_eq!(seconds_of(&s.all), [1200, 1800, 6100 + 200, 600, 2000]);
    assert_eq!(s.since.as_deref(), Some("2026-10-17"));

    // 25 songs: 20 shown, this song among them.
    let mut songs = serde_json::Map::new();
    let id = |i: u64| format!("00000000-0000-4000-8000-{i:012}");
    for i in 1..=25u64 {
        songs.insert(
            id(i),
            song_json(
                &format!("S{i:02}"),
                &[(DAY, day_json([0, 0, 0, 0, i * 60], 0, &[], 0))],
            ),
        );
    }
    d.write(
        &json!({"version": 1, "since": DAY, "songs": songs, "untitled": {"days": {}}}).to_string(),
    );
    let s = d.summary(None, Some(&id(1)));
    assert_eq!(s.songs.len(), 20);
    assert_eq!(s.more_songs, 5);
    assert_eq!(s.songs[0].name, "S25");
    let last = s.songs.last().unwrap();
    assert_eq!(last.name, "S01");
    assert!(last.this_song);
    assert_eq!(s.songs.iter().filter(|x| x.this_song).count(), 1);

    // Repeats are cut: 10 for all, 5 per song, largest first then label.
    let labels: Vec<(String, u32)> = (1..=12u32)
        .map(|i| (format!("L{i:02}"), if i == 3 { 12 } else { i }))
        .collect();
    let refs: Vec<(&str, u32)> = labels.iter().map(|(l, n)| (l.as_str(), *n)).collect();
    d.write(
        &json!({"version": 1, "since": DAY,
            "songs": {A: song_json("Night Drive", &[(DAY, day_json([0; 5], 0, &refs, 0))])},
            "untitled": {"days": {}}})
        .to_string(),
    );
    let s = d.summary(None, None);
    let got: Vec<(String, u32)> = s
        .all
        .repeats
        .iter()
        .map(|c| (c.label.clone(), c.count))
        .collect();
    assert_eq!(got.len(), 10);
    assert_eq!(got[0], ("L03".to_string(), 12));
    assert_eq!(got[1], ("L12".to_string(), 12));
    assert_eq!(got[2], ("L11".to_string(), 11));
    assert_eq!(s.songs[0].totals.repeats.len(), 5);

    // Empty and missing stores.
    d.write(
        &json!({"version": 1, "since": null, "songs": {}, "untitled": {"days": {}}}).to_string(),
    );
    assert_eq!(d.summary(None, None).since, None);
    let d = Support::new();
    let s = d.summary(Some(7), None);
    assert_eq!(s.since, None);
    assert!(s.songs.is_empty());
}

#[test]
fn overview_and_delete_all() {
    let _g = serial();
    let d = Support::new();
    let o = metrics_overview(d.s()).unwrap();
    assert_eq!((o.songs, o.since.clone(), o.total_seconds), (0, None, 0));
    d.write(&ranked_store().to_string());
    let o = metrics_overview(d.s()).unwrap();
    assert_eq!(o.songs, 4, "untitled counts as one");
    assert_eq!(o.since.as_deref(), Some("2026-11-01"));
    assert_eq!(o.total_seconds, 1200 + 1800 + 6100 + 600 + 2000);
    metrics_delete_all(d.s()).unwrap();
    assert!(!d.metrics().exists());
    metrics_delete_all(d.s()).unwrap();
}

#[test]
fn delete_all_never_follows_links() {
    let _g = serial();
    let d = Support::new();
    let elsewhere = tempfile::tempdir().unwrap();
    fs::write(elsewhere.path().join("keep.txt"), "keep").unwrap();
    symlink(elsewhere.path(), d.metrics()).unwrap();
    metrics_delete_all(d.s()).unwrap();
    assert!(fs::symlink_metadata(d.metrics()).is_err(), "the link goes");
    assert_eq!(
        fs::read_to_string(elsewhere.path().join("keep.txt")).unwrap(),
        "keep"
    );
}

#[test]
fn recorder_refuses_hostile_input() {
    let _g = serial();
    let d = Support::new();
    let file = d.path.join("a file");
    fs::write(&file, "x").unwrap();
    for support in [
        "relative/support".to_string(),
        String::new(),
        d.path.join("missing").to_string_lossy().into_owned(),
        file.to_string_lossy().into_owned(),
        format!("{}\0", d.s()),
    ] {
        let r = MetricsRecorder::open(support.clone(), BLOCK_MS, SAMPLE_MS);
        assert!(r.is_err(), "{support:?}");
        assert!(metrics_summary(support.clone(), DAY.into(), None, None).is_err());
        assert!(metrics_overview(support.clone()).is_err());
        assert!(metrics_delete_all(support).is_err());
    }
    for (block, sample) in [(0, 0), (60_000, 0), (1_000, 5_000), (u64::MAX, 5_000)] {
        assert!(MetricsRecorder::open(d.s(), block, sample).is_err());
    }
    for (today, days) in [
        ("2026-11-31", None),
        ("today", None),
        (DAY, Some(0)),
        (DAY, Some(3651)),
    ] {
        let e = metrics_summary(d.s(), today.into(), days, None);
        assert!(matches!(e, Err(VizError::Model { .. })), "{today} {days:?}");
    }
    assert!(metrics_summary(d.s(), DAY.into(), None, Some("x\0".repeat(40_000))).is_ok());

    let r = d.open();
    r.note_menu(at(T0), u64::MAX, Some("\0".repeat(70_000)));
    r.note_step(at(T0), song(A), "z".repeat(70_000));
    r.sample(at(T0), Some(Cat::Other), None);
    r.flush().unwrap();
    let s = d.summary(None, None);
    assert_eq!(s.all.hunts, 1);
    assert_eq!(s.all.hunt_ends, vec![count("another item", 1)]);
    r.clear();
    r.flush().unwrap();
}

#[test]
fn clear_drops_what_is_pending() {
    let _g = serial();
    let d = Support::new();
    let r = d.open();
    for i in 0..5 {
        r.note_undo(at(T0 + i), song(A));
    }
    for i in 0..11 {
        r.sample(at(T0 + i * SAMPLE_MS), Some(Cat::Editing), song(A));
    }
    r.clear();
    r.sample(at(T0 + BLOCK_MS), None, None);
    r.flush().unwrap();
    assert_eq!(d.json(), Value::Null, "nothing pending after clear");
}

#[test]
fn snapshot_step_follows_history() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let rename = |name: &str| EditCommand::Sequence {
        edit: SequenceEdit::Rename {
            seq,
            name: name.into(),
        },
    };
    let start = s.snapshot().unwrap().step;
    let one = s.edit(rename("One"), None).unwrap().step;
    assert!(one > start);
    let two = s.edit(rename("Two"), None).unwrap().step;
    assert!(two > one);
    assert_eq!(s.undo().unwrap().step, one);
    assert_eq!(s.undo().unwrap().step, start);
    let redo = s.redo().unwrap().step;
    assert_eq!(redo, one);
    assert!(redo <= two);
    assert_eq!(s.redo().unwrap().step, two);
}

#[test]
fn metrics_result_json() {
    let _g = serial();
    let totals = MetricsTotalsRow {
        seconds: MetricsSecondsRow {
            composing: 1,
            recording: 2,
            editing: 3,
            mixing: 4,
            other: 5,
        },
        undo_bursts: 6,
        repeats: vec![count("New Audio Track", 7)],
        hunts: 8,
        hunt_ends: vec![count("nothing chosen", 9)],
    };
    let row = MetricsSummaryRow {
        since: Some(DAY.into()),
        days: Some(7),
        all: totals.clone(),
        songs: vec![MetricsSongTotalsRow {
            name: "Night Drive".into(),
            this_song: true,
            untitled: false,
            totals,
        }],
        more_songs: 2,
    };
    let v = result_json(&ControlResult::CoProducerMetrics { on: true, row });
    let totals = json!({
        "seconds": {"composing": 1, "recording": 2, "editing": 3, "mixing": 4, "other": 5},
        "undo_bursts": 6, "repeats": [["New Audio Track", 7]], "hunts": 8,
        "hunt_ends": [["nothing chosen", 9]],
    });
    let mut song = totals.clone();
    song["name"] = json!("Night Drive");
    song["this_song"] = json!(true);
    song["untitled"] = json!(false);
    assert_eq!(
        v,
        json!({"on": true, "since": DAY, "days": 7, "all": totals, "songs": [song], "more_songs": 2})
    );
}
