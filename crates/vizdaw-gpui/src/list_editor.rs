//! The List editor in the bottom pane (Swift `ListEditorPane`, `ListBar`
//! and `ListEditorView`): a bar (Insert, Columns, Show and the row count),
//! a header row, then the rows, 18 points high in the Swift column widths,
//! every other one shaded. Only the rows in view are drawn and recorded.
//!
//! A click selects a row, ⇧-click extends the selection, ⌘-click (⌃ off
//! macOS) toggles one; the selection becomes the document's (its events and
//! tracks). A double-click or Return (on the selected row's Position) edits
//! a cell with one field over it; Tab and Shift-Tab move to the row's next
//! or previous editable cell, Return commits, Escape restores, and leaving
//! the field commits. Read-only cells never take the keyboard. While a cell
//! is edited the document has the text-editing owner `list.<column>` (the
//! bare transport keys wait); while the table has the keyboard the
//! document's list is focused (Return no longer returns to start; Space
//! still plays), and Delete or Forward Delete deletes the selected rows.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, AppContext, Bounds, Context, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyBinding, MouseButton, MouseDownEvent, ParentElement, Pixels, ScrollDelta,
    ScrollEvent, StatefulInteractiveElement, Styled, Subscription, Window, actions, anchored,
    canvas, deferred, div, hsla, point, px, rgb,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::editor::EditorTarget;
use vizdaw_app::list::{ListColumn, ListColumns, ListKind, ListSource, ListText, Smpte};

use crate::detail_pane;
use crate::registry::RegistryScope;
use crate::shell::DocumentWindow;
use crate::text_input::{TextInput, TextInputEvent};
use crate::transport_bar::colors;

actions!(list_editor, [EditRow, DeleteRows]);

/// The table's key context: its keys outrank the window's (Return edits
/// instead of returning to start).
pub const CONTEXT: &str = "ListTable";
/// The table's label (Swift's accessibility label).
pub const TABLE: &str = "Event List";
/// A row's height (Swift `table.rowHeight`).
pub const ROW_HEIGHT: f32 = 18.;
/// The Insert split button's menu part.
pub const INSERT_KIND: &str = "Insert Kind";
pub const COLUMNS: &str = "Columns";
pub const SHOW: &str = "Show";
pub const SHOW_EVERYTHING: &str = "Show Everything";
pub const SAVE_AS_DEFAULT: &str = "Save as Default";
pub const FRAME_RATE: &str = "SMPTE Frame Rate";

/// The Columns menu's toggles, in Swift's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColumnToggle {
    End,
    Length,
    Release,
    Smpte,
}

impl ColumnToggle {
    const ALL: [ColumnToggle; 4] = [
        ColumnToggle::End,
        ColumnToggle::Length,
        ColumnToggle::Release,
        ColumnToggle::Smpte,
    ];

    fn title(self) -> &'static str {
        match self {
            ColumnToggle::End => "End",
            ColumnToggle::Length => "Length",
            ColumnToggle::Release => "Release Velocity",
            ColumnToggle::Smpte => "SMPTE",
        }
    }

    fn flag(self, c: &mut ListColumns) -> &mut bool {
        match self {
            ColumnToggle::End => &mut c.end,
            ColumnToggle::Length => &mut c.duration,
            ColumnToggle::Release => &mut c.release,
            ColumnToggle::Smpte => &mut c.smpte,
        }
    }
}

/// The table's keys (only while it, or a cell in it, has the keyboard).
pub fn bindings() -> Vec<KeyBinding> {
    let c = Some(CONTEXT);
    vec![
        KeyBinding::new("enter", EditRow, c),
        KeyBinding::new("backspace", DeleteRows, c),
        KeyBinding::new("delete", DeleteRows, c),
    ]
}

/// A column's width (Swift `ListEditorView.width`).
pub fn width(c: ListColumn) -> f32 {
    match c {
        ListColumn::Position | ListColumn::End => 90.,
        ListColumn::Duration => 70.,
        ListColumn::Kind => 110.,
        ListColumn::Lane => 90.,
        ListColumn::Dest => 110.,
        ListColumn::Data1 => 90.,
        ListColumn::Data2 => 140.,
        ListColumn::Release => 60.,
        ListColumn::Smpte => 100.,
        ListColumn::Mute => 44.,
    }
}

/// A column's name in a text-editing owner (`list.<name>`; Swift's column
/// identifiers).
pub fn column_name(c: ListColumn) -> &'static str {
    match c {
        ListColumn::Position => "position",
        ListColumn::End => "end",
        ListColumn::Duration => "duration",
        ListColumn::Kind => "kind",
        ListColumn::Lane => "lane",
        ListColumn::Dest => "dest",
        ListColumn::Data1 => "data1",
        ListColumn::Data2 => "data2",
        ListColumn::Release => "release",
        ListColumn::Smpte => "smpte",
        ListColumn::Mute => "mute",
    }
}

fn owner(c: ListColumn) -> String {
    format!("list.{}", column_name(c))
}

/// Which bar menu is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BarMenu {
    Insert,
    Columns,
    Show,
}

/// How a cell edit ends (Swift's text movements).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Movement {
    Return,
    Cancel,
    Tab,
    BackTab,
    /// A click elsewhere, focus moving away.
    Other,
}

/// The cell being edited and its one field.
struct CellEdit {
    /// The list's target when the edit began: row IDs are the target's.
    target: EditorTarget,
    row: String,
    /// What the row showed when the edit began: an undo can move the map
    /// rows under their IDs (`t1` then names another change).
    source: ListSource,
    column: ListColumn,
    original: String,
    input: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

/// The table's state, owned by the document window.
pub struct ListEditor {
    focus: FocusHandle,
    /// The selected rows' IDs, in the order they were chosen, and as a set.
    selected: Vec<String>,
    selected_set: HashSet<String>,
    /// Where a ⇧-click range starts.
    anchor: Option<String>,
    /// The model's cursor the selection last followed.
    seen_cursor: Option<String>,
    /// The rows' scroll, in points.
    scroll: f32,
    /// A row to bring into view at the next draw.
    scroll_to: Option<String>,
    /// The rows' area at the last draw.
    body: Rc<Cell<Bounds<Pixels>>>,
    editing: Option<CellEdit>,
    pub(crate) menu: Option<BarMenu>,
    rows_drawn: Cell<usize>,
}

impl ListEditor {
    pub fn new(cx: &mut Context<DocumentWindow>) -> Self {
        ListEditor {
            focus: cx.focus_handle(),
            selected: Vec::new(),
            selected_set: HashSet::new(),
            anchor: None,
            seen_cursor: None,
            scroll: 0.,
            scroll_to: None,
            body: Rc::default(),
            editing: None,
            menu: None,
            rows_drawn: Cell::new(0),
        }
    }

    /// The table's keyboard focus.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The selected rows' IDs.
    pub fn selected(&self) -> Vec<String> {
        self.selected.clone()
    }

    /// The cell being edited: its row's ID and its column.
    pub fn editing(&self) -> Option<(String, ListColumn)> {
        self.editing.as_ref().map(|e| (e.row.clone(), e.column))
    }

    /// The edited cell's field.
    pub fn input(&self) -> Option<&Entity<TextInput>> {
        self.editing.as_ref().map(|e| &e.input)
    }

    /// How many rows the last frame drew (only those in view): a test seam.
    #[doc(hidden)]
    pub fn rows_drawn(&self) -> usize {
        self.rows_drawn.get()
    }

    /// Closes the bar's menu; false when none was open.
    pub(crate) fn close_menu(&mut self) -> bool {
        self.menu.take().is_some()
    }

    fn set_selected(&mut self, ids: Vec<String>) {
        self.selected_set = ids.iter().cloned().collect();
        self.selected = ids;
    }

    /// The selection follows the model's cursor when something else moved
    /// it (an edit, an insert, a delete, a new target), except while a
    /// cell is edited (Swift's table waits for that edit to end).
    fn follow(&mut self, d: &DocumentModel) {
        if self.editing.is_some() {
            return;
        }
        let cursor = d.list().cursor().map(str::to_owned);
        if cursor == self.seen_cursor {
            return;
        }
        self.seen_cursor = cursor.clone();
        match cursor {
            Some(c) => {
                if !self.selected_set.contains(&c) {
                    self.set_selected(vec![c.clone()]);
                }
                self.anchor = Some(c.clone());
                self.scroll_to = Some(c);
            }
            None => {
                self.set_selected(vec![]);
                self.anchor = None;
            }
        }
    }

    /// The rows' area height, `fallback` before the first draw.
    fn body_height(&self, fallback: f32) -> f32 {
        let h = f32::from(self.body.get().size.height);
        if h > 0. { h } else { fallback }
    }

    /// The rows of `count` in view, partly or wholly.
    fn visible(&self, count: usize, fallback: f32) -> std::ops::Range<usize> {
        let h = self.body_height(fallback);
        let first = (self.scroll / ROW_HEIGHT).floor().max(0.) as usize;
        let last = ((self.scroll + h) / ROW_HEIGHT).ceil().max(0.) as usize;
        first.min(count)..last.min(count)
    }

    /// Before drawing: follows the cursor, brings a row into view and
    /// keeps the scroll inside the rows. Answers what the window must do
    /// next turn.
    pub(crate) fn prepare(&mut self, d: &DocumentModel, fallback: f32) -> ListAfter {
        if !d.list_open() {
            self.menu = None;
            self.scroll = 0.;
            self.set_selected(vec![]);
            self.anchor = None;
            self.seen_cursor = None;
            return ListAfter {
                cancel_edit: self.editing.is_some(),
                closed: true,
            };
        }
        // A new target's rows are other rows: an edit begun on the old
        // one ends unsaved (a map row's ID names another row now).
        if self
            .editing
            .as_ref()
            .is_some_and(|e| d.list_target() != Some(e.target))
        {
            return ListAfter {
                cancel_edit: true,
                closed: false,
            };
        }
        self.follow(d);
        let h = self.body_height(fallback);
        if let Some(id) = self.scroll_to.take()
            && let Some(i) = d.list_index(&id)
        {
            let top = i as f32 * ROW_HEIGHT;
            if top < self.scroll {
                self.scroll = top;
            } else if top + ROW_HEIGHT > self.scroll + h {
                self.scroll = top + ROW_HEIGHT - h;
            }
        }
        let max = (d.list_rows().len() as f32 * ROW_HEIGHT - h).max(0.);
        self.scroll = self.scroll.clamp(0., max);
        ListAfter::default()
    }
}

/// Whether `now` is still the row `began` was: the same event, or the
/// same map entry (kind, place and value).
fn same_row(now: &ListSource, began: &ListSource) -> bool {
    match (now, began) {
        (ListSource::Event { row: a, .. }, ListSource::Event { row: b, .. }) => a.id == b.id,
        _ => now == began,
    }
}

/// What a draw found the window must do about the list next turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ListAfter {
    /// End the open cell edit unsaved.
    pub cancel_edit: bool,
    /// The list is no longer open.
    pub closed: bool,
}

/// One drawn row.
#[derive(Debug, Clone)]
pub struct RowState {
    pub id: String,
    pub summary: String,
    /// Each column's text, and whether it can be edited.
    pub cells: Vec<(String, bool)>,
    pub selected: bool,
}

/// What the list draws of the document this frame.
#[derive(Debug, Clone)]
pub struct ListState {
    pub count: usize,
    pub columns: Vec<ListColumn>,
    pub first: usize,
    pub rows: Vec<RowState>,
    pub last_kind: ListKind,
    pub insert_kinds: Vec<ListKind>,
    pub prefs: ListColumns,
    /// Each kind and whether it shows.
    pub kinds: Vec<(ListKind, bool)>,
    /// Each destination: its ID, name and whether it shows.
    pub dests: Vec<(u64, String, bool)>,
}

impl ListState {
    /// The rows in view only; their cells' text is made here, never for
    /// rows out of view.
    pub fn of(d: &DocumentModel, editor: &ListEditor, fallback: f32) -> Self {
        let rows = d.list_rows();
        let context = d.list_context();
        let columns = d.list_columns();
        let range = editor.visible(rows.len(), fallback);
        let first = range.start;
        let drawn = rows[range]
            .iter()
            .map(|r| RowState {
                id: r.id().to_owned(),
                summary: ListText::summary(r, &context),
                cells: columns
                    .iter()
                    .map(|&c| (ListText::cell(r, c, &context), ListText::editable(r, c)))
                    .collect(),
                selected: editor.selected_set.contains(r.id()),
            })
            .collect();
        let filter = d.list_filter();
        ListState {
            count: rows.len(),
            columns,
            first,
            rows: drawn,
            last_kind: d.list().last_insert_kind(),
            insert_kinds: d.list_insert_kinds(),
            prefs: d.list_prefs().columns,
            kinds: ListKind::ALL
                .into_iter()
                .map(|k| (k, !filter.hidden_kinds.contains(&k)))
                .collect(),
            dests: d
                .list_destinations()
                .iter()
                .map(|x| (x.id, x.name.clone(), d.list_shows_dest(x.id)))
                .collect(),
        }
    }
}

/// The List editor: its bar, then the table. `scope` is its own container.
pub(crate) fn render(
    editor: &ListEditor,
    state: &ListState,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(bar(editor, state, scope, window, cx))
        .child(detail_pane::divider())
        .child(table(editor, state, &mut scope.container(), cx))
        .into_any_element()
}

/// A bar button: `text` shown, recorded as a `button` labelled `label`.
fn bar_button(
    id: &'static str,
    text: &str,
    label: &str,
    width: f32,
    scope: &mut RegistryScope,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .relative()
        .h(px(22.))
        .w(px(width))
        .flex()
        .flex_none()
        .items_center()
        .px_2()
        .rounded_md()
        .cursor_pointer()
        .text_xs()
        .bg(rgb(colors::BUTTON))
        .text_color(rgb(colors::TEXT))
        .overflow_hidden()
        .whitespace_nowrap()
        .child(text.to_owned())
        .child(scope.control("button", label, true))
}

/// The bar (Swift `ListBar`): Insert, Columns, Show, then the row count.
fn bar(
    editor: &ListEditor,
    state: &ListState,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let title = format!("Insert {}", state.last_kind.title());
    let insert = bar_button("list-insert", &title, &title, 166., scope).on_click(cx.listener(
        |this, _, _, cx| {
            this.list_editor.menu = None;
            this.with_model(cx, |d| d.list_insert(None));
        },
    ));
    let mut kinds = bar_button("list-insert-kind", "\u{25be}", INSERT_KIND, 24., scope)
        .justify_center()
        .px_0()
        .on_click(cx.listener(|this, _, _, cx| this.toggle_list_menu(BarMenu::Insert, cx)));
    if editor.menu == Some(BarMenu::Insert) {
        let items = state
            .insert_kinds
            .iter()
            .map(|&k| {
                Entry::item(k.title(), None, false, move |this, cx| {
                    this.with_model(cx, |d| d.list_insert(Some(k)));
                })
            })
            .collect();
        kinds = kinds.children(menu(
            "list-insert-menu",
            items,
            &mut scope.container(),
            window,
            cx,
        ));
    }
    let mut columns = bar_button("list-columns", COLUMNS, COLUMNS, 100., scope)
        .on_click(cx.listener(|this, _, _, cx| this.toggle_list_menu(BarMenu::Columns, cx)));
    if editor.menu == Some(BarMenu::Columns) {
        let mut prefs = state.prefs;
        let mut items: Vec<Entry> = ColumnToggle::ALL
            .into_iter()
            .map(|t| {
                let on = *t.flag(&mut prefs);
                Entry::toggle(t.title(), on, move |this, cx| {
                    this.set_list_columns(|c| *t.flag(c) = !*t.flag(c), cx)
                })
            })
            .collect();
        items.push(Entry::Separator);
        items.push(Entry::Label(FRAME_RATE));
        for fps in Smpte::RATES {
            items.push(Entry::item(
                &format!("{fps} fps"),
                None,
                state.prefs.fps == fps,
                move |this, cx| this.set_list_columns(|c| c.fps = fps, cx),
            ));
        }
        columns = columns.children(menu(
            "list-columns-menu",
            items,
            &mut scope.container(),
            window,
            cx,
        ));
    }
    let mut show = bar_button("list-show", SHOW, SHOW, 80., scope)
        .on_click(cx.listener(|this, _, _, cx| this.toggle_list_menu(BarMenu::Show, cx)));
    if editor.menu == Some(BarMenu::Show) {
        let mut items: Vec<Entry> = state
            .kinds
            .iter()
            .map(|&(k, on)| {
                Entry::toggle(k.title(), on, move |this, cx| {
                    this.with_model(cx, |d| d.list_toggle_kind(k));
                })
            })
            .collect();
        items.push(Entry::Separator);
        for (id, name, on) in &state.dests {
            let id = *id;
            items.push(Entry::toggle(name, *on, move |this, cx| {
                this.with_model(cx, |d| d.list_toggle_dest(id));
            }));
        }
        items.push(Entry::Separator);
        items.push(Entry::item(SHOW_EVERYTHING, None, false, |this, cx| {
            this.with_model(cx, DocumentModel::list_show_everything);
        }));
        items.push(Entry::item(SAVE_AS_DEFAULT, None, false, |this, cx| {
            this.save_list_filter_as_default(cx)
        }));
        show = show.children(menu(
            "list-show-menu",
            items,
            &mut scope.container(),
            window,
            cx,
        ));
    }
    let count = format!("{} rows", state.count);
    div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .px(px(8.))
        .py(px(4.))
        .bg(rgb(colors::BAR))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_none()
                .gap(px(1.))
                .child(insert)
                .child(kinds),
        )
        .child(columns)
        .child(show)
        .child(div().flex_1())
        .child(
            div()
                .relative()
                .text_xs()
                .text_color(rgb(colors::DIM))
                .child(count.clone())
                .child(scope.control("text", &count, true)),
        )
        .into_any_element()
}

type Act = Rc<dyn Fn(&mut DocumentWindow, &mut Context<DocumentWindow>)>;

/// A bar menu's line.
enum Entry {
    Item {
        title: String,
        /// A toggle's "on" or "off".
        value: Option<&'static str>,
        selected: bool,
        act: Act,
    },
    Label(&'static str),
    Separator,
}

impl Entry {
    fn item(
        title: &str,
        value: Option<&'static str>,
        selected: bool,
        act: impl Fn(&mut DocumentWindow, &mut Context<DocumentWindow>) + 'static,
    ) -> Self {
        Entry::Item {
            title: title.to_owned(),
            value,
            selected,
            act: Rc::new(act),
        }
    }

    fn toggle(
        title: &str,
        on: bool,
        act: impl Fn(&mut DocumentWindow, &mut Context<DocumentWindow>) + 'static,
    ) -> Self {
        Self::item(title, Some(if on { "on" } else { "off" }), false, act)
    }
}

/// A bar menu: a backdrop that closes it, then its lines under the
/// button. Choosing an item closes it.
fn menu(
    id: &'static str,
    entries: Vec<Entry>,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> Vec<AnyElement> {
    let size = window.viewport_size();
    let backdrop = deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("list-menu-backdrop")
                .w(size.width)
                .h(size.height)
                .occlude()
                .on_any_mouse_down(cx.listener(|this, _, _, cx| {
                    this.list_editor.menu = None;
                    cx.notify();
                })),
        ),
    )
    .with_priority(1)
    .into_any_element();
    let mut list = crate::arrangement::menu_list(id);
    for (n, e) in entries.into_iter().enumerate() {
        list = match e {
            Entry::Separator => list.child(div().h(px(1.)).my_1().bg(rgb(colors::BORDER))),
            Entry::Label(title) => list.child(
                div()
                    .relative()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(rgb(colors::DIM))
                    .child(title)
                    .child(scope.control("text", title, true)),
            ),
            Entry::Item {
                title,
                value,
                selected,
                act,
            } => {
                let mark = if selected || value == Some("on") {
                    "\u{2713} "
                } else {
                    "    "
                };
                list.child(
                    div()
                        .id(("list-menu-item", n))
                        .relative()
                        .px_3()
                        .py_1()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(colors::BUTTON)))
                        .child(format!("{mark}{title}"))
                        .child(scope.control_selected("menuitem", &title, true, value, selected))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.list_editor.menu = None;
                            act(this, cx);
                            cx.notify();
                        })),
                )
            }
        };
    }
    let list = div()
        .absolute()
        .top_full()
        .left_0()
        .child(deferred(anchored().snap_to_window().child(list)).with_priority(2))
        .into_any_element();
    vec![backdrop, list]
}

/// The table: the header row, then the rows in view.
fn table(
    editor: &ListEditor,
    state: &ListState,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    editor.rows_drawn.set(state.rows.len());
    let mut header = div()
        .flex()
        .flex_row()
        .flex_none()
        .h(px(ROW_HEIGHT))
        .bg(rgb(colors::BAR))
        .text_xs()
        .text_color(rgb(colors::DIM));
    for &c in &state.columns {
        header = header.child(
            div()
                .relative()
                .flex_none()
                .w(px(width(c)))
                .px_1()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(c.title())
                .child(scope.control("text", c.title(), true)),
        );
    }
    let mut body_scope = scope.container();
    let mut rows = div()
        .absolute()
        .left_0()
        .top(px(state.first as f32 * ROW_HEIGHT - editor.scroll))
        .w_full()
        .flex()
        .flex_col();
    for (n, r) in state.rows.iter().enumerate() {
        rows = rows.child(row(
            editor,
            state,
            state.first + n,
            r,
            &mut body_scope.container(),
            cx,
        ));
    }
    let measured = editor.body.clone();
    let weak = cx.entity().downgrade();
    let measure = canvas(
        move |bounds, _, cx| {
            if measured.get().size != bounds.size {
                let weak = weak.clone();
                cx.defer(move |cx| {
                    weak.update(cx, |_, cx| cx.notify()).ok();
                });
            }
            measured.set(bounds);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    let body = div()
        .relative()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .child(measure)
        .child(rows)
        .child(body_scope.clip());
    div()
        .id("list-table")
        .key_context(CONTEXT)
        .track_focus(&editor.focus)
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .text_xs()
        .text_color(rgb(colors::TEXT))
        .on_action(cx.listener(|this, _: &EditRow, window, cx| this.list_return(window, cx)))
        .on_action(cx.listener(|this, _: &DeleteRows, _, cx| this.list_delete_rows(cx)))
        .on_scroll(cx.listener(|this, e: &ScrollEvent, _, cx| this.list_wheel(e, cx)))
        .child(header)
        .child(body)
        .child(scope.this("table", TABLE, true))
        .into_any_element()
}

/// One row: its cells (the edited one is the field), shaded every other
/// row, filled when selected.
fn row(
    editor: &ListEditor,
    state: &ListState,
    index: usize,
    r: &RowState,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let mut d = div()
        .relative()
        .flex()
        .flex_row()
        .flex_none()
        .h(px(ROW_HEIGHT))
        .w_full()
        .when(index % 2 == 1, |d| d.bg(hsla(0., 0., 1., 0.04)))
        .when(r.selected, |d| d.bg(rgb(colors::ON)));
    for (&column, (text, editable)) in state.columns.iter().zip(&r.cells) {
        let edited = editor
            .editing
            .as_ref()
            .filter(|e| e.row == r.id && e.column == column);
        let cell = div()
            .relative()
            .flex_none()
            .w(px(width(column)))
            .h(px(ROW_HEIGHT))
            .overflow_hidden()
            .whitespace_nowrap();
        d = d.child(match edited {
            Some(e) => {
                let value = e.input.read(cx).text().to_owned();
                cell.child(e.input.clone()).child(scope.control_with(
                    "textfield",
                    column.title(),
                    true,
                    Some(&value),
                    true,
                ))
            }
            None => {
                let id = r.id.clone();
                cell.px_1()
                    .when(!editable, |c| {
                        c.when(!r.selected, |c| c.text_color(rgb(colors::DIM)))
                    })
                    .child(text.clone())
                    .child(scope.control_with(
                        if *editable { "textfield" } else { "text" },
                        column.title(),
                        true,
                        Some(text),
                        false,
                    ))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                            this.list_press(&id, Some(column), e, window, cx)
                        }),
                    )
            }
        });
    }
    let id = r.id.clone();
    d.child(div().flex_1().h_full().on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
            this.list_press(&id, None, e, window, cx)
        }),
    ))
    .child(scope.this_selected("row", &r.summary, true, r.selected))
    .into_any_element()
}

impl DocumentWindow {
    /// The List editor's state.
    pub fn list_editor(&self) -> &ListEditor {
        &self.list_editor
    }

    /// Gives the table the keyboard, and the document's list is focused
    /// at once: not on the next drawn frame's focus events, which an
    /// inactive window (or one not drawing, as a background app on macOS)
    /// never sends.
    fn focus_list_table(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.list_editor.focus, cx);
        self.set_list_focused(true, cx);
    }

    fn set_list_focused(&mut self, on: bool, cx: &mut Context<Self>) {
        let app = self.app().clone();
        let now = app
            .read(cx)
            .store
            .document(self.doc())
            .map(DocumentModel::list_focused);
        if now.is_some_and(|f| f != on) {
            self.with_model(cx, |d| d.set_list_focused(on));
        }
    }

    /// While drawing: the document's list is focused exactly while the
    /// open list's table has the window's keyboard (the keyboard moved
    /// elsewhere, or the list closed, gives Return back).
    pub(crate) fn sync_list_focus(&mut self, open: bool, window: &Window, cx: &mut Context<Self>) {
        let on = open && self.list_editor.focus.is_focused(window);
        self.set_list_focused(on, cx);
    }

    /// The bottom pane is not shown: the list gives up the keyboard (a
    /// cell edit ends, committed), so no key acts on rows out of sight.
    pub(crate) fn list_hidden(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.list_editor.editing.is_none() && !self.list_editor.focus.is_focused(window) {
            return;
        }
        cx.defer_in(window, |this, window, cx| {
            this.end_list_cell(true, Movement::Other, window, cx);
            if this.list_editor.focus.is_focused(window) {
                window.focus(&this.focus_handle(cx), cx);
            }
            this.set_list_focused(false, cx);
        });
    }

    /// After a draw: an edit begun on another target, or on a list now
    /// closed, ends unsaved; a closed list's table gives the window the
    /// keyboard.
    pub(crate) fn list_after(
        &mut self,
        after: ListAfter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if after == ListAfter::default() {
            return;
        }
        cx.defer_in(window, move |this, window, cx| {
            if after.cancel_edit {
                this.end_list_cell(false, Movement::Other, window, cx);
            }
            if after.closed && this.list_editor.focus.is_focused(window) {
                window.focus(&this.focus_handle(cx), cx);
                this.set_list_focused(false, cx);
            }
        });
    }

    fn toggle_list_menu(&mut self, m: BarMenu, cx: &mut Context<Self>) {
        let e = &mut self.list_editor;
        e.menu = if e.menu == Some(m) { None } else { Some(m) };
        cx.notify();
    }

    /// The selection as it is now (the cursor may have moved since the
    /// last draw).
    fn list_follow(&mut self, cx: &mut Context<Self>) {
        let app = self.app().clone();
        let s = app.read(cx);
        if let Some(d) = s.store.document(self.doc()) {
            self.list_editor.follow(d);
        }
    }

    /// A press on a row (on `column`'s cell, or past the last one): it
    /// takes the keyboard and selects (⇧ a range from the anchor, ⌘ or ⌃
    /// off macOS toggles); a second press of a double-click edits the
    /// cell. An open cell edit ends first, committed.
    fn list_press(
        &mut self,
        id: &str,
        column: Option<ListColumn>,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The table's own focus-on-press would take the keyboard back
        // from a cell this press starts editing.
        window.prevent_default();
        self.list_editor.menu = None;
        self.end_list_cell(true, Movement::Other, window, cx);
        if e.click_count >= 2
            && let Some(column) = column
        {
            self.begin_list_cell(id, column, window, cx);
            return;
        }
        self.focus_list_table(window, cx);
        self.list_follow(cx);
        let m = e.modifiers;
        let toggle = if cfg!(target_os = "macos") {
            m.platform
        } else {
            m.control
        };
        let ids = {
            let app = self.app().clone();
            let s = app.read(cx);
            let Some(d) = s.store.document(self.doc()) else {
                return;
            };
            let ed = &self.list_editor;
            let anchor = ed.anchor.as_deref().and_then(|a| d.list_index(a));
            match (m.shift, anchor, d.list_index(id)) {
                (true, Some(a), Some(i)) => {
                    let rows = d.list_rows();
                    rows[a.min(i)..=a.max(i)]
                        .iter()
                        .map(|r| r.id().to_owned())
                        .collect()
                }
                _ if toggle => {
                    let mut ids = ed.selected.clone();
                    if ed.selected_set.contains(id) {
                        ids.retain(|x| x != id);
                    } else {
                        ids.push(id.to_owned());
                    }
                    ids
                }
                _ => vec![id.to_owned()],
            }
        };
        if !m.shift || self.list_editor.anchor.is_none() {
            self.list_editor.anchor = Some(id.to_owned());
        }
        self.list_editor.set_selected(ids.clone());
        let cursor = if self.list_editor.selected_set.contains(id) {
            Some(id.to_owned())
        } else {
            ids.last().cloned()
        };
        self.list_editor.seen_cursor = cursor.clone();
        self.with_model(cx, |d| {
            d.list_set_cursor(cursor);
            d.list_select(&ids);
        });
    }

    /// Return in the table: the selected row's Position is edited.
    fn list_return(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.list_follow(cx);
        let cursor = {
            let app = self.app().clone();
            let s = app.read(cx);
            s.store
                .document(self.doc())
                .and_then(|d| d.list().cursor().map(str::to_owned))
        };
        let ed = &self.list_editor;
        let row = cursor
            .filter(|c| ed.selected_set.contains(c))
            .or_else(|| ed.selected.last().cloned());
        if let Some(row) = row {
            self.begin_list_cell(&row, ListColumn::Position, window, cx);
        }
    }

    /// Delete or Forward Delete in the table: the selected rows go.
    fn list_delete_rows(&mut self, cx: &mut Context<Self>) {
        self.list_follow(cx);
        let ids: Vec<String> = {
            let app = self.app().clone();
            let s = app.read(cx);
            let Some(d) = s.store.document(self.doc()) else {
                return;
            };
            self.list_editor
                .selected
                .iter()
                .filter(|id| d.list_index(id).is_some())
                .cloned()
                .collect()
        };
        if ids.is_empty() {
            return;
        }
        self.with_model(cx, |d| d.list_delete(&ids));
    }

    fn list_wheel(&mut self, e: &ScrollEvent, cx: &mut Context<Self>) {
        let dy = match e.delta {
            ScrollDelta::Pixels(p) => f32::from(p.y),
            ScrollDelta::Lines(p) => p.y * ROW_HEIGHT,
        };
        // Kept inside the rows at the next draw.
        self.list_editor.scroll = (self.list_editor.scroll - dy).max(0.);
        cx.stop_propagation();
        cx.notify();
    }

    /// Edits cell `column` of row `id` (Return, Tab, a double-click):
    /// one field over it, its text all selected, with the keyboard. A
    /// read-only cell, a hidden column or a row not shown does nothing. An
    /// edit of another cell ends first, committed.
    pub fn begin_list_cell(
        &mut self,
        id: &str,
        column: ListColumn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .list_editor
            .editing
            .as_ref()
            .is_some_and(|e| e.row == id && e.column == column)
        {
            return;
        }
        let text = {
            let app = self.app().clone();
            let s = app.read(cx);
            s.store.document(self.doc()).and_then(|d| {
                let r = d.list_row(id)?;
                let target = d.list_target()?;
                (d.list_open()
                    && d.list_columns().contains(&column)
                    && ListText::editable(&r, column))
                .then(|| {
                    (
                        ListText::cell(&r, column, &d.list_context()),
                        target,
                        r.source.clone(),
                    )
                })
            })
        };
        let Some((text, target, source)) = text else {
            return;
        };
        self.end_list_cell(true, Movement::Other, window, cx);
        self.list_editor.menu = None;
        let input = cx.new(|cx| TextInput::new("", cx).cell().with_text(&text));
        let focus = input.focus_handle(cx);
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |this, _, event, window, cx| {
                let (commit, movement) = match event {
                    TextInputEvent::Confirm => (true, Movement::Return),
                    TextInputEvent::Cancel => (false, Movement::Cancel),
                    TextInputEvent::Tab => (true, Movement::Tab),
                    TextInputEvent::BackTab => (true, Movement::BackTab),
                };
                this.end_list_cell(commit, movement, window, cx);
            }),
            // The keyboard going anywhere else ends the edit, committed.
            cx.on_blur(&focus, window, |this, window, cx| {
                this.end_list_cell(true, Movement::Other, window, cx)
            }),
        ];
        window.focus(&focus, cx);
        // The cell has the keyboard, not the table.
        self.with_model(cx, |d| {
            d.begin_text_editing(&owner(column));
            d.set_list_focused(false);
        });
        self.list_editor.scroll_to = Some(id.to_owned());
        self.list_editor.editing = Some(CellEdit {
            target,
            row: id.to_owned(),
            source,
            column,
            original: text,
            input,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    /// Ends the cell edit: a changed text is committed unless `commit` is
    /// false (a refusal's message goes to the status line). Tab and
    /// Shift-Tab then edit the next or previous editable cell of the row;
    /// Return and Escape give the table the keyboard; otherwise the
    /// keyboard stays where it went (the table when nowhere).
    fn end_list_cell(
        &mut self,
        commit: bool,
        movement: Movement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(e) = self.list_editor.editing.take() else {
            return;
        };
        let text = e.input.read(cx).text().to_owned();
        let changed = commit && text != e.original;
        let (row, column) = (e.row.clone(), e.column);
        self.with_model(cx, |d| {
            d.end_text_editing(&owner(column));
            // The row must still be the one the edit began on (an event by
            // its ID; a map row by its kind, tick and value): else the
            // edit ends unsaved.
            let same = d
                .list_row(&row)
                .is_some_and(|r| same_row(&r.source, &e.source));
            if changed && same {
                d.list_commit(&text, &row, column);
            }
        });
        let table = self.list_editor.focus.clone();
        match movement {
            Movement::Tab | Movement::BackTab => {
                let next = {
                    let app = self.app().clone();
                    let s = app.read(cx);
                    s.store.document(self.doc()).and_then(|d| {
                        let r = d.list_row(&row)?;
                        let columns = d.list_columns();
                        let at = columns.iter().position(|&c| c == column)?;
                        let order: Vec<ListColumn> = if movement == Movement::Tab {
                            columns[at + 1..].to_vec()
                        } else {
                            columns[..at].iter().rev().copied().collect()
                        };
                        order.into_iter().find(|&c| ListText::editable(&r, c))
                    })
                };
                match next {
                    Some(c) => self.begin_list_cell(&row, c, window, cx),
                    None => self.focus_list_table(window, cx),
                }
            }
            Movement::Return | Movement::Cancel => self.focus_list_table(window, cx),
            Movement::Other => {
                let field = e.input.focus_handle(cx);
                if window.focused(cx).is_none_or(|f| f == field || f == table) {
                    self.focus_list_table(window, cx);
                }
            }
        }
        cx.notify();
    }

    /// The Columns menu: the app's list columns change, in every
    /// document. A preference that could not be written says so.
    fn set_list_columns(&mut self, change: impl FnOnce(&mut ListColumns), cx: &mut Context<Self>) {
        let doc = self.doc();
        self.app().clone().update(cx, |s, cx| {
            let mut c = s.prefs.list().columns;
            change(&mut c);
            let written = s.prefs.set_list_columns(c);
            s.push_list_prefs();
            if let Err(e) = written {
                s.store.with_document(doc, |d, _| d.set_status(Some(e)));
            }
            cx.notify();
        });
    }

    /// "Save as Default": this list's hidden kinds become the default.
    fn save_list_filter_as_default(&mut self, cx: &mut Context<Self>) {
        let doc = self.doc();
        self.app().clone().update(cx, |s, cx| {
            let Some(filter) = s.store.document(doc).map(DocumentModel::list_filter) else {
                return;
            };
            let written = s.prefs.set_list_default_filter(&filter);
            s.push_list_prefs();
            if let Err(e) = written {
                s.store.with_document(doc, |d, _| d.set_status(Some(e)));
            }
            cx.notify();
        });
    }
}
