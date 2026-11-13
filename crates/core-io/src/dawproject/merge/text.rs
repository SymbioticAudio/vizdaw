//! The merge's report text (plan 6e Task 5, Ruling 15): every line a
//! merge adds to its report, the preview's summary and the refusal when
//! the exported sequence is gone. The core owns every sentence; the app
//! never builds one.

use super::decide::MergePreview;
use crate::report::ItemKind;

// ---------------------------------------------------------------- Conflicts

/// Conflicts: a track part changed on both sides; theirs became an
/// alternate take. Detail [`conflict_take`].
pub const CONFLICT_TAKE: &str =
    "Tracks changed here and in the other DAW (the other DAW's version is an alternate take)";
/// Conflicts: a setting changed on both sides; ours was kept. Detail
/// [`conflict_setting`].
pub const CONFLICT_SETTING: &str =
    "Settings changed here and in the other DAW (kept as they are here)";
/// Conflicts: tracks theirs deleted that hold what the file cannot carry
/// (Ruling 7). Detail: the track.
pub const KEPT_EMPTY: &str =
    "Tracks the other DAW deleted that hold what the file cannot carry (kept, emptied)";
/// Conflicts: parts theirs deleted that ours changed. Detail: the part.
pub const DELETED_THERE: &str = "Parts the other DAW deleted that were changed here (kept)";
/// Conflicts: parts ours deleted that theirs changed. Detail: the part.
pub const DELETED_HERE: &str = "Parts deleted here that the other DAW changed (left deleted)";
/// Conflicts: parts added here and in the other DAW under one name, that
/// differ (Task 5 fix round 1). Detail: the part.
pub const ADDED_BOTH: &str =
    "Parts added here and in the other DAW under the same name (kept as they are here)";

/// Conflicts: changes the other DAW made that the merge does not apply
/// (Task 6 fix round 1): edits on a track that plays with its own loop,
/// length or play quantize, an instrument swap, a plug-in state dropped.
/// Detail [`conflict_setting`].
pub const NOT_APPLIED: &str =
    "Changes the other DAW made that VizDAW does not apply (kept as they are here)";

// ---------------------------------------------------------------- Changed

/// Changed: shared-sequence uses edited differently (Ruling 8). Detail:
/// the sequence.
pub const PRIVATE_COPIES: &str =
    "Uses of a shared sequence edited differently became private copies";
/// Changed: looped uses edited on some passes (Ruling 8).
pub const LOOPS_UNROLLED: &str = "Looped uses edited on some passes became notes";

// ---------------------------------------------------------------- Left out

/// Left out: devices theirs replaced with plug-ins VizDAW cannot host
/// (Ruling 10). Detail: the plug-in's name.
pub const VST_KEPT: &str =
    "Plug-ins the other DAW replaced with ones VizDAW cannot host (kept as they are here)";

// ---------------------------------------------------------------- Notes

/// Note: replaces `import::FROM_VIZDAW` in a merge. `<yyyy-mm-dd>` is the
/// export's date in UTC ([`merged`]).
pub const MERGED: &str =
    "This file was merged into the project it came from (the export of <yyyy-mm-dd>)";
/// Note: the file lost VizDAW's track tags ([`Matches::heuristic`]).
///
/// [`Matches::heuristic`]: super::matching::Matches::heuristic
pub const BY_NAME: &str =
    "The file lost VizDAW's track tags; tracks were matched by name and order";
/// Note: the user chose the snapshot.
pub const SNAPSHOT_CHOSEN: &str =
    "The file lost its export ID; it was compared with the export you chose";
/// Note: a format-1 snapshot (Ruling 3); every such merge is matched by
/// name and order.
pub const OLD_EXPORT: &str =
    "This export was made before merging existed; tracks were matched by name and order";
/// Note: shared-sequence edits play outside the merged sequence too
/// (Ruling 8). Detail: the sequence.
pub const SHARED_ELSEWHERE: &str =
    "Edits to shared sequences also play where they are used outside the merged sequence";
/// Note: a conflict take holds only the conflicting part of a split track
/// (Ruling 6). Detail: the track.
pub const TAKE_PARTS: &str = "An alternate take holds only the other DAW's changed part of a track";

// ---------------------------------------------------------------- summaries

/// The summary when the merge has nothing to do.
pub const NOTHING: &str = "Nothing to merge: the file matches the project.";

/// The refusal when the project no longer has the exported sequence.
pub const SEQUENCE_GONE: &str =
    "The sequence this file was exported from is no longer in the project.";

/// Every report line above with the group it goes in, for the app's
/// grouping and the tests.
pub const LINES: [(&str, ItemKind); 16] = [
    (CONFLICT_TAKE, ItemKind::Conflict),
    (CONFLICT_SETTING, ItemKind::Conflict),
    (KEPT_EMPTY, ItemKind::Conflict),
    (DELETED_THERE, ItemKind::Conflict),
    (DELETED_HERE, ItemKind::Conflict),
    (ADDED_BOTH, ItemKind::Conflict),
    (NOT_APPLIED, ItemKind::Conflict),
    (PRIVATE_COPIES, ItemKind::Changed),
    (LOOPS_UNROLLED, ItemKind::Changed),
    (VST_KEPT, ItemKind::LeftOut),
    (MERGED, ItemKind::Note),
    (BY_NAME, ItemKind::Note),
    (SNAPSHOT_CHOSEN, ItemKind::Note),
    (OLD_EXPORT, ItemKind::Note),
    (SHARED_ELSEWHERE, ItemKind::Note),
    (TAKE_PARTS, ItemKind::Note),
];

/// "Merged: <a> applied, <k> kept, <n> added, <r> removed, <c> in
/// conflict", or [`NOTHING`] when every count is 0.
pub fn summary(p: &MergePreview) -> String {
    if p.applied == 0 && p.kept == 0 && p.added == 0 && p.removed == 0 && p.conflicts == 0 {
        return NOTHING.to_owned();
    }
    format!(
        "Merged: {} applied, {} kept, {} added, {} removed, {} in conflict",
        p.applied, p.kept, p.added, p.removed, p.conflicts
    )
}

/// [`MERGED`] with the date of `exported_at` (seconds since 1970, UTC).
pub fn merged(exported_at: u64) -> String {
    MERGED.replace("<yyyy-mm-dd>", &utc_date(exported_at))
}

/// [`CONFLICT_TAKE`]'s detail: "<track>: take “<take>”".
pub fn conflict_take(track: &str, take: &str) -> String {
    format!("{track}: take \u{201c}{take}\u{201d}")
}

/// [`CONFLICT_SETTING`]'s detail: "<what>: here <ours>, there <theirs>".
pub fn conflict_setting(what: &str, ours: &str, theirs: &str) -> String {
    format!("{what}: here {ours}, there {theirs}")
}

/// `secs` since 1970 as a UTC date, `yyyy-mm-dd` (days to the civil
/// calendar, proleptic Gregorian).
pub fn utc_date(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX / 2);
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe
        .saturating_add(era.saturating_mul(400))
        .saturating_add(i64::from(m <= 2));
    format!("{y:04}-{m:02}-{d:02}")
}

// ---------------------------------------------------------------- values

/// A level as the app shows it: "−6.0 dB", "+3.0 dB", "0.0 dB", "−∞ dB"
/// (at or below the −144 dB floor, or not a number).
pub fn level(db: f64) -> String {
    let floor = core_model::mixer::GAIN_FLOOR_DB;
    if db.is_nan() || db <= floor {
        return "\u{2212}\u{221e} dB".to_owned();
    }
    let v = (db * 10.0).round() / 10.0;
    if v <= floor {
        return "\u{2212}\u{221e} dB".to_owned();
    }
    if v == 0.0 {
        return "0.0 dB".to_owned();
    }
    let sign = if v < 0.0 { "\u{2212}" } else { "+" };
    format!("{sign}{:.1} dB", v.abs())
}

/// A pan as the app shows it: "L 24", "C", "R 10".
pub fn pan(p: f64) -> String {
    let p = if p.is_nan() { 0.0 } else { p };
    let n = (p.abs().min(1.0) * 100.0).round();
    if n == 0.0 {
        "C".to_owned()
    } else if p < 0.0 {
        format!("L {n}")
    } else {
        format!("R {n}")
    }
}

/// "on" or "off".
pub fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

/// A tempo in milli-BPM: "120 BPM", "120.5 BPM".
pub fn bpm(milli: u32) -> String {
    let whole = milli / 1_000;
    let frac = milli % 1_000;
    if frac == 0 {
        format!("{whole} BPM")
    } else {
        let f = format!("{frac:03}");
        format!("{whole}.{} BPM", f.trim_end_matches('0'))
    }
}

/// A curve: "1 point", "12 points".
pub fn points(n: usize) -> String {
    if n == 1 {
        "1 point".to_owned()
    } else {
        format!("{n} points")
    }
}
