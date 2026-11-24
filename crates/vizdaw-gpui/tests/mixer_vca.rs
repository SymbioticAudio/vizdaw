//! VCA columns in the Mix view (Swift `VcaStripCells`, `VcaRenameField`,
//! `VcaMenuItems`, `StripVcaOffset`), through GPUI's test platform with
//! the scripted session: the column's labels and place, its fader drag
//! (and the press rules every drag keeps), level, M S and mode menu, the
//! rename popover and the VCA menu, the members' offset row and mark, and
//! the strips still fitting the window with groups, VCAs and MIDI strips.

mod mixer_support;

use gpui::{TestAppContext, point, px};
use mixer_support::{Setup, setup};
use vizdaw_app::mixer::{StripEntry, VcaStripLabels};
use vizdaw_app::testing::{ext_dest, group_row, groups_fake, vca_row};
use vizdaw_ffi::automation::GesturePhaseRow;
use vizdaw_ffi::events::AutomationTargetRow as T;
use vizdaw_ffi::mixer::MidiMixRow;
use vizdaw_gpui::registry::Element;

const VCA: T = T::VcaFader { vca: 9 };

/// Drums (9: Kick 2 and Snare 3, at −3 dB) over the groups' mixer.
fn with_vca(cx: &mut TestAppContext) -> Setup {
    setup(
        cx,
        groups_fake(vec![vca_row(9, "Drums", &[2, 3], -3.0)], vec![]),
    )
}

fn call(id: &str, args: &[&str]) -> (String, Vec<String>) {
    (id.to_owned(), args.iter().map(|a| a.to_string()).collect())
}

#[gpui::test]
fn a_vca_column_sits_before_master_with_the_swift_labels(cx: &mut TestAppContext) {
    let mut s = with_vca(cx);
    assert_eq!(
        s.fader_labels(),
        [
            "Fader of Kick",
            "Fader of Snare",
            "Fader of Bass",
            "Fader of Bus 1",
            "Fader of VCA Drums",
            "Fader of Master",
        ]
    );
    assert!(s.find("text", "VCA Drums").is_some(), "its title");
    assert!(s.find("text", "VCA \u{b7} 2 members").is_some());
    assert_eq!(
        s.value("slider", "Fader of VCA Drums").as_deref(),
        Some(StripEntry::level_text(-3.0).as_str())
    );
    assert_eq!(
        s.value("textfield", "Level of VCA Drums").as_deref(),
        Some(StripEntry::level_text(-3.0).as_str())
    );
    assert_eq!(s.value("button", "Mute VCA Drums").as_deref(), Some("off"));
    assert_eq!(s.value("button", "Solo VCA Drums").as_deref(), Some("off"));
    assert_eq!(
        s.value("menu", VcaStripLabels::MODE).as_deref(),
        Some("Read")
    );
    s.faders_line_up();
}

#[gpui::test]
fn members_show_the_vca_offset_and_mark(cx: &mut TestAppContext) {
    let mut s = with_vca(cx);
    let offset = VcaStripLabels::offset("VCA \u{2212}3.0");
    assert_eq!(s.all("text", &offset).len(), 2, "Kick's and Snare's");
    assert_eq!(
        s.value("text", &VcaStripLabels::mark("Kick")).as_deref(),
        Some("off")
    );
    assert!(s.find("text", &VcaStripLabels::mark("Bass")).is_none());
    // A member's own fader shows its own level (Swift).
    assert_eq!(
        s.value("slider", "Fader of Kick").as_deref(),
        Some(StripEntry::level_text(0.0).as_str())
    );
    // The mark lights while a VCA mutes the strip.
    s.fake.edit_mixer(|m| {
        m.strips.iter_mut().find(|x| x.id == 2).unwrap().vca_mark = true;
    });
    s.click("button", "Mute VCA Drums");
    assert_eq!(
        s.value("text", &VcaStripLabels::mark("Kick")).as_deref(),
        Some("on")
    );
    // The row sits under the level box, above M S.
    let level = s.element("textfield", "Level of Kick").bounds;
    let mark = s.element("text", &VcaStripLabels::mark("Kick")).bounds;
    let mute = s.element("button", "Mute Kick").bounds;
    assert!(mark.top() >= level.bottom() && mark.bottom() <= mute.top());
}

#[gpui::test]
fn a_vca_fader_drag_is_one_gesture(cx: &mut TestAppContext) {
    let mut s = with_vca(cx);
    let at = s.fader_at("Fader of VCA Drums");
    s.drag_up(at);
    let phases = s.of(VCA);
    assert!(matches!(phases[0], GesturePhaseRow::Touch { value: None }));
    assert_eq!(phases.last(), Some(&GesturePhaseRow::Release));
    assert_eq!(
        phases
            .iter()
            .filter(|p| matches!(p, GesturePhaseRow::Touch { .. }))
            .count(),
        1
    );
    assert!(s.fake.gestures().iter().all(|g| g.0 == VCA && !g.2));
    assert!(!s.pressed());
}

#[gpui::test]
fn a_vca_press_and_release_in_one_frame_leave_no_drag_open(cx: &mut TestAppContext) {
    let mut s = with_vca(cx);
    let at = s.fader_at("Fader of VCA Drums");
    s.press_and_release_in_one_frame(at);
    assert!(!s.pressed(), "no press left open");
    assert!(s.fake.gestures().is_empty(), "{:?}", s.fake.gestures());
}

#[gpui::test]
fn a_vca_drag_whose_mouse_up_was_lost_ends_at_the_next_press(cx: &mut TestAppContext) {
    let mut s = with_vca(cx);
    let at = s.fader_at("Fader of VCA Drums");
    s.down(at);
    s.move_to(point(at.x, at.y - px(10.)), true);
    s.move_to(point(at.x, at.y - px(30.)), true);
    assert!(s.pressed());
    // The up never came. The next press, anywhere (here the bar), ends
    // the drag at once, released once; dragging on from there moves
    // nothing.
    let elsewhere = s.element("button", "Add Bus").bounds.center();
    s.down(elsewhere);
    assert!(!s.pressed(), "the press ended the drag");
    let released = s.fake.gestures();
    assert_eq!(
        released.last().map(|g| (g.0, g.1)),
        Some((VCA, GesturePhaseRow::Release))
    );
    s.move_to(point(elsewhere.x, elsewhere.y + px(40.)), true);
    s.up(point(elsewhere.x, elsewhere.y + px(40.)), 1);
    assert_eq!(s.fake.gestures(), released, "nothing more moved");
    // A lost up followed by a click on a strip's mute: the VCA releases
    // before the mute.
    s.fake.clear_gestures();
    let at = s.fader_at("Fader of VCA Drums");
    s.down(at);
    s.move_to(point(at.x, at.y - px(10.)), true);
    s.move_to(point(at.x, at.y - px(20.)), true);
    s.click("button", "Mute Kick");
    let g = s.fake.gestures();
    let release = g
        .iter()
        .position(|g| g.0 == VCA && g.1 == GesturePhaseRow::Release)
        .expect("the VCA released");
    let mute = g
        .iter()
        .position(|g| g.0 == T::StripMute { strip: 2 })
        .expect("the mute clicked");
    assert!(release < mute, "{g:?}");
}

#[gpui::test]
fn a_typed_vca_level_is_one_click(cx: &mut TestAppContext) {
    let mut s = with_vca(cx);
    s.click("textfield", "Level of VCA Drums");
    s.cx.simulate_input("-6");
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(s.of(VCA).len(), 3, "{:?}", s.of(VCA));
    s.fake.clear_gestures();
    s.click("textfield", "Level of VCA Drums");
    s.cx.simulate_input("loud");
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert!(s.fake.gestures().is_empty());
    assert_eq!(s.status().as_deref(), Some(StripEntry::LEVEL_HINT));
}

#[gpui::test]
fn vca_mute_solo_and_mode_are_set_vca(cx: &mut TestAppContext) {
    let mut s = with_vca(cx);
    s.click("button", "Mute VCA Drums");
    s.click("button", "Solo VCA Drums");
    s.click("menu", VcaStripLabels::MODE);
    assert_eq!(
        s.items(),
        [
            ("Off".into(), true, false),
            ("Read".into(), true, true),
            ("Touch".into(), true, false),
            ("Latch".into(), true, false),
        ]
    );
    s.click("menuitem", "Latch");
    assert!(s.titles().is_empty());
    assert_eq!(
        s.mixer_calls(),
        [
            call("mixer.set_vca", &["vca=9", "mute=true"]),
            call("mixer.set_vca", &["vca=9", "solo=true"]),
            call("mixer.set_vca", &["vca=9", "mode=latch"]),
        ]
    );
}

#[gpui::test]
fn a_double_click_renames_the_vca_in_its_popover(cx: &mut TestAppContext) {
    let mut s = with_vca(cx);
    s.double_click("text", "VCA Drums");
    let field = s.element("textfield", VcaStripLabels::RENAME_FIELD);
    assert_eq!(field.value.as_deref(), Some("Drums"));
    assert!(field.focused);
    assert!(!s.model(|d| d.transport_keys_enabled()), "the keys wait");
    s.cx.simulate_input("Kit");
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(
        s.mixer_calls(),
        [call("mixer.rename_vca", &["vca=9", "name=Kit"])]
    );
    assert!(s.find("textfield", VcaStripLabels::RENAME_FIELD).is_none());
    assert!(s.model(|d| d.transport_keys_enabled()));
    // Escape, or a click away from the popover, cancels.
    s.double_click("text", "VCA Drums");
    s.cx.simulate_input("Gone");
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    s.double_click("text", "VCA Drums");
    s.cx.simulate_input("Gone");
    s.click_at(point(px(4.), px(4.)));
    assert!(s.find("textfield", VcaStripLabels::RENAME_FIELD).is_none());
    assert_eq!(s.mixer_calls().len(), 1, "{:?}", s.mixer_calls());
    assert!(s.model(|d| d.mixer().unwrap().renaming_vca().is_none()));
}

#[gpui::test]
fn the_vca_menu_renames_and_deletes(cx: &mut TestAppContext) {
    let mut s = with_vca(cx);
    s.right_click("text", "VCA Drums");
    assert_eq!(s.titles(), ["Rename VCA\u{2026}", "Delete VCA"]);
    s.click("menuitem", "Rename VCA\u{2026}");
    assert!(s.element("textfield", VcaStripLabels::RENAME_FIELD).focused);
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    s.right_click("text", "VCA Drums");
    s.click("menuitem", "Delete VCA");
    assert_eq!(s.mixer_calls(), [call("mixer.delete_vca", &["vca=9"])]);
}

/// Every mode menu (the strips' and the VCAs') inside `area`, its bottom
/// `gap` above the area's: the strips fill the area exactly.
fn modes_fill(all: &[Element], area: gpui::Bounds<gpui::Pixels>, gap: f32) {
    let modes: Vec<&Element> = all
        .iter()
        .filter(|e| {
            e.role == "menu"
                && (e.label.starts_with("Automation of ") || e.label == VcaStripLabels::MODE)
        })
        .collect();
    assert!(modes.iter().any(|e| e.label == VcaStripLabels::MODE));
    for e in modes {
        let below = f32::from(area.bottom() - e.bounds.bottom());
        assert!(
            (below - gap).abs() < 0.5,
            "{} at {:?}, {below} above the bottom of {:?}",
            e.label,
            e.bounds,
            area
        );
    }
}

#[gpui::test]
fn groups_vcas_and_midi_strips_still_fit_the_default_window(cx: &mut TestAppContext) {
    let f = groups_fake(
        vec![vca_row(9, "Drums", &[2, 3], -3.0)],
        vec![group_row(1, "Kit", &[2, 3])],
    );
    f.set_destinations(vec![ext_dest(7, "Juno", MidiMixRow::default())]);
    let mut s = setup(cx, f);
    let size = s.cx.update(|w, _| w.viewport_size());
    assert_eq!(
        (f32::from(size.width), f32::from(size.height)),
        (1440., 900.)
    );
    assert!(s.find("slider", "Fader of Juno").is_some());
    // The mixer's padding (8) and the strip's (10) under the mode menus.
    let gap = 18.;
    let area = s.area();
    let all = s.elements();
    modes_fill(&all, area, gap);
    // The status line comes: the strips shrink with the area.
    s.show_status("Nothing is playing.");
    let smaller = s.area();
    assert!(smaller.size.height < area.size.height);
    let all = s.elements();
    modes_fill(&all, smaller, gap);
    assert_eq!(s.view().read_with(&s.cx, |v, _| v.room()).1, 0.0);
    s.faders_line_up();
}

/// The review's scene: the demo's strips, two buses (Bus 2 sends to Bus
/// 1), Group 1 on Synth and Bus 1, VCA 1 on the buses at −3 dB.
fn review_scene() -> vizdaw_app::testing::FakeSession {
    use vizdaw_ffi::mixer::{BusRow, SendRow, StripKindRow};
    let f = vizdaw_app::testing::mixer_fake();
    f.edit_mixer(|m| {
        let mut b2 = vizdaw_app::testing::strip_row(3, "Bus 2", StripKindRow::Bus);
        b2.sends = vec![SendRow {
            target: 2,
            level_db: 0.0,
            pre_fader: false,
            enabled: true,
        }];
        m.strips.insert(2, b2);
        m.buses.push(BusRow {
            id: 3,
            name: "Bus 2".into(),
        });
        m.groups = vec![group_row(1, "Group 1", &[1, 2])];
        m.vcas = vec![vca_row(9, "VCA 1", &[2, 3], -3.0)];
        for s in &mut m.strips {
            if [1, 2].contains(&s.id) {
                s.group = Some(1);
            }
            if [2, 3].contains(&s.id) {
                s.vcas = vec![9];
                s.vca_offset_db = -3.0;
            }
        }
    });
    f
}

/// Every frame logged from `from` on: the centre inside the pane host's
/// area, and the strips made for the very area they were drawn in (the
/// strips fill it exactly, or scroll).
fn every_frame_fits(s: &mut Setup, from: (usize, usize)) {
    let shell = s.window.read_with(&s.cx, |v, _| v.fit_log()).unwrap();
    assert!(shell.len() > from.0, "a frame was drawn");
    for (area, centre) in &shell[from.0..] {
        let centre = centre.expect("the centre drawn");
        assert!(
            centre.bottom() <= area.bottom() + px(0.5),
            "the centre {centre:?} runs past the area {area:?}"
        );
    }
    let fits = s.view().read_with(&s.cx, |v, _| v.fits().to_vec());
    assert!(fits.len() > from.1, "the strips were drawn");
    for (strips, area) in &fits[from.1..] {
        assert!(
            (strips - area).abs() < 0.5,
            "strips {strips} tall made for an area {area} tall"
        );
    }
}

fn logged(s: &mut Setup) -> (usize, usize) {
    let shell = s.window.read_with(&s.cx, |v, _| v.fit_log().len()).unwrap();
    let fits = s.view().read_with(&s.cx, |v, _| v.fits().len());
    (shell, fits)
}

/// Review fix round 1: the strips fit the area of the frame they are drawn
/// in, on the very frame a status line comes or goes, and on the first
/// frame of a Mix view made again; never the area measured a frame
/// before.
#[gpui::test]
fn the_strips_fit_the_area_of_every_frame(cx: &mut TestAppContext) {
    let mut s = setup(cx, review_scene());
    assert!(s.find("menu", "Group Group 1").is_some());
    let before = logged(&mut s);
    s.show_status("Nothing is playing.");
    every_frame_fits(&mut s, before);
    let gap = 18.;
    let all = s.elements();
    modes_fill(&all, s.area(), gap);
    // The status line goes.
    let before = logged(&mut s);
    s.show_status_cleared();
    every_frame_fits(&mut s, before);
    // Arrange, then Mix again with the status line: a new placement fits
    // from its first frame.
    s.show_status("Nothing is playing.");
    s.cx.dispatch_action(vizdaw_gpui::RunCommand::new("ui.view_arrange"));
    s.cx.run_until_parked();
    let shell = s.window.read_with(&s.cx, |v, _| v.fit_log().len()).unwrap();
    s.cx.dispatch_action(vizdaw_gpui::RunCommand::new("ui.view_mix"));
    s.cx.run_until_parked();
    every_frame_fits(&mut s, (shell, 0));
    let all = s.elements();
    modes_fill(&all, s.area(), gap);
}

/// Review fix round 1: a press inside the rename popover (the field, its
/// padding) keeps the rename; what is typed there is the new name.
#[gpui::test]
fn a_click_into_the_rename_popover_keeps_the_rename(cx: &mut TestAppContext) {
    let mut s = with_vca(cx);
    s.double_click("text", "VCA Drums");
    s.click("textfield", VcaStripLabels::RENAME_FIELD);
    assert!(s.find("textfield", VcaStripLabels::RENAME_FIELD).is_some());
    // The padding beside the field.
    let field = s.element("textfield", VcaStripLabels::RENAME_FIELD).bounds;
    s.click_at(point(field.left() + px(2.), field.bottom() + px(4.)));
    assert!(s.find("textfield", VcaStripLabels::RENAME_FIELD).is_some());
    s.click("textfield", VcaStripLabels::RENAME_FIELD);
    s.cx.simulate_keystrokes("secondary-a");
    s.cx.simulate_input("Kit");
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(
        s.mixer_calls(),
        [call("mixer.rename_vca", &["vca=9", "name=Kit"])]
    );
}
