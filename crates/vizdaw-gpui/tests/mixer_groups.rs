//! Fader groups in the Mix view (Swift `StripGroupTag`, the fader's
//! `alone` at mouse-down), through GPUI's test platform with the scripted
//! session: the group tag and its menu, a group drag whose followers
//! show the core's values, and ⌥ at the press moving a fader alone.

mod mixer_support;

use gpui::{Modifiers, TestAppContext, point, px};
use mixer_support::{Setup, setup};
use vizdaw_app::mixer::{MixerMath, StripEntry};
use vizdaw_app::testing::{group_row, groups_fake};
use vizdaw_ffi::automation::{AutomationStateRow, GesturePhaseRow};
use vizdaw_ffi::events::AutomationTargetRow as T;

/// Kick (2) and Snare (3) in Drums (1), Bass (4) in none.
fn grouped(cx: &mut TestAppContext) -> Setup {
    setup(
        cx,
        groups_fake(vec![], vec![group_row(1, "Drums", &[2, 3])]),
    )
}

#[gpui::test]
fn a_grouped_strip_shows_its_group_tag(cx: &mut TestAppContext) {
    let mut s = grouped(cx);
    let tags = s.all("menu", "Group Drums");
    assert_eq!(tags.len(), 2, "Kick's and Snare's");
    assert!(tags.iter().all(|t| t.value.as_deref() == Some("Drums")));
    // Under each member's kind, inside its strip.
    let near = |s: &mut Setup, strip: &str| {
        let x = s
            .element("slider", &format!("Fader of {strip}"))
            .bounds
            .center()
            .x;
        tags.iter()
            .filter(|t| (t.bounds.center().x - x).abs() < px(60.))
            .count()
    };
    assert_eq!(near(&mut s, "Kick"), 1);
    assert_eq!(near(&mut s, "Snare"), 1);
    assert_eq!(near(&mut s, "Bass"), 0);
    // Every strip makes room for it: the faders still line up.
    s.faders_line_up();
}

#[gpui::test]
fn the_group_tag_menu_links_removes_and_ungroups(cx: &mut TestAppContext) {
    let mut s = grouped(cx);
    let tag = s.all("menu", "Group Drums")[0].bounds.center();
    s.click_at(tag);
    assert_eq!(
        s.items(),
        [
            ("Link Mutes".into(), true, true),
            ("Link Solos".into(), true, true),
            ("Remove from Group".into(), true, false),
            ("Ungroup".into(), true, false),
        ]
    );
    s.click("menuitem", "Link Mutes");
    assert!(s.titles().is_empty(), "the menu closes");
    s.click_at(tag);
    s.click("menuitem", "Link Solos");
    s.click_at(tag);
    s.click("menuitem", "Remove from Group");
    s.click_at(tag);
    s.click("menuitem", "Ungroup");
    let calls = s.mixer_calls();
    let call = |id: &str, args: &[&str]| {
        (
            id.to_owned(),
            args.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
        )
    };
    assert_eq!(
        calls,
        [
            call("mixer.set_group_links", &["group=1", "mute=false"]),
            call("mixer.set_group_links", &["group=1", "solo=false"]),
            call("mixer.remove_from_group", &["group=1", "strips=2"]),
            call("mixer.delete_group", &["group=1"]),
        ]
    );
}

#[gpui::test]
fn a_group_drag_shows_the_followers_and_option_moves_alone(cx: &mut TestAppContext) {
    let mut s = grouped(cx);
    let kick = T::StripFader { strip: 2 };
    let snare = T::StripFader { strip: 3 };
    let at = s.fader_at("Fader of Kick");
    s.down(at);
    s.move_to(point(at.x, at.y - px(10.)), true);
    s.move_to(point(at.x, at.y - px(40.)), true);
    // The core moves Snare with Kick: the view shows the core's value.
    let v = vizdaw_app::mixer::norm_of(kick, MixerMath::fader_position(-6.0));
    s.fake.set_automation_state(AutomationStateRow {
        modes: vec![],
        touched: vec![kick, snare],
        touched_values: vec![v, v],
        suspended: false,
        writing: false,
    });
    s.period();
    assert_eq!(
        s.value("slider", "Fader of Snare").as_deref(),
        Some(StripEntry::level_text(-6.0).as_str())
    );
    s.up(point(at.x, at.y - px(40.)), 1);
    let gestures = s.fake.gestures();
    assert!(gestures.iter().all(|g| g.0 == kick && !g.2), "{gestures:?}");
    assert_eq!(s.of(kick).last(), Some(&GesturePhaseRow::Release));
    assert!(s.of(snare).is_empty(), "the core moves the follower");
    // ⌥ at the press: alone, every phase.
    s.fake.clear_gestures();
    s.fake.set_automation_state(AutomationStateRow {
        modes: vec![],
        touched: vec![],
        touched_values: vec![],
        suspended: false,
        writing: false,
    });
    s.period();
    let at = s.fader_at("Fader of Kick");
    s.down_with(at, 1, Modifiers::alt());
    s.move_to(point(at.x, at.y - px(10.)), true);
    s.move_to(point(at.x, at.y - px(30.)), true);
    s.up(point(at.x, at.y - px(30.)), 1);
    let gestures = s.fake.gestures();
    assert!(!gestures.is_empty());
    assert!(gestures.iter().all(|g| g.0 == kick && g.2), "{gestures:?}");
}
