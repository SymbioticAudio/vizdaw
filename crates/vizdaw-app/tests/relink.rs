//! Relink (slice 7b Task 5; Swift `MediaModelTests`'s relink rows): Locate…
//! and Search Folder… run as `Relink` jobs with their banner, a failure is
//! the alert "Relink Failed", a partial relink is a Problems banner; the
//! sheet is offered for missing or changed media on open, when idle finds
//! more and after a recheck, each item once, never under the control
//! channel; a held merge blocks a relink. The pool rows of the Swift suite
//! belong to the Media Pool slice.

mod support;

use std::sync::Arc;

use support::{FakeSession, demo_fake};
use vizdaw_app::relink::{
    HELD_HELP, LEAVE_OFFLINE, LOCATE, MESSAGE, SEARCH_FOLDER, TITLE, detail, locate_label,
    reason_text,
};
use vizdaw_app::testing::{JobRequest, JobScript};
use vizdaw_app::{DocumentModel, ProjectBanner};
use vizdaw_ffi::{JobKindRow, MediaOriginRow, MediaRow, OfflineReasonRow};

fn fake() -> Arc<FakeSession> {
    Arc::new(demo_fake())
}

fn model(f: &Arc<FakeSession>) -> DocumentModel {
    DocumentModel::new(f.clone()).unwrap()
}

/// The app's offer when the window opens (outside the channel).
fn opened(f: &Arc<FakeSession>) -> DocumentModel {
    let mut m = model(f);
    m.offer_relink_if_due(false);
    m
}

fn media(id: u64, name: &str, offline: Option<OfflineReasonRow>) -> MediaRow {
    MediaRow {
        id,
        name: name.into(),
        path: format!("Media/{name}.caf"),
        channels: 2,
        frames: 48_000,
        sample_rate: 48_000,
        origin: MediaOriginRow::Imported,
        original_name: None,
        offline,
        uses: 1,
        used_percent: 100.0,
        peak: None,
    }
}

fn has_relink_banner(m: &DocumentModel) -> bool {
    m.banners().iter().any(|b| {
        matches!(
            b,
            ProjectBanner::Job {
                kind: JobKindRow::Relink,
                ..
            }
        )
    })
}

#[test]
fn the_sheet_says_what_swift_says() {
    assert_eq!(
        TITLE,
        "Some audio files of this project are missing or changed"
    );
    assert_eq!(
        MESSAGE,
        "Their regions stay silent until each item is relinked to its file. Each relink is one undo step."
    );
    assert_eq!(LOCATE, "Locate\u{2026}");
    assert_eq!(SEARCH_FOLDER, "Search Folder\u{2026}");
    assert_eq!(LEAVE_OFFLINE, "Leave Offline");
    assert_eq!(locate_label("kick"), "Locate kick");
    assert_eq!(
        HELD_HELP,
        "Finish or cancel the DAWproject merge waiting to be confirmed first."
    );
    // Swift `poolRowsSayWhatTheWindowShows`'s reason texts.
    assert_eq!(reason_text(OfflineReasonRow::Missing), "Missing");
    assert_eq!(
        reason_text(OfflineReasonRow::Unreadable),
        "Not a VizDAW file"
    );
    assert_eq!(reason_text(OfflineReasonRow::Format), "Another format");
    assert_eq!(reason_text(OfflineReasonRow::Length), "Changed length");
    assert_eq!(reason_text(OfflineReasonRow::Changed), "Changed");
    let r = media(1, "kick", Some(OfflineReasonRow::Missing));
    assert_eq!(detail(&r), "Missing \u{b7} Media/kick.caf");
}

/// Swift `relinkAndSearchRunAsRelinkJobs`.
#[test]
fn relink_and_search_run_as_relink_jobs() {
    let f = fake();
    f.set_media(vec![media(3, "kick", Some(OfflineReasonRow::Missing))]);
    f.script_job(JobScript::done().steps(&[0.5]).created(&[3]));
    let mut m = model(&f);
    m.relink(3, "/x/kick.wav");
    let started = f.jobs_started();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].kind, JobKindRow::Relink);
    assert_eq!(
        started[0].request,
        JobRequest::Relink {
            media: 3,
            path: "/x/kick.wav".into()
        }
    );
    assert_eq!(m.jobs()[0].kind, JobKindRow::Relink);
    assert!(has_relink_banner(&m));
    assert_eq!(m.banners()[0].text(), "Relinking media\u{2026}");
    let before = m.media_version();
    m.poll_jobs();
    assert_eq!(m.media_version(), before + 1);
    assert!(!has_relink_banner(&m));
    assert!(m.alert.is_none());

    f.script_job(JobScript::done().problems(&["kick: not found in /x"]));
    m.search_folder("/x");
    assert_eq!(f.jobs_started()[1].request, JobRequest::Search("/x".into()));
    m.poll_jobs();
    let alert = m.alert.clone().expect("an alert");
    assert_eq!(alert.title, "Relink Failed");
    assert!(alert.message.contains("not found"), "{alert:?}");
}

/// Swift `aPartialRelinkAnswersOkWithWarnings` (the model's half).
#[test]
fn a_partial_relink_is_a_problems_banner() {
    let f = fake();
    f.set_media(vec![
        media(3, "kick", Some(OfflineReasonRow::Missing)),
        media(4, "snare", Some(OfflineReasonRow::Missing)),
    ]);
    f.script_job(
        JobScript::done()
            .created(&[3])
            .problems(&["snare: not found in /x"]),
    );
    let mut m = model(&f);
    m.search_folder("/x");
    m.poll_jobs();
    assert!(m.alert.is_none());
    assert!(m.banners().contains(&ProjectBanner::Problems(vec![
        "snare: not found in /x".into()
    ])));
}

#[test]
fn a_refused_relink_is_the_relink_failed_alert() {
    let f = fake();
    f.set_media(vec![media(3, "kick", Some(OfflineReasonRow::Missing))]);
    f.fail_next(
        "relink_media",
        vizdaw_ffi::VizError::Model {
            message: "no such file".into(),
        },
    );
    let mut m = model(&f);
    m.relink(3, "/nope.wav");
    assert_eq!(
        m.alert.as_ref().map(|a| a.title.as_str()),
        Some("Relink Failed")
    );
    assert!(m.jobs().is_empty());
}

/// Swift `theRelinkSheetIsOfferedForMissingOrChangedMedia`.
#[test]
fn the_sheet_is_offered_for_missing_or_changed_media() {
    let f = fake();
    f.set_media(vec![media(1, "a", Some(OfflineReasonRow::Format))]);
    let m = opened(&f);
    assert!(!m.relink_shown(), "Convert handles another format");
    assert!(!m.shows_relink_sheet(false));

    let f = fake();
    f.set_media(vec![
        media(1, "a", Some(OfflineReasonRow::Changed)),
        media(2, "b", None),
    ]);
    let mut m = opened(&f);
    assert!(m.relink_shown());
    assert!(m.shows_relink_sheet(false));
    assert!(!m.shows_relink_sheet(true), "never under the channel");
    assert_eq!(m.relinkable().iter().map(|r| r.id).collect::<Vec<_>>(), [1]);
    m.leave_offline();
    assert!(!m.relink_shown());
    assert!(!m.shows_relink_sheet(false));
}

/// The offer is made once per cause: a second check with nothing new
/// leaves Leave Offline standing.
#[test]
fn the_offer_runs_once_per_cause() {
    let f = fake();
    f.set_media(vec![media(1, "a", Some(OfflineReasonRow::Missing))]);
    let mut m = opened(&f);
    assert!(m.relink_shown());
    m.leave_offline();
    m.offer_relink_if_due(false);
    assert!(!m.relink_shown(), "nothing is due");
}

/// Swift `theRelinkSheetIsOfferedWhenIdleFindsChangedMedia`.
#[test]
fn the_sheet_is_offered_when_idle_finds_changed_media() {
    let f = fake();
    f.set_media(vec![media(1, "a", None), media(2, "b", None)]);
    let mut m = opened(&f);
    assert!(!m.relink_shown());
    // The worker re-hashed a file after open: it is Changed now.
    f.set_media(vec![
        media(1, "a", Some(OfflineReasonRow::Changed)),
        media(2, "b", None),
    ]);
    let idle = |m: &mut DocumentModel| {
        f.set_idle_media_changed();
        m.idle();
        m.offer_relink_if_due(false);
    };
    idle(&mut m);
    assert!(m.relink_shown());
    m.leave_offline();
    idle(&mut m);
    assert!(!m.relink_shown(), "the same item is not offered twice");
    f.set_media(vec![
        media(1, "a", Some(OfflineReasonRow::Changed)),
        media(2, "b", Some(OfflineReasonRow::Missing)),
    ]);
    idle(&mut m);
    assert!(m.relink_shown(), "a new item is offered");
}

/// Swift `recheckMedia`: the app became active and files changed.
#[test]
fn a_recheck_that_found_changes_offers_the_sheet() {
    let f = fake();
    f.set_media(vec![media(1, "a", None)]);
    let mut m = opened(&f);
    f.set_media(vec![media(1, "a", Some(OfflineReasonRow::Missing))]);
    // Nothing changed: no new read, no offer.
    let before = m.media_version();
    assert!(!m.recheck_media());
    m.offer_relink_if_due(false);
    assert_eq!(m.media_version(), before);
    assert!(!m.relink_shown());
    f.set_recheck_media(true);
    assert!(m.recheck_media());
    assert_eq!(m.media_version(), before + 1);
    m.offer_relink_if_due(false);
    assert!(m.relink_shown());
}

/// An item that went back online is forgotten: a new loss is offered
/// again.
#[test]
fn an_item_back_online_is_offered_again_when_lost() {
    let f = fake();
    f.set_media(vec![media(1, "a", Some(OfflineReasonRow::Missing))]);
    let mut m = opened(&f);
    m.leave_offline();
    f.set_media(vec![media(1, "a", None)]);
    f.set_recheck_media(true);
    m.recheck_media();
    m.offer_relink_if_due(false);
    assert!(!m.relink_shown());
    f.set_media(vec![media(1, "a", Some(OfflineReasonRow::Missing))]);
    f.set_recheck_media(true);
    m.recheck_media();
    m.offer_relink_if_due(false);
    assert!(m.relink_shown());
}

/// Under the channel the sheet is never offered and nothing is marked
/// offered: once the channel is off the items are offered.
#[test]
fn under_the_channel_nothing_is_offered_or_marked() {
    let f = fake();
    f.set_media(vec![media(1, "a", Some(OfflineReasonRow::Missing))]);
    let mut m = model(&f);
    m.offer_relink_if_due(true);
    assert!(!m.relink_shown());
    m.offer_relink_if_needed(false);
    assert!(m.relink_shown(), "offered once the channel is off");
}

/// Relink Media… shows the sheet again; it shows only while an item is
/// left to relink.
#[test]
fn the_menu_shows_the_sheet_while_something_is_relinkable() {
    let f = fake();
    f.set_media(vec![media(1, "a", Some(OfflineReasonRow::Missing))]);
    let mut m = opened(&f);
    m.leave_offline();
    m.show_relink();
    assert!(m.shows_relink_sheet(false));
    f.set_media(vec![media(1, "a", None)]);
    f.set_recheck_media(true);
    m.recheck_media();
    assert!(!m.shows_relink_sheet(false), "nothing left to relink");
}

/// A held DAWproject merge blocks Locate… and Search Folder… (Swift
/// `relinkBlocked`, help `MergeModel.heldHelp`).
#[test]
fn a_held_merge_blocks_a_relink() {
    let f = fake();
    f.set_media(vec![media(1, "a", Some(OfflineReasonRow::Missing))]);
    let mut m = model(&f);
    assert!(!m.relink_blocked());
    m.set_merge_held(true);
    assert!(m.relink_blocked());
    m.relink(1, "/x/a.wav");
    m.search_folder("/x");
    assert!(
        f.jobs_started().is_empty(),
        "nothing starts beside a held merge"
    );
}
