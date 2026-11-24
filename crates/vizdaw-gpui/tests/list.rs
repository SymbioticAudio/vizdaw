//! The List editor in the bottom pane (Swift `ListEditorView` and
//! `ListBar`): its rows and columns, cell editing with one field over the
//! edited cell, the table's keys and selection, its nodes in the tree and
//! a long list drawn a screen at a time, driven through GPUI's test
//! platform with the scripted session.

use std::sync::Arc;

use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, ScrollDelta, ScrollEvent,
    TestAppContext, VisualTestContext, WindowHandle, point, px,
};
use vizdaw_app::DocumentStore;
use vizdaw_app::editor::{DetailEditorKind, EditorTarget};
use vizdaw_app::list::{ListColumn, ListKind};
use vizdaw_app::testing::{FakeSession, demo_fake, note_row, roll_fake};
use vizdaw_ffi::edit::{EditCommand, EventEdit};
use vizdaw_ffi::events::EventRow;
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::{AppState, DocumentWindow};

const BAR: i64 = 61_440;
const TABLE: &str = "Event List";
const FIRST: &str = "Note C4, bar 1 beat 1, velocity 64";
const SECOND: &str = "Note E4, bar 1 beat 2, velocity 64";

struct Setup {
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    cx: VisualTestContext,
}

fn setup(cx: &mut TestAppContext, fake: FakeSession) -> Setup {
    let fake = Arc::new(fake);
    let shared = fake.clone();
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(move || Ok(shared.clone())),
        Box::new(|_| Ok(Arc::new(demo_fake()))),
    );
    let (app, window) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        let window = vizdaw_gpui::new_demo(cx).expect("a demo window");
        (app, window)
    });
    let vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();
    Setup {
        fake,
        app,
        window,
        cx: vcx,
    }
}

/// Swift `listView()`: the roll fake with a List on Chords (notes 1 at
/// 1.1.0, C4, and 2 at 1.2.0, E4).
fn list_view(cx: &mut TestAppContext) -> Setup {
    list_on(cx, roll_fake())
}

fn list_on(cx: &mut TestAppContext, fake: FakeSession) -> Setup {
    let mut s = setup(cx, fake);
    s.update_model(|d| {
        d.open_detail(
            EditorTarget::Lane { seq: 3, lane: 31 },
            Some(DetailEditorKind::List),
        )
    });
    s
}

/// ⌘ on macOS, ⌃ elsewhere, as `menus::secondary!`.
fn secondary(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{key}")
    } else {
        format!("ctrl-{key}")
    }
}

/// ⌘ on macOS, ⌃ elsewhere.
fn secondary_mods() -> Modifiers {
    if cfg!(target_os = "macos") {
        Modifiers::command()
    } else {
        Modifiers::control()
    }
}

impl Setup {
    fn doc(&self) -> vizdaw_app::DocId {
        self.window.read_with(&self.cx, |v, _| v.doc()).unwrap()
    }

    fn model<R>(&mut self, f: impl FnOnce(&vizdaw_app::DocumentModel) -> R) -> R {
        let doc = self.doc();
        self.app
            .read_with(&self.cx, |a, _| f(a.store.document(doc).unwrap()))
    }

    fn update_model(&mut self, f: impl FnOnce(&mut vizdaw_app::DocumentModel)) {
        let doc = self.doc();
        self.app.update(&mut self.cx, |a, cx| {
            a.store.with_document(doc, |d, _| f(d));
            cx.notify();
        });
        self.cx.run_until_parked();
    }

    fn elements(&mut self) -> Vec<Element> {
        self.window
            .read_with(&self.cx, |v, _| v.registry().borrow().elements().to_vec())
            .unwrap()
    }

    fn find(&mut self, role: &str, label: &str) -> Option<Element> {
        self.elements()
            .into_iter()
            .find(|e| e.role == role && e.label == label)
    }

    fn element(&mut self, role: &str, label: &str) -> Element {
        let all = self.elements();
        self.find(role, label)
            .unwrap_or_else(|| panic!("no {role} {label:?} in {all:#?}"))
    }

    fn rows(&mut self) -> Vec<Element> {
        self.elements()
            .into_iter()
            .filter(|e| e.role == "row")
            .collect()
    }

    /// The cells of the row labelled `row`, in column order.
    fn cells(&mut self, row: &str) -> Vec<Element> {
        let all = self.elements();
        let r = all
            .iter()
            .find(|e| e.role == "row" && e.label == row)
            .unwrap_or_else(|| panic!("no row {row:?}"))
            .clone();
        all.into_iter()
            .filter(|e| {
                e.path.len() == r.path.len() + 1
                    && e.path.starts_with(&r.path)
                    && (e.role == "textfield" || e.role == "text")
            })
            .collect()
    }

    fn cell(&mut self, row: &str, column: &str) -> Element {
        self.cells(row)
            .into_iter()
            .find(|e| e.label == column)
            .unwrap_or_else(|| panic!("no {column} cell in {row:?}"))
    }

    fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.cx.run_until_parked();
    }

    fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
        self.cx.run_until_parked();
    }

    fn click_with(&mut self, e: &Element, modifiers: Modifiers, count: usize) {
        let position = e.bounds.center();
        for n in 1..=count {
            self.cx.simulate_event(MouseDownEvent {
                position,
                modifiers,
                button: MouseButton::Left,
                click_count: n,
                first_mouse: false,
            });
            self.cx.simulate_event(MouseUpEvent {
                position,
                modifiers,
                button: MouseButton::Left,
                click_count: n,
            });
        }
        self.cx.run_until_parked();
    }

    fn click(&mut self, role: &str, label: &str) {
        let e = self.element(role, label);
        self.click_with(&e, Modifiers::none(), 1);
    }

    fn click_row(&mut self, label: &str) {
        let e = self.element("row", label);
        self.click_with(&e, Modifiers::none(), 1);
    }

    /// Clicks the first row, if any is in the tree (none in a hidden pane).
    fn click_row_if_any(&mut self) {
        if let Some(r) = self.rows().into_iter().next() {
            self.click_with(&r, Modifiers::none(), 1);
        }
    }

    fn selected(&mut self) -> Vec<String> {
        self.window
            .read_with(&self.cx, |v, _| v.list_editor().selected())
            .unwrap()
    }

    fn editing(&mut self) -> Option<(String, ListColumn)> {
        self.window
            .read_with(&self.cx, |v, _| v.list_editor().editing())
            .unwrap()
    }

    fn begin(&mut self, row: &str, column: ListColumn) {
        self.window
            .update(&mut self.cx, |v, window, cx| {
                v.begin_list_cell(row, column, window, cx)
            })
            .unwrap();
        self.cx.run_until_parked();
    }

    fn transport_keys(&mut self) -> bool {
        self.model(|d| d.transport_keys_enabled())
    }
}

fn moved(id: u64, delta: i64) -> EditCommand {
    EditCommand::Event {
        edit: EventEdit::Move {
            events: vec![id],
            delta,
            to_lane: None,
        },
    }
}

#[gpui::test]
fn rows_and_columns_show(cx: &mut TestAppContext) {
    let mut s = list_view(cx);
    let rows: Vec<String> = s.rows().into_iter().map(|r| r.label).collect();
    assert_eq!(rows, [FIRST, SECOND]);
    let headers: Vec<String> = ListColumn::ALL
        .iter()
        .map(|c| c.title())
        .filter(|t| s.find("text", t).is_some_and(|e| e.value.is_none()))
        .map(str::to_owned)
        .collect();
    assert_eq!(
        headers,
        [
            "Position",
            "Length",
            "Kind",
            "Destination",
            "Data 1",
            "Data 2",
            "Mute"
        ]
    );
    let columns: Vec<String> = s.cells(SECOND).into_iter().map(|c| c.label).collect();
    assert_eq!(columns, headers);
    assert_eq!(s.cell(SECOND, "Position").value.as_deref(), Some("1.2.0"));
    assert_eq!(s.cell(SECOND, "Data 1").value.as_deref(), Some("E4"));
    assert!(
        s.find("text", "2 rows").is_some(),
        "the bar counts the rows"
    );
}

#[gpui::test]
fn editing_a_cell_turns_the_transport_keys_off_until_it_ends(cx: &mut TestAppContext) {
    let mut s = list_view(cx);
    s.begin("e1", ListColumn::Position);
    assert!(!s.transport_keys());
    assert_eq!(s.editing(), Some(("e1".into(), ListColumn::Position)));
    // One field, over the edited cell, with the keyboard.
    let field = s.cell(FIRST, "Position");
    assert_eq!(field.role, "textfield");
    assert!(field.focused);
    assert_eq!(field.value.as_deref(), Some("1.1.0"));
    s.type_text("2");
    s.keys("enter");
    assert!(s.transport_keys());
    assert_eq!(s.fake.edits().last(), Some(&moved(1, BAR)));
    assert_eq!(
        s.model(|d| d.list().cursor().map(str::to_owned)).as_deref(),
        Some("e1"),
        "the list follows the moved event"
    );
    assert_eq!(s.editing(), None);
    assert!(!s.cell(FIRST, "Position").focused);
}

#[gpui::test]
fn escape_leaves_the_event_alone(cx: &mut TestAppContext) {
    let mut s = list_view(cx);
    s.begin("e1", ListColumn::Position);
    s.type_text("9");
    s.keys("escape");
    assert!(s.fake.edits().is_empty());
    assert!(s.transport_keys());
    assert_eq!(s.editing(), None);
    assert_eq!(s.cell(FIRST, "Position").value.as_deref(), Some("1.1.0"));
}

#[gpui::test]
fn keys_delete_rows_and_return_edits_the_position(cx: &mut TestAppContext) {
    let mut s = list_view(cx);
    s.click_row(FIRST);
    assert_eq!(
        s.model(|d| d.list().cursor().map(str::to_owned)).as_deref(),
        Some("e1")
    );
    assert_eq!(s.selected(), ["e1"]);
    assert_eq!(s.model(|d| d.selection().events.clone()), [1]);
    // The table has the keyboard: Return no longer returns to start.
    assert!(s.model(|d| d.list_focused()));
    assert!(s.model(|d| !d.return_key_enabled() && d.transport_keys_enabled()));
    s.keys("backspace");
    assert_eq!(
        s.fake.edits().last(),
        Some(&EditCommand::Event {
            edit: EventEdit::Delete { events: vec![1] }
        })
    );
    // Forward Delete too.
    s.click_row(SECOND);
    s.keys("delete");
    assert_eq!(
        s.fake.edits().last(),
        Some(&EditCommand::Event {
            edit: EventEdit::Delete { events: vec![2] }
        })
    );
    s.click_row(SECOND);
    s.keys("enter");
    assert_eq!(s.editing(), Some(("e2".into(), ListColumn::Position)));
    assert!(s.cell(SECOND, "Position").focused);
    // The keyboard elsewhere gives Return back.
    s.keys("escape");
    let canvas = s.element("group", "Arrangement");
    s.click_with(&canvas, Modifiers::none(), 1);
    assert!(s.model(|d| !d.list_focused() && d.return_key_enabled()));
}

#[gpui::test]
fn the_table_is_in_the_tree(cx: &mut TestAppContext) {
    let mut s = list_view(cx);
    let table = s.element("table", TABLE);
    let rows = s.rows();
    assert_eq!(rows.len(), 2);
    for r in &rows {
        assert!(r.path.starts_with(&table.path) && r.path.len() > table.path.len());
        assert!(!r.selected);
    }
    // Editable cells are fields, read-only ones text; both read their text.
    let kind = s.cell(FIRST, "Kind");
    assert_eq!(
        (kind.role.as_str(), kind.value.as_deref()),
        ("text", Some("Note"))
    );
    let pitch = s.cell(SECOND, "Data 1");
    assert_eq!(
        (pitch.role.as_str(), pitch.value.as_deref()),
        ("textfield", Some("E4"))
    );
    assert_eq!(s.cell(FIRST, "Data 2").value.as_deref(), Some("64"));
    // A click selects; ⇧-click extends; ⌘-click (⌃ off macOS) toggles.
    s.click_row(FIRST);
    assert!(s.element("row", FIRST).selected);
    assert!(!s.element("row", SECOND).selected);
    let second = s.element("row", SECOND);
    s.click_with(&second, Modifiers::shift(), 1);
    assert_eq!(s.selected(), ["e1", "e2"]);
    assert_eq!(s.model(|d| d.selection().events.clone()), [1, 2]);
    assert!(s.element("row", SECOND).selected);
    let first = s.element("row", FIRST);
    s.click_with(&first, secondary_mods(), 1);
    assert_eq!(s.selected(), ["e2"]);
    assert_eq!(s.model(|d| d.selection().events.clone()), [2]);
    // The bar's controls by their titles.
    for label in ["Insert Note", "Columns", "Show"] {
        assert!(s.find("button", label).is_some(), "no {label}");
    }
}

#[gpui::test]
fn a_read_only_cell_never_takes_the_keyboard(cx: &mut TestAppContext) {
    let mut s = list_view(cx);
    let kind = s.cell(FIRST, "Kind");
    s.click_with(&kind, Modifiers::none(), 2);
    assert_eq!(s.editing(), None);
    assert!(s.transport_keys());
    s.begin("e1", ListColumn::Kind);
    assert_eq!(s.editing(), None);
    assert!(s.transport_keys());
    // A double-click on an editable cell edits it.
    let pitch = s.cell(SECOND, "Data 1");
    s.click_with(&pitch, Modifiers::none(), 2);
    assert_eq!(s.editing(), Some(("e2".into(), ListColumn::Data1)));
    // Tab skips the read-only cells: Data 2 next; Shift-Tab back past Kind
    // to Length.
    s.keys("tab");
    assert_eq!(s.editing(), Some(("e2".into(), ListColumn::Data2)));
    s.keys("shift-tab shift-tab shift-tab");
    assert_eq!(s.editing(), Some(("e2".into(), ListColumn::Duration)));
    assert!(s.fake.edits().is_empty(), "unchanged text sends nothing");
}

#[gpui::test]
fn moving_straight_to_another_cell_commits(cx: &mut TestAppContext) {
    let mut s = list_view(cx);
    s.begin("e1", ListColumn::Position);
    s.type_text("3");
    s.begin("e2", ListColumn::Position);
    assert_eq!(s.fake.edits().last(), Some(&moved(1, 2 * BAR)));
    assert!(!s.transport_keys());
    assert_eq!(s.editing(), Some(("e2".into(), ListColumn::Position)));
    s.keys("enter");
    assert_eq!(s.selected(), ["e1"], "on the moved event");
    assert!(s.transport_keys());
    assert_eq!(s.fake.edits().len(), 1);
}

#[gpui::test]
fn space_plays_from_the_table_but_return_does_not_locate(cx: &mut TestAppContext) {
    let mut s = list_view(cx);
    s.click_row(SECOND);
    s.fake.clear_calls();
    s.keys("enter");
    assert!(
        !s.fake.calls().iter().any(|c| c.starts_with("locate")),
        "{:?}",
        s.fake.calls()
    );
    assert_eq!(s.editing(), Some(("e2".into(), ListColumn::Position)));
    s.keys("escape");
    assert!(
        s.model(|d| d.list_focused()),
        "Escape gives the table the keyboard"
    );
    s.keys("space");
    assert!(
        s.fake.calls().iter().any(|c| c.starts_with("play")),
        "{:?}",
        s.fake.calls()
    );
    // ⌘I inserts the last inserted kind (a Note).
    let before = s.fake.edits().len();
    s.keys(&secondary("i"));
    assert_eq!(s.fake.edits().len(), before + 1);
}

#[gpui::test]
fn a_long_list_draws_only_the_rows_in_view(cx: &mut TestAppContext) {
    let f = roll_fake();
    let events: Vec<EventRow> = (0..100_000)
        .map(|i| note_row(i + 1, i as i64 * 240, 60))
        .collect();
    f.set_events(3, 31, events);
    let mut s = list_on(cx, f);
    assert!(s.find("text", "100000 rows").is_some());
    let drawn = s
        .window
        .read_with(&s.cx, |v, _| v.list_editor().rows_drawn())
        .unwrap();
    assert!(drawn > 0 && drawn < 60, "drew {drawn} rows");
    let rows = s.rows();
    assert_eq!(rows.len(), drawn);
    let first = rows[0].label.clone();
    // Scrolling draws other rows, as few.
    let table = s.element("table", TABLE);
    s.cx.simulate_event(ScrollEvent {
        position: table.bounds.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-18_000.))),
        ..Default::default()
    });
    s.cx.run_until_parked();
    let rows = s.rows();
    assert!(rows.len() < 60);
    assert_ne!(rows[0].label, first);
    assert!(!rows[0].label.contains("bar 1 "), "{}", rows[0].label);
}

#[gpui::test]
fn the_bar_inserts_and_chooses_columns_and_kinds(cx: &mut TestAppContext) {
    let mut s = list_view(cx);
    s.click("button", "Insert Note");
    assert_eq!(s.fake.edits().len(), 1, "Insert inserts the last kind");
    // The split button's menu lists the kinds a track's list can insert.
    s.click("button", "Insert Kind");
    let items: Vec<String> = s
        .elements()
        .into_iter()
        .filter(|e| e.role == "menuitem")
        .map(|e| e.label)
        .collect();
    assert_eq!(
        items,
        ListKind::INSERTABLE
            .iter()
            .filter(|k| k.event_kind().is_some())
            .map(|k| k.title())
            .collect::<Vec<_>>()
    );
    s.click("menuitem", "Controller");
    assert_eq!(s.fake.edits().len(), 2);
    assert!(s.find("button", "Insert Controller").is_some());

    // Columns: End is an app preference every document takes.
    s.click("button", "Columns");
    assert_eq!(s.element("menuitem", "End").value.as_deref(), Some("off"));
    assert!(s.element("menuitem", "30 fps").selected);
    s.click("menuitem", "End");
    assert!(s.model(|d| d.list_prefs().columns.end));
    assert!(s.app.read_with(&s.cx, |a, _| a.prefs.list().columns.end));
    assert!(s.find("text", "End").is_some(), "the End column shows");

    // Show: a kind hidden hides its rows; Show Everything shows them.
    s.click("button", "Show");
    assert_eq!(s.element("menuitem", "Note").value.as_deref(), Some("on"));
    assert!(s.find("menuitem", "Piano").is_some(), "the destinations");
    s.click("menuitem", "Note");
    assert!(s.rows().iter().all(|r| !r.label.starts_with("Note")));
    s.click("button", "Show");
    s.click("menuitem", "Show Everything");
    assert!(s.rows().iter().any(|r| r.label == FIRST));
}

#[gpui::test]
fn closing_the_list_ends_an_edit_unsaved_and_gives_the_keys_back(cx: &mut TestAppContext) {
    let mut s = list_view(cx);
    s.begin("e1", ListColumn::Position);
    s.type_text("3");
    s.update_model(|d| d.close_detail());
    assert!(s.fake.edits().is_empty());
    assert_eq!(s.editing(), None);
    assert!(s.model(|d| d.transport_keys_enabled() && d.return_key_enabled()));
    // The table had the keyboard: closing gives it back too.
    s.update_model(|d| {
        d.open_detail(
            EditorTarget::Lane { seq: 3, lane: 31 },
            Some(DetailEditorKind::List),
        )
    });
    s.click_row(FIRST);
    assert!(s.model(|d| d.list_focused()));
    s.update_model(|d| d.close_detail());
    assert!(s.model(|d| !d.list_focused() && d.return_key_enabled()));
    assert!(s.find("table", TABLE).is_none());
}

/// A row click gives the table the keyboard even in a window that is not
/// the active one (as a channel click on a background app on macOS): the
/// document's list is focused at once, and Return edits the Position.
#[gpui::test]
fn a_row_click_focuses_the_table_in_an_inactive_window(cx: &mut TestAppContext) {
    let mut s = list_view(cx);
    s.cx.deactivate_window();
    s.click_row(FIRST);
    assert!(s.model(|d| d.list_focused()), "the table has the keyboard");
    assert!(s.model(|d| !d.return_key_enabled()));
    s.keys("enter");
    assert_eq!(s.editing(), Some(("e1".into(), ListColumn::Position)));
    assert!(s.model(|d| !d.list_focused()), "the cell has it now");
    s.keys("escape");
    assert!(s.model(|d| d.list_focused()), "back to the table");
    // The keyboard going elsewhere gives Return back.
    let canvas = s.element("group", "Arrangement");
    s.click_with(&canvas, Modifiers::none(), 1);
    assert!(s.model(|d| !d.list_focused() && d.return_key_enabled()));
}

/// A cell edit open when the list changes target ends unsaved: its row ID
/// (a map row's `t0`) names another sequence's row there.
#[gpui::test]
fn a_retarget_cancels_an_open_cell_edit(cx: &mut TestAppContext) {
    let f = roll_fake();
    f.set_info(2, vizdaw_app::testing::four_four(2));
    let mut s = setup(cx, f);
    s.update_model(|d| {
        d.open_detail(
            EditorTarget::Sequence { seq: 3 },
            Some(DetailEditorKind::List),
        )
    });
    s.begin("t0", ListColumn::Data1);
    assert_eq!(s.editing(), Some(("t0".into(), ListColumn::Data1)));
    s.type_text("90");
    s.update_model(|d| {
        d.open_detail(
            EditorTarget::Sequence { seq: 2 },
            Some(DetailEditorKind::List),
        )
    });
    assert_eq!(s.editing(), None, "the edit ends with the target");
    assert!(s.transport_keys());
    s.keys("enter");
    assert!(s.fake.edits().is_empty(), "{:?}", s.fake.edits());
}

/// Song (3) with a second tempo change at bar 2 (`t1`), and an undo step
/// to undo.
fn two_tempos() -> FakeSession {
    use vizdaw_ffi::events::TempoPointRow;
    let f = roll_fake();
    let mut info = vizdaw_app::testing::four_four(3);
    info.tempo.push(TempoPointRow {
        tick: BAR,
        bpm: 100.,
        ramp: false,
    });
    f.set_info(3, info);
    f
}

/// Undo moves the tempo map under the edited `t1`: it now names another
/// change (at bar 3).
fn undo_shifts_t1(f: &FakeSession) {
    use vizdaw_ffi::events::TempoPointRow;
    let mut info = vizdaw_app::testing::four_four(3);
    info.tempo.push(TempoPointRow {
        tick: 2 * BAR,
        bpm: 90.,
        ramp: false,
    });
    f.set_info(3, info);
}

fn tempo_list(cx: &mut TestAppContext) -> Setup {
    let f = two_tempos();
    let mut s = setup(cx, f);
    let mut undoable = s.fake.snap();
    undoable.version += 1;
    undoable.undo_label = Some("Insert Tempo".into());
    s.fake.set_idle(Some(undoable));
    s.update_model(|d| {
        d.idle();
        d.open_detail(
            EditorTarget::Sequence { seq: 3 },
            Some(DetailEditorKind::List),
        )
    });
    s
}

/// ⌘Z while a map row's cell is edited: the row the edit began on is no
/// longer `t1`'s, so Return writes nothing.
#[gpui::test]
fn undo_under_a_cell_edit_by_key_writes_nothing_elsewhere(cx: &mut TestAppContext) {
    let mut s = tempo_list(cx);
    s.begin("t1", ListColumn::Position);
    assert_eq!(s.editing(), Some(("t1".into(), ListColumn::Position)));
    s.type_text("4");
    undo_shifts_t1(&s.fake);
    s.keys(&secondary("z"));
    assert!(
        s.fake.calls().iter().any(|c| c == "undo"),
        "{:?}",
        s.fake.calls()
    );
    s.keys("enter");
    assert!(s.fake.edits().is_empty(), "{:?}", s.fake.edits());
    assert!(s.transport_keys());
}

/// The same with `edit.undo` run as the channel runs it.
#[gpui::test]
fn undo_under_a_cell_edit_by_channel_writes_nothing_elsewhere(cx: &mut TestAppContext) {
    let mut s = tempo_list(cx);
    s.begin("t1", ListColumn::Position);
    s.type_text("4");
    undo_shifts_t1(&s.fake);
    s.update_model(|d| {
        vizdaw_app::commands::run("edit.undo", &[], d).expect("undo runs");
    });
    s.keys("enter");
    assert!(s.fake.edits().is_empty(), "{:?}", s.fake.edits());
    // An unchanged row still commits.
    s.begin("t1", ListColumn::Position);
    s.type_text("5");
    s.keys("enter");
    assert_eq!(s.fake.edits().len(), 1, "{:?}", s.fake.edits());
}

/// A hidden bottom pane gives up the keyboard: Delete deletes nothing
/// out of sight, and an open cell edit ends (committed).
#[gpui::test]
fn a_hidden_pane_keeps_no_keyboard(cx: &mut TestAppContext) {
    let mut s = list_view(cx);
    s.click_row(FIRST);
    let second = s.element("row", SECOND);
    s.click_with(&second, Modifiers::shift(), 1);
    assert!(s.model(|d| d.list_focused()));
    s.keys(if cfg!(target_os = "macos") {
        "ctrl-cmd-down"
    } else {
        "ctrl-down"
    });
    assert!(s.model(|d| !d.list_focused() && d.return_key_enabled()));
    s.keys("backspace");
    s.keys("delete");
    assert!(s.fake.edits().is_empty(), "{:?}", s.fake.edits());

    // Shown again; a cell edit, then a window too short for the pane.
    s.update_model(|d| d.update_layout(|l| l.views.arrange.bottom.shown = true));
    s.begin("e1", ListColumn::Position);
    s.type_text("2");
    s.cx.simulate_resize(gpui::size(px(1000.), px(300.)));
    s.cx.run_until_parked();
    assert_eq!(s.editing(), None, "the edit ended");
    assert_eq!(s.fake.edits().last(), Some(&moved(1, BAR)), "committed");
    assert!(s.model(|d| !d.list_focused() && d.transport_keys_enabled()));
    s.click_row_if_any();
    s.keys("backspace");
    assert_eq!(s.fake.edits().len(), 1, "{:?}", s.fake.edits());
}

/// A file panel the test keeps up until it answers.
struct HeldPanel {
    done: std::cell::RefCell<
        Vec<futures::channel::oneshot::Sender<vizdaw_gpui::file_panels::Chosen>>,
    >,
}

impl vizdaw_gpui::file_panels::PanelBackend for HeldPanel {
    fn show(
        &self,
        _spec: &vizdaw_app::panels::PanelSpec,
        _place: &vizdaw_gpui::file_panels::PanelPlace,
        _window: Option<gpui::AnyWindowHandle>,
        done: futures::channel::oneshot::Sender<vizdaw_gpui::file_panels::Chosen>,
        _cx: &mut gpui::App,
    ) {
        self.done.borrow_mut().push(done);
    }
}

#[gpui::test]
fn the_windows_keys_do_nothing_while_a_file_panel_is_up(cx: &mut TestAppContext) {
    use vizdaw_gpui::file_panels::{PanelBackend, PanelPlace, choose, set_backend};
    let mut s = list_view(cx);
    s.click_row(FIRST);
    let held = std::rc::Rc::new(HeldPanel {
        done: Default::default(),
    });
    let backend: std::rc::Rc<dyn PanelBackend> = held.clone();
    s.cx.update(|_, cx| {
        set_backend(backend, cx);
        choose(
            &vizdaw_app::panels::IMPORT_MIDI,
            PanelPlace::default(),
            None,
            cx,
        )
        .detach();
    });
    s.cx.run_until_parked();
    assert_eq!(held.done.borrow().len(), 1);
    let edits = s.fake.edits().len();
    // Delete on the selected row, the transport's Space, a typed key.
    s.keys("backspace");
    s.keys("space");
    s.keys("enter");
    assert_eq!(s.fake.edits().len(), edits);
    assert_eq!(s.fake.count("play(3)"), 0);
    assert_eq!(s.editing(), None);
    // The panel answers: the keys work again.
    held.done
        .borrow_mut()
        .pop()
        .unwrap()
        .send(Ok(None))
        .unwrap();
    s.cx.run_until_parked();
    s.keys("backspace");
    assert_eq!(
        s.fake.edits().last(),
        Some(&EditCommand::Event {
            edit: EventEdit::Delete { events: vec![1] }
        })
    );
}
