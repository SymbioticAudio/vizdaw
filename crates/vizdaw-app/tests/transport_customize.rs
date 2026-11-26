//! The Customize Transport Bar model (Swift `TransportCustomizeTests`).

mod support;

use std::sync::Arc;

use core_control::ErrorCode;
use core_control::registry;
use serde_json::{Map, Value};
use support::{FakeSession, demo_fake};
use vizdaw_app::bar_items::{STALE_TOOLTIP, command_enabled};
use vizdaw_app::transport_bar::{Size, TransportItem as I, TransportLayout};
use vizdaw_app::transport_customize::{Customizer, LABEL_TOO_LONG, NEW_COMMAND};
use vizdaw_app::{DocumentModel, Prefs};

fn model(text: &str) -> Customizer {
    Customizer::new(&TransportLayout::parse(text).unwrap())
}

fn text(c: &Customizer) -> String {
    c.finish().unwrap().tokens()
}

fn command(id: &str, label: Option<&str>) -> I {
    I::Command {
        id: id.into(),
        label: label.map(Into::into),
    }
}

#[test]
fn cannot_remove_the_cluster() {
    let mut c = model("cluster,loop");
    c.insert(0, I::Space);
    let before = c.items().to_vec();
    assert!(!c.remove(1), "the cluster stays");
    assert!(!c.remove(99), "out of range");
    assert_eq!(c.items(), before);
    assert!(c.remove(0));
    assert_eq!(text(&c), "cluster,loop");
}

#[test]
fn ops_keep_order() {
    let mut c = model("cluster,loop,bar:large");
    assert!(c.insert(1, I::Tempo(Size::Large)));
    assert_eq!(text(&c), "cluster,tempo:large,loop,bar:large");
    assert!(c.insert(99, I::Space), "past the end appends");
    assert_eq!(text(&c), "cluster,tempo:large,loop,bar:large,space");
    assert!(!c.insert(0, I::Loop), "a built-in item is on the bar once");
    assert_eq!(text(&c), "cluster,tempo:large,loop,bar:large,space");
    c.move_right(0);
    assert_eq!(text(&c), "tempo:large,cluster,loop,bar:large,space");
    c.move_left(0);
    assert_eq!(
        text(&c),
        "tempo:large,cluster,loop,bar:large,space",
        "no further left"
    );
    c.move_right(4);
    assert_eq!(
        text(&c),
        "tempo:large,cluster,loop,bar:large,space",
        "no further right"
    );
    c.move_to(4, 0);
    assert_eq!(text(&c), "space,tempo:large,cluster,loop,bar:large");
    c.move_to(0, 99);
    assert_eq!(text(&c), "tempo:large,cluster,loop,bar:large,space");
    assert!(c.remove(2));
    assert_eq!(text(&c), "tempo:large,cluster,bar:large,space");
    c.set_size(2, Size::Small);
    c.set_size(3, Size::Small);
    assert_eq!(
        text(&c),
        "tempo:large,cluster,bar:small,space",
        "only bar, time and tempo come small"
    );
}

#[test]
fn selection_follows_its_item() {
    let mut c = model("cluster,loop,click");
    c.select(Some(2));
    c.move_to(2, 0);
    assert_eq!(c.selected(), Some(0));
    c.move_left(0);
    assert_eq!(c.selected(), Some(0));
    c.move_right(0);
    assert_eq!(c.selected(), Some(1));
    c.select(Some(99));
    assert_eq!(c.selected(), None);
}

#[test]
fn palette_is_what_is_not_on_the_bar() {
    let mut c = model("cluster,loop,space,bar:small");
    let p = c.palette();
    let names: Vec<_> = p.iter().map(I::name).collect();
    assert!(!names.contains(&"cluster") && !names.contains(&"loop") && !names.contains(&"bar"));
    let tail: Vec<_> = names[names.len() - 3..].to_vec();
    assert_eq!(
        tail,
        ["space", "separator", "command"],
        "then space, separator, a command"
    );
    assert_eq!(p[p.len() - 1], command(NEW_COMMAND, None));
    assert_eq!(NEW_COMMAND, "window.mixer");
    // Every other built-in once, in the bar's order.
    let rest = &names[..names.len() - 3];
    assert_eq!(
        rest,
        [
            "click",
            "replace_overdub",
            "record_options",
            "count_in",
            "summary",
            "time",
            "tempo",
            "signature",
            "position_field",
            "edit_fields",
            "punch_fields",
            "dsp_load",
            "device",
            "faults",
            "main_view",
            "panes"
        ]
    );
    assert!(c.insert(1, I::Tempo(Size::Large)));
    assert!(!c.palette().iter().any(|i| i.name() == "tempo"));
    assert!(
        c.palette().iter().any(|i| i.name() == "space"),
        "spaces are always offered"
    );
}

#[test]
fn picker_by_title_and_menu() {
    let c = model("cluster");
    let ids = |q: &str| -> Vec<&'static str> { c.commands(q).into_iter().map(|r| r.0).collect() };
    assert!(ids("mix").contains(&"window.mixer"));
    let file = ids("File");
    assert!(file.contains(&"file.save"), "by menu");
    assert_eq!(c.commands("").len(), registry::COMMANDS.len());
    assert_eq!(c.commands("  ").len(), registry::COMMANDS.len());
    assert_eq!(ids("WINDOW.MIXER"), ["window.mixer"], "by id, any case");
    let order: Vec<_> = registry::COMMANDS.iter().map(|r| r.id).collect();
    let expect: Vec<_> = order.iter().copied().filter(|i| file.contains(i)).collect();
    assert_eq!(file, expect, "registry order");
    assert!(ids(&"x".repeat(10_000)).is_empty());
    let row = c.commands("window.mixer").remove(0);
    assert_eq!((row.1, row.2), ("Mixer", "Window"));
}

#[test]
fn labels_trimmed_and_bounded() {
    let mut c = model("cluster,command:window.mixer");
    assert_eq!(c.set_label(1, "  Go  "), Ok(()));
    assert_eq!(c.items()[1], command("window.mixer", Some("Go")));
    assert_eq!(
        c.set_label(1, &"a".repeat(33)),
        Err("A label is at most 32 characters.".to_owned())
    );
    assert_eq!(LABEL_TOO_LONG, "A label is at most 32 characters.");
    assert_eq!(
        c.items()[1],
        command("window.mixer", Some("Go")),
        "33 refused"
    );
    assert_eq!(c.set_label(1, &"a".repeat(32)), Ok(()));
    assert!(c.set_command(1, "file.save"));
    assert_eq!(c.set_label(1, "   "), Ok(()));
    assert_eq!(
        c.items()[1],
        command("file.save", None),
        "blank is the title"
    );
    assert!(c.set_label(0, "x").is_err(), "not a command button");
    assert!(!c.set_command(1, "bad id,x"));
    assert!(!c.set_command(0, "file.save"));
    assert_eq!(c.set_label(1, "Save, then: go 100%"), Ok(()));
    let layout = c.finish().unwrap();
    assert_eq!(TransportLayout::parse(&layout.tokens()), Ok(layout));
}

#[test]
fn stale_commands_greyed() {
    let mut c = model("cluster,command:gone.command,command:window.mixer");
    assert_eq!(c.stale(1).as_deref(), Some(STALE_TOOLTIP));
    assert_eq!(c.stale(2), None);
    assert_eq!(c.stale(0), None);
    assert_eq!(c.stale(99), None);
    // Kept, not dropped, and still a layout.
    assert_eq!(
        text(&c),
        "cluster,command:gone.command,command:window.mixer"
    );
    assert!(c.set_command(1, "file.save"));
    assert_eq!(c.stale(1), None);
}

#[test]
fn availability_follows_menus() {
    // The picker offers registry commands, and a chosen one is enabled as
    // its menu item is.
    let c = model("cluster");
    let m = DocumentModel::new(Arc::new(demo_fake())).unwrap();
    let empty = DocumentModel::new(Arc::new(FakeSession::new(vec![]))).unwrap();
    assert!(c.commands("play").iter().any(|r| r.0 == "transport.play"));
    assert!(command_enabled("transport.play", Some(&m)));
    assert!(!command_enabled("transport.play", Some(&empty)));
    assert!(!command_enabled("window.mixer", None));
    assert!(command_enabled("ui.customize_transport", None), "app-wide");
}

#[test]
fn customize_transport_needs_a_dialog() {
    let spec = registry::find("ui.customize_transport").unwrap();
    let e = registry::validate_args(spec, &Map::<String, Value>::new()).unwrap_err();
    assert_eq!(e.code, ErrorCode::DialogRequired);
    assert_eq!(e.dialog.as_deref(), Some("Customize Transport Bar"));
    // And the app-wide runner answers the same, without touching the store.
    let dir = tempfile::tempdir().unwrap();
    let path = Prefs::file_in(dir.path());
    let mut p = Prefs::at(&path);
    let e = vizdaw_app::commands::run_app("ui.customize_transport", &[], &mut p).unwrap_err();
    assert_eq!(e.code, ErrorCode::DialogRequired);
    assert_eq!(e.dialog.as_deref(), Some("Customize Transport Bar"));
    assert!(vizdaw_app::commands::APP_COMMANDS.contains(&"ui.customize_transport"));
    assert!(p.raw("transportBar").is_none());
}

#[test]
fn an_invalid_layout_is_never_written() {
    let mut c = model("cluster,loop");
    // The model refuses what would break a rule, leaving the draft.
    assert!(!c.insert(2, I::Loop));
    assert!(!c.insert(2, command("a,b", None)));
    assert!(!c.insert(2, command("file.save", Some(" padded "))));
    assert!(!c.insert(2, I::Cluster));
    for _ in 0..62 {
        assert!(c.insert(2, I::Space));
    }
    assert!(!c.insert(2, I::Space), "at most 64");
    assert_eq!(c.items().len(), 64);
    let layout = c.finish().unwrap();
    assert_eq!(layout.items.len(), 64);
    // A model whose items are not a layout finishes with an error, and
    // the preference refuses it too.
    let dir = tempfile::tempdir().unwrap();
    let mut p = Prefs::at(Prefs::file_in(dir.path()));
    let broken = TransportLayout {
        items: vec![I::Loop],
    };
    assert!(p.set_transport_bar(&broken).is_err());
    assert!(p.raw("transportBar").is_none());
}

#[test]
fn distinct_command_buttons() {
    let mut c = model("cluster");
    assert!(c.insert(1, command("window.mixer", None)));
    assert!(c.insert(2, command("file.save", None)));
    assert!(
        c.insert(3, command("window.mixer", None)),
        "commands repeat"
    );
    assert_eq!(
        text(&c),
        "cluster,command:window.mixer,command:file.save,command:window.mixer"
    );
    assert!(c.set_command(2, "file.new"));
    assert_eq!(c.items()[1], command("window.mixer", None));
    assert_eq!(c.items()[2], command("file.new", None));
}

#[test]
fn a_move_past_the_selection_keeps_it_on_its_item() {
    // Selected: loop (index 2).
    let mut c = model("cluster,click,loop,bar:large,tempo:large");
    c.select(Some(2));
    // An item before it moves after it.
    c.move_to(1, 3);
    assert_eq!(text(&c), "cluster,loop,bar:large,click,tempo:large");
    assert_eq!(c.selected(), Some(1), "still on loop");
    // An item after it moves before it.
    c.move_to(3, 0);
    assert_eq!(text(&c), "click,cluster,loop,bar:large,tempo:large");
    assert_eq!(c.selected(), Some(2), "still on loop");
    // A move that does not cross it leaves it.
    c.move_to(3, 4);
    assert_eq!(text(&c), "click,cluster,loop,tempo:large,bar:large");
    assert_eq!(c.selected(), Some(2));
    // The selected item itself still follows.
    c.move_to(2, 4);
    assert_eq!(c.selected(), Some(4));
}

#[test]
fn an_insert_before_the_selection_shifts_it() {
    let mut c = model("cluster,loop,bar:large");
    c.select(Some(1));
    assert!(c.insert(0, I::Space));
    assert_eq!(text(&c), "space,cluster,loop,bar:large");
    assert_eq!(c.selected(), Some(2), "still on loop");
    assert!(c.insert(2, I::Separator), "at the selection: it moves on");
    assert_eq!(c.selected(), Some(3));
    assert!(c.insert(99, I::Space), "after it: it stays");
    assert_eq!(c.selected(), Some(3));
    assert!(!c.insert(0, I::Loop), "a refused insert changes nothing");
    assert_eq!(c.selected(), Some(3));
}

#[test]
fn drops_move_add_and_remove() {
    use vizdaw_app::transport_customize::DragSource as D;
    let mut c = model("cluster,loop,bar:large");
    // A bar item dropped on slot 3 (the end) ends last.
    assert!(c.drop_on_bar(D::Bar(1), 3));
    assert_eq!(text(&c), "cluster,bar:large,loop");
    // Dropped where it already is: nothing changed.
    assert!(!c.drop_on_bar(D::Bar(1), 1));
    assert!(!c.drop_on_bar(D::Bar(1), 2));
    // A palette item is added before the slot.
    let first = c.palette()[0].clone();
    assert!(c.drop_on_bar(D::Palette(0), 1));
    assert_eq!(c.items()[1], first);
    assert!(!c.drop_on_bar(D::Palette(99), 0));
    assert!(!c.drop_on_bar(D::Bar(99), 0));
    // Back on the palette: removed, never the cluster.
    assert!(c.drop_on_palette(D::Bar(1)));
    assert!(!c.items().contains(&first));
    assert!(!c.drop_on_palette(D::Bar(0)), "the cluster stays");
    assert!(!c.drop_on_palette(D::Palette(0)));
}
