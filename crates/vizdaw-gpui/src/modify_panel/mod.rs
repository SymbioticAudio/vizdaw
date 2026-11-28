//! The Select and Modify panel (Swift `ModifyPanel`, `S/ModifyPanel.swift`):
//! a floating window per document, 620×460 and at least 580×320, titled
//! "Select and Modify — ‹title›". The top row has the Task picker and the
//! Templates menu; the body hosts the Select and Modify halves; then the
//! notice line, the bottom row (Template name, Save as Template, what
//! Apply acts on, Modify Again, Apply) and the document's status line.
//!
//! The panel's state is the document's [`SelectModifyModel`]
//! (`vizdaw_app::select_modify::model`): GPUI destroys a closed window, so a
//! reopened panel is rebuilt from the model and shows its config, drafts
//! and template name as they were. While the panel is the key window its
//! document's bare transport keys are off; Return is Apply; the menu
//! commands act on its document.
//!
//! [`SelectModifyModel`]: vizdaw_app::select_modify::model::SelectModifyModel

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable,
    InteractiveElement, IntoElement, MouseDownEvent, ParentElement, Render,
    StatefulInteractiveElement, Styled, Subscription, TitlebarOptions, Window, WindowBounds,
    WindowOptions, anchored, deferred, div, point, px, rgb, size,
};
use vizdaw_app::alerts::AlertItem;
use vizdaw_app::commands;
use vizdaw_app::select_modify::{MODIFY_AGAIN_HELP, ModifyConfig, PanelTask};
use vizdaw_app::{DocId, DocumentModel, WindowToken};
use vizdaw_ffi::mixer::DestRow;
use vizdaw_ffi::modify::{ConditionTest, ModifyRow};

use crate::app_state::{AppState, WindowKind, modify_panel_title};
use crate::menus::{CloseWindow, MODIFY_PANEL_CONTEXT, RunCommand};
use crate::registry::{RegistryScope, SharedRegistry};
use crate::strip_palette::tokens;
use crate::text_input::{TextInput, TextInputEvent};

mod modify_half;
mod select_half;
#[cfg(test)]
mod tests;

gpui::actions!(
    modify_panel,
    [
        /// The panel's default button: Return runs Apply.
        Apply
    ]
);

/// The size the panel opens at (`S/ModifyPanel.swift:36`).
pub const WIDTH: f32 = 620.;
pub const HEIGHT: f32 = 460.;
/// Its least size (`S/ModifyPanel.swift:64`).
pub const MIN_WIDTH: f32 = 580.;
pub const MIN_HEIGHT: f32 = 320.;
/// The Task picker's and the Templates menu's widths.
const TASK_WIDTH: f32 = 260.;
const TEMPLATES_WIDTH: f32 = 120.;
/// The Template name field's width.
const TEMPLATE_NAME_WIDTH: f32 = 180.;

/// The labels of the frame (`S/ModifyPanel.swift:64-400`); each is also
/// its control's accessibility label.
pub const REMOVE_CONDITION: &str = "Remove Condition";
pub const TASK: &str = "Task";
pub const SELECT: &str = "Select";
pub const MODIFY: &str = "Modify";
pub const TEMPLATES: &str = "Templates";
pub const NO_TEMPLATES: &str = "No Templates";
pub const TEMPLATE_NAME: &str = "Template name";
pub const SAVE_AS_TEMPLATE: &str = "Save as Template";
pub const MODIFY_AGAIN: &str = "Modify Again";
pub const APPLY: &str = "Apply";

/// The Templates menu's Load item for `name`.
pub fn load_item(name: &str) -> String {
    format!("Load \u{201c}{name}\u{201d}")
}

/// The Templates menu's Delete item for `name`.
pub fn delete_item(name: &str) -> String {
    format!("Delete \u{201c}{name}\u{201d}")
}

/// A menu or picker of the panel, while open (one at a time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelMenu {
    Task,
    Templates,
    /// The select half's.
    Result,
    Match,
    Comparison(usize),
    Kinds(usize),
    ConditionDest(usize),
    AddCondition,
    /// The modify half's.
    Operation,
    Root,
    Scale,
    ValueToChange,
    ChangeOp,
    Amount,
    Curve,
    Wave,
    Destination,
    Substitute,
}

/// A field of the halves: its input and what it was made with.
struct PanelField {
    placeholder: String,
    input: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

type Pick = Rc<dyn Fn(&mut ModifyPanel, &mut Window, &mut Context<ModifyPanel>)>;

/// One row of a panel menu.
enum MenuRow {
    Item {
        title: String,
        enabled: bool,
        checked: bool,
        pick: Pick,
    },
    Separator,
}

impl MenuRow {
    fn item(
        title: impl Into<String>,
        pick: impl Fn(&mut ModifyPanel, &mut Window, &mut Context<ModifyPanel>) + 'static,
    ) -> Self {
        MenuRow::Item {
            title: title.into(),
            enabled: true,
            checked: false,
            pick: Rc::new(pick),
        }
    }
}

/// A document's Select and Modify panel.
pub struct ModifyPanel {
    app: Entity<AppState>,
    doc: DocId,
    token: WindowToken,
    focus: FocusHandle,
    registry: SharedRegistry,
    /// The Template name field (the model's name is its text).
    template_name: Entity<TextInput>,
    /// The halves' fields by model key (`c<i>.<j>`, `m<j>`, `map`,
    /// `offsets`); each shows the model's text for its key.
    fields: BTreeMap<String, PanelField>,
    /// The keys drawn this frame (the rest are dropped after it).
    drawn: BTreeSet<String>,
    /// The open menu.
    menu: Option<PanelMenu>,
    /// The title last given to the platform window.
    title: String,
    /// The app state has listed this window (until then a notify that
    /// does not list it is not its closing).
    listed: bool,
    _subscriptions: Vec<Subscription>,
}

impl ModifyPanel {
    fn new(
        app: Entity<AppState>,
        doc: DocId,
        token: WindowToken,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let name = app
            .read(cx)
            .store
            .document(doc)
            .map(|d| d.select_modify().template_name.clone())
            .unwrap_or_default();
        let template_name =
            cx.new(|cx| TextInput::new(TEMPLATE_NAME, cx).compact().with_text(&name));
        let state = app.clone();
        cx.on_release(move |_, cx| {
            // The model stays: a reopened panel shows it as it was.
            state.update(cx, |s, cx| {
                s.remove_window(token);
                s.store.with_document(doc, |d, _| d.modify_panel_closed());
                cx.notify();
            });
        })
        .detach();
        let subscriptions = vec![
            cx.observe_in(&app, window, move |this, app, window, cx| {
                let s = app.read(cx);
                let listed = s.windows().iter().any(|w| w.token == this.token);
                // Its document went (closed, quit), or its entry did: the
                // panel goes too.
                if s.store.document(this.doc).is_none() || (this.listed && !listed) {
                    window.remove_window();
                    return;
                }
                this.listed |= listed;
                let name = s
                    .store
                    .document(this.doc)
                    .map(|d| d.select_modify().template_name.clone());
                // The title follows renames and Save As.
                if let Some(t) = s.store.document(this.doc).map(modify_panel_title)
                    && t != this.title
                {
                    window.set_window_title(&t);
                    this.title = t;
                }
                // The model's template name changed elsewhere (Modify ›
                // Templates › ‹name› loads one): the field shows it, so
                // Save as Template saves under the name on screen.
                if let Some(name) = name
                    && name != this.template_name.read(cx).text()
                {
                    this.template_name.update(cx, |f, cx| f.set_text(&name, cx));
                }
                // The halves' fields likewise follow the model (a new
                // operation, a removed row, a loaded template), and are
                // red while their text does not parse. A field being typed
                // in already agrees: its text is the model's draft.
                this.sync_fields(cx);
                cx.notify();
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                let active = window.is_window_active();
                let (doc, token) = (this.doc, this.token);
                this.app.update(cx, |s, cx| {
                    if active {
                        s.store.focus_document(doc, token);
                    }
                    s.store
                        .with_document(doc, |d, _| d.modify_panel_focused(active));
                    cx.notify();
                });
            }),
            cx.observe(&template_name, |this, field, cx| {
                let text = field.read(cx).text().to_owned();
                this.with_doc(cx, |d| d.select_modify_mut().template_name = text);
            }),
            cx.subscribe_in(&template_name, window, |this, _, e, window, cx| {
                if *e == TextInputEvent::Confirm {
                    this.apply(&Apply, window, cx);
                }
            }),
        ];
        ModifyPanel {
            app,
            doc,
            token,
            focus,
            registry: SharedRegistry::default(),
            template_name,
            fields: BTreeMap::new(),
            drawn: BTreeSet::new(),
            menu: None,
            title,
            listed: false,
            _subscriptions: subscriptions,
        }
    }

    /// The panel's document.
    pub fn doc(&self) -> DocId {
        self.doc
    }

    /// The field for model key `key`, while drawn.
    #[cfg(test)]
    fn field_for_test(&self, key: &str) -> Option<&Entity<TextInput>> {
        self.fields.get(key).map(|f| &f.input)
    }

    /// The elements this window drew last.
    pub fn registry(&self) -> &SharedRegistry {
        &self.registry
    }

    /// Runs `f` on the panel's document (nothing once it is closed), then
    /// redraws its windows.
    fn with_doc<R>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut DocumentModel) -> R,
    ) -> Option<R> {
        let doc = self.doc;
        let r = self.app.update(cx, |s, cx| {
            cx.notify();
            s.store.with_document(doc, |d, _| f(d))
        });
        cx.notify();
        r
    }

    /// Apply (Return): runs the config on the selection, or on an Exact
    /// Mode request's dragged events. A refusal is the status line.
    fn apply(&mut self, _: &Apply, _: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.with_doc(cx, |d| d.apply_modify());
    }

    fn modify_again(&mut self, cx: &mut Context<Self>) {
        // As the menu's: a hidden selection or nothing to repeat is the
        // status line.
        self.with_doc(cx, |d| {
            let _ = commands::perform_from_menu("modify.again", d);
        });
    }

    fn save_template(&mut self, cx: &mut Context<Self>) {
        self.with_doc(cx, |d| d.save_modify_template());
    }

    /// A menu command while the panel is key: its document's, as the
    /// document window runs it (after this dispatch).
    fn run_command(&mut self, action: &RunCommand, _: &mut Window, cx: &mut Context<Self>) {
        let (app, doc, action) = (self.app.clone(), self.doc, action.clone());
        cx.defer(move |cx| {
            let Some(h) = app
                .read(cx)
                .window_of(doc)
                .and_then(|w| w.document_window())
            else {
                return;
            };
            h.update(cx, |v, window, cx| v.run_command(&action, window, cx))
                .ok();
        });
    }

    fn toggle_menu(&mut self, menu: PanelMenu, cx: &mut Context<Self>) {
        self.menu = if self.menu == Some(menu) {
            None
        } else {
            Some(menu)
        };
        cx.notify();
    }

    /// The Task picker's items: choosing one starts the panel over for it
    /// (`open(task, filter:)`).
    fn task_items(current: PanelTask) -> Vec<MenuRow> {
        PanelTask::ALL
            .into_iter()
            .map(|task| {
                let mut row = MenuRow::item(task.title(), move |this, _, cx| {
                    this.with_doc(cx, |d| {
                        let filter = d.select_modify().filter().cloned();
                        d.open_modify(task, filter);
                    });
                });
                if let MenuRow::Item { checked, .. } = &mut row {
                    *checked = task == current;
                }
                row
            })
            .collect()
    }

    /// The Templates menu: "No Templates", else Load “‹name›” for each, a
    /// divider, then Delete “‹name›” for each.
    fn template_items(names: &[String]) -> Vec<MenuRow> {
        if names.is_empty() {
            return vec![MenuRow::Item {
                title: NO_TEMPLATES.into(),
                enabled: false,
                checked: false,
                pick: Rc::new(|_, _, _| {}),
            }];
        }
        let mut rows: Vec<MenuRow> = names
            .iter()
            .map(|name| {
                let name = name.clone();
                MenuRow::item(load_item(&name), move |this, _, cx| {
                    let loaded = this.with_doc(cx, |d| {
                        let t = d
                            .modify_templates()
                            .iter()
                            .find(|t| t.name == name)
                            .cloned();
                        if let Some(t) = &t {
                            d.load_modify_template(t);
                        }
                        t.map(|t| t.name)
                    });
                    if let Some(Some(n)) = loaded {
                        this.template_name.update(cx, |f, cx| f.set_text(&n, cx));
                    }
                })
            })
            .collect();
        rows.push(MenuRow::Separator);
        rows.extend(names.iter().map(|name| {
            let name = name.clone();
            MenuRow::item(delete_item(&name), move |this, _, cx| {
                this.with_doc(cx, |d| d.delete_modify_template(&name));
            })
        }));
        rows
    }

    /// The open menu's backdrop (a press outside closes it) and its list
    /// under its face.
    fn popup(
        &self,
        id: &'static str,
        rows: Vec<MenuRow>,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let t = tokens();
        let viewport = window.viewport_size();
        let backdrop = deferred(
            anchored().position(point(px(0.), px(0.))).child(
                div()
                    .id("modify-menu-backdrop")
                    .w(viewport.width)
                    .h(viewport.height)
                    .occlude()
                    .on_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.menu = None;
                        cx.notify();
                    })),
            ),
        )
        .with_priority(1)
        .into_any_element();
        let mut list = crate::arrangement::menu_list(id);
        for (n, row) in rows.into_iter().enumerate() {
            match row {
                MenuRow::Separator => list = list.child(crate::arrangement::menu_separator()),
                MenuRow::Item {
                    title,
                    enabled,
                    checked,
                    pick,
                } => {
                    let mut el = div()
                        .id(("modify-menu-item", n))
                        .relative()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .px_3()
                        .py_1()
                        .text_xs()
                        .child(
                            div()
                                .w(px(10.))
                                .child(if checked { "\u{2713}" } else { "" }),
                        )
                        .child(title.clone())
                        .child(scope.control_selected("menuitem", &title, enabled, None, checked));
                    if enabled {
                        el = el
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.surface_raised)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.menu = None;
                                pick(this, window, cx);
                                cx.notify();
                            }));
                    } else {
                        el = el.text_color(rgb(t.text_secondary));
                    }
                    list = list.child(el);
                }
            }
        }
        let list = div()
            .absolute()
            .top_full()
            .left_0()
            .child(deferred(anchored().snap_to_window().child(list)).with_priority(2))
            .into_any_element();
        vec![backdrop, list]
    }

    /// A top-row menu's face showing `text`, opening `menu`.
    fn menu_face(
        &mut self,
        menu: PanelMenu,
        text: &str,
        rows: Vec<MenuRow>,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (label, width) = match menu {
            PanelMenu::Task => (TASK, TASK_WIDTH),
            _ => (TEMPLATES, TEMPLATES_WIDTH),
        };
        self.picker(menu, label, width, text, rows, scope, window, cx)
    }

    /// The face of picker or menu `menu`, `width` wide, labelled `label`
    /// (also its accessibility label) and showing `text`; open, its rows
    /// under it.
    #[allow(clippy::too_many_arguments)]
    fn picker(
        &mut self,
        menu: PanelMenu,
        label: &str,
        width: f32,
        text: &str,
        rows: Vec<MenuRow>,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match menu {
            PanelMenu::Task => "modify-task-menu",
            PanelMenu::Templates => "modify-templates-menu",
            _ => "modify-picker-menu",
        };
        let me = cx.entity().downgrade();
        let face =
            crate::controls::menu_face::menu_face(label, text, None, true, scope, move |_, cx| {
                me.update(cx, |this, cx| this.toggle_menu(menu, cx)).ok();
            });
        div()
            .relative()
            .flex_none()
            .w(px(width))
            .child(face)
            .when(self.menu == Some(menu), |d| {
                d.children(self.popup(id, rows, scope, window, cx))
            })
            .into_any_element()
    }

    /// A picker's rows: one per item, the current one checked; a pick
    /// runs `pick` on the document.
    fn choice_rows<T: Copy + PartialEq + 'static>(
        items: &[T],
        current: Option<T>,
        title: impl Fn(T) -> String,
        pick: impl Fn(&mut DocumentModel, T) + 'static,
    ) -> Vec<MenuRow> {
        let pick = Rc::new(pick);
        items
            .iter()
            .map(|&item| {
                let pick = pick.clone();
                MenuRow::Item {
                    title: title(item),
                    enabled: true,
                    checked: current == Some(item),
                    pick: Rc::new(move |this, _, cx| {
                        this.with_doc(cx, |d| pick(d, item));
                    }),
                }
            })
            .collect()
    }

    /// The field for model key `key`, `width` wide, showing `placeholder`
    /// while empty and recorded as `textfield` `label`. Its text is the
    /// model's for the key; what is typed goes to `set_modify_field`; it
    /// owns the text owner `selectModify.‹key›` while focused; Return in
    /// it applies.
    #[allow(clippy::too_many_arguments)]
    fn text_field(
        &mut self,
        key: &str,
        placeholder: &str,
        label: &str,
        width: f32,
        help: Option<&'static str>,
        scope: &mut RegistryScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.drawn.insert(key.to_owned());
        let stale = self
            .fields
            .get(key)
            .is_none_or(|f| f.placeholder != placeholder);
        if stale {
            let field = self.new_field(key, placeholder, window, cx);
            self.fields.insert(key.to_owned(), field);
        }
        let input = self.fields[key].input.clone();
        let text = input.read(cx).text().to_owned();
        let focused = input.focus_handle(cx).is_focused(window);
        let el = div()
            .id(gpui::ElementId::Name(format!("modify-field:{key}").into()))
            .relative()
            .flex_none()
            .w(px(width))
            .child(input)
            .child(scope.control_with("textfield", label, true, Some(&text), focused));
        match help {
            Some(h) => crate::sheet_host::with_help(el, h).into_any_element(),
            None => el.into_any_element(),
        }
    }

    fn new_field(
        &mut self,
        key: &str,
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PanelField {
        let (text, invalid) = self
            .app
            .read(cx)
            .store
            .document(self.doc)
            .map(|d| (d.modify_field_text(key), d.modify_field_invalid(key)))
            .unwrap_or_default();
        let input = cx.new(|cx| {
            let mut f = TextInput::new(placeholder, cx).compact().with_text(&text);
            f.set_error(invalid, cx);
            f
        });
        let focus = input.focus_handle(cx);
        let (k1, k2, k3) = (key.to_owned(), key.to_owned(), key.to_owned());
        let subscriptions = vec![
            // Typed text goes to the model; a caret move, or the model's
            // own text set here, is not an edit.
            cx.observe(&input, move |this, field, cx| {
                let text = field.read(cx).text().to_owned();
                let doc = this.app.read(cx).store.document(this.doc);
                if doc.is_some_and(|d| d.modify_field_text(&k1) != text) {
                    this.with_doc(cx, |d| d.set_modify_field(&k1, &text));
                }
            }),
            cx.subscribe_in(&input, window, |this, _, e, window, cx| {
                if *e == TextInputEvent::Confirm {
                    this.apply(&Apply, window, cx);
                }
            }),
            cx.on_focus(&focus, window, move |this, _, cx| {
                this.with_doc(cx, |d| d.set_modify_text_editing(&k2, true));
            }),
            cx.on_blur(&focus, window, move |this, _, cx| {
                this.with_doc(cx, |d| d.set_modify_text_editing(&k3, false));
            }),
        ];
        PanelField {
            placeholder: placeholder.to_owned(),
            input,
            _subscriptions: subscriptions,
        }
    }

    /// Each field shows the model's text for its key (unless it already
    /// does) and is red while that text does not parse.
    fn sync_fields(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.app.read(cx).store.document(self.doc) else {
            return;
        };
        let wanted: Vec<(Entity<TextInput>, String, bool)> = self
            .fields
            .iter()
            .map(|(k, f)| {
                (
                    f.input.clone(),
                    d.modify_field_text(k),
                    d.modify_field_invalid(k),
                )
            })
            .collect();
        for (input, text, invalid) in wanted {
            input.update(cx, |f, cx| {
                if f.text() != text {
                    f.set_text(&text, cx);
                }
                f.set_error(invalid, cx);
            });
        }
    }

    /// Drops the fields this frame did not draw; one torn down while it
    /// had the keyboard gives it back.
    fn drop_undrawn_fields(&mut self, window: &Window, cx: &mut Context<Self>) {
        let drawn = std::mem::take(&mut self.drawn);
        let gone: Vec<String> = self
            .fields
            .keys()
            .filter(|k| !drawn.contains(*k))
            .cloned()
            .collect();
        if gone.is_empty() {
            return;
        }
        for k in &gone {
            self.fields.remove(k);
        }
        cx.defer_in(window, move |this, _, cx| {
            this.with_doc(cx, |d| {
                for k in &gone {
                    d.set_modify_text_editing(k, false);
                }
            });
        });
    }

    /// A half's on/off toggle: its box and `title`, a `checkbox` (selected
    /// when on); a click runs `flip`.
    fn checkbox(
        id: &'static str,
        title: &'static str,
        on: bool,
        scope: &mut RegistryScope,
        cx: &mut Context<Self>,
        flip: impl Fn(&mut DocumentModel) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .relative()
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .gap_2()
            .text_size(px(15.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .cursor_pointer()
            .child(if on { "\u{2611}" } else { "\u{2610}" })
            .child(title)
            .child(scope.control_selected("checkbox", title, true, None, on))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.menu = None;
                this.with_doc(cx, &flip);
            }))
            .into_any_element()
    }

    /// The remove button of a condition row.
    fn remove_button(i: usize, scope: &mut RegistryScope, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens();
        let b = div()
            .id(("modify-remove-condition", i))
            .relative()
            .flex_none()
            .px_1()
            .text_color(rgb(t.text_secondary))
            .cursor_pointer()
            .child("\u{2296}")
            .child(scope.control("button", REMOVE_CONDITION, true))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.menu = None;
                this.with_doc(cx, |d| d.remove_modify_condition(i));
            }));
        crate::sheet_host::with_help(b, REMOVE_CONDITION).into_any_element()
    }

    /// A segmented picker `label` (a `group`), one `button` per segment,
    /// the chosen one selected; a click runs `pick` with its index.
    fn segments(
        label: &'static str,
        width: f32,
        titles: &[&'static str],
        chosen: usize,
        scope: &mut RegistryScope,
        cx: &mut Context<Self>,
        pick: impl Fn(&mut DocumentModel, usize) + 'static,
    ) -> AnyElement {
        let t = tokens();
        let mut group = scope.container();
        let pick = Rc::new(pick);
        let mut row = div()
            .relative()
            .flex()
            .flex_row()
            .flex_none()
            .w(px(width))
            .h(px(22.))
            .rounded(px(t.radius))
            .overflow_hidden()
            .border_1()
            .border_color(rgb(t.surface_separator))
            .child(group.this("group", label, true));
        for (n, &title) in titles.iter().enumerate() {
            let on = n == chosen;
            let pick = pick.clone();
            row = row.child(
                div()
                    .id((label, n))
                    .relative()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.))
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            d.bg(rgb(t.accent)).text_color(rgb(t.on_accent))
                        } else {
                            d.bg(rgb(t.surface_raised)).text_color(rgb(t.text_primary))
                        }
                    })
                    .child(title)
                    .child(group.control_selected("button", title, true, None, on))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menu = None;
                        this.with_doc(cx, |d| pick(d, n));
                    })),
            );
        }
        row.into_any_element()
    }
}

/// A bottom-row button; `default` draws it as the default (Return) one.
fn button(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    default: bool,
    scope: &mut RegistryScope,
) -> gpui::Stateful<gpui::Div> {
    let t = tokens();
    div()
        .id(id)
        .relative()
        .flex_none()
        .h(px(22.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(t.radius))
        .border_1()
        .border_color(rgb(t.surface_separator))
        .whitespace_nowrap()
        .map(|d| {
            if default {
                d.bg(rgb(t.accent)).text_color(rgb(t.on_accent))
            } else {
                d.bg(rgb(t.surface_raised)).text_color(rgb(t.text_primary))
            }
        })
        .when(enabled, |d| d.cursor_pointer())
        .when(!enabled, |d| d.opacity(0.45))
        .child(label)
        .child(scope.control("button", label, enabled))
}

/// What the frame shows of the document, read once per frame.
struct Shown {
    task: PanelTask,
    config: ModifyConfig,
    /// The session's destinations, read only while a picker shows them.
    dests: Vec<DestRow>,
    templates: Vec<String>,
    notice: Option<String>,
    caption: &'static str,
    acts_on_dragged: bool,
    can_modify_again: bool,
    status: Option<String>,
}

impl Shown {
    fn of(d: &DocumentModel) -> Self {
        let config = d.select_modify().config.clone();
        let wants_dests = matches!(config.modify.op, ModifyRow::SetDest { .. })
            || config
                .select
                .conditions
                .iter()
                .any(|c| matches!(c.test, ConditionTest::Dest { .. }));
        let dests = if wants_dests {
            d.session().destinations().unwrap_or_default()
        } else {
            Vec::new()
        };
        Shown {
            task: d.select_modify().task,
            config,
            dests,
            templates: d
                .modify_templates()
                .iter()
                .map(|t| t.name.clone())
                .collect(),
            notice: d.modify_notice().map(str::to_owned),
            caption: d.apply_target_caption(),
            acts_on_dragged: d.modify_acts_on_dragged_events(),
            can_modify_again: d.can_modify_again(),
            status: d.status().map(str::to_owned),
        }
    }
}

impl Focusable for ModifyPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ModifyPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens();
        let mut scope = RegistryScope::root(&self.registry);
        let shown = self.app.read(cx).store.document(self.doc).map(Shown::of);
        let root = div()
            .id("modify-panel")
            .track_focus(&self.focus)
            .key_context(MODIFY_PANEL_CONTEXT)
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .on_action(cx.listener(Self::apply))
            .on_action(cx.listener(Self::run_command))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(crate::settings_window::background()))
            .text_color(rgb(t.text_primary))
            .text_size(px(13.));
        // Its document closing: the window goes on the next notify.
        let Some(shown) = shown else {
            return root;
        };

        // The top row: the Task picker and the Templates menu.
        let mut top_scope = scope.container();
        let task = self.menu_face(
            PanelMenu::Task,
            shown.task.title(),
            Self::task_items(shown.task),
            &mut top_scope,
            window,
            cx,
        );
        let templates = self.menu_face(
            PanelMenu::Templates,
            TEMPLATES,
            Self::template_items(&shown.templates),
            &mut top_scope,
            window,
            cx,
        );
        let top = div()
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .gap_2()
            .p_2()
            .child(task)
            .child(div().flex_1())
            .child(templates);

        // The body: the Select and Modify halves, each under its toggle.
        let mut body_scope = scope.container();
        let c = &shown.config;
        let select_on = c.select.enabled;
        let select_toggle = Self::checkbox(
            "modify-select-toggle",
            SELECT,
            select_on,
            &mut body_scope,
            cx,
            move |d| d.select_modify_mut().config.select.enabled = !select_on,
        );
        let select = select_on
            .then(|| self.select_half(&c.select, &shown.dests, &mut body_scope, window, cx));
        let modify_on = c.modify.enabled;
        let modify_toggle = Self::checkbox(
            "modify-modify-toggle",
            MODIFY,
            modify_on,
            &mut body_scope,
            cx,
            move |d| d.select_modify_mut().config.modify.enabled = !modify_on,
        );
        let modify = modify_on
            .then(|| self.modify_half(&c.modify, &shown.dests, &mut body_scope, window, cx));
        self.drop_undrawn_fields(window, cx);
        let divider = || div().flex_none().h(px(1.)).bg(rgb(t.surface_separator));
        let body = div()
            .id("modify-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .border_t_1()
            .border_color(rgb(t.surface_separator))
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .child(select_toggle)
            .children(select)
            .child(divider())
            .child(modify_toggle)
            .children(modify);

        // The notice line, when there is one.
        let notice = shown.notice.as_ref().map(|n| {
            div()
                .relative()
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .text_size(px(11.))
                .text_color(rgb(t.text_secondary))
                .child("\u{24D8}")
                .child(n.clone())
                .child(scope.control("text", n, true))
        });

        // The bottom row.
        let mut bottom_scope = scope.container();
        let name = self.template_name.read(cx).text().to_owned();
        let focused = self.template_name.focus_handle(cx).is_focused(window);
        let field = div()
            .relative()
            .flex_none()
            .w(px(TEMPLATE_NAME_WIDTH))
            .child(self.template_name.clone())
            .child(bottom_scope.control_with(
                "textfield",
                TEMPLATE_NAME,
                true,
                Some(&name),
                focused,
            ));
        let save = button(
            "modify-save-template",
            SAVE_AS_TEMPLATE,
            true,
            false,
            &mut bottom_scope,
        )
        .on_click(cx.listener(|this, _, _, cx| this.save_template(cx)));
        let caption_colour = if shown.acts_on_dragged {
            t.accent
        } else {
            t.text_secondary
        };
        let caption = div()
            .relative()
            .min_w_0()
            .truncate()
            .text_size(px(11.))
            .text_color(rgb(caption_colour))
            .child(shown.caption)
            .child(bottom_scope.control("text", shown.caption, true));
        let again = button(
            "modify-again",
            MODIFY_AGAIN,
            shown.can_modify_again,
            false,
            &mut bottom_scope,
        )
        .when(shown.can_modify_again, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.modify_again(cx)))
        });
        let again = crate::sheet_host::with_help(again, MODIFY_AGAIN_HELP);
        let apply = button("modify-apply", APPLY, true, true, &mut bottom_scope)
            .on_click(cx.listener(|this, _, window, cx| this.apply(&Apply, window, cx)));
        let bottom = div()
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .gap_2()
            .p_2()
            .border_t_1()
            .border_color(rgb(t.surface_separator))
            .child(field)
            .child(save)
            .child(div().flex_1())
            .child(caption)
            .child(again)
            .child(apply);

        // The document's status line.
        let status = shown.status.unwrap_or_default();
        let status_line = div()
            .relative()
            .flex_none()
            .h(px(20.))
            .px_2()
            .flex()
            .items_center()
            .text_size(px(11.))
            .text_color(rgb(t.text_secondary))
            .bg(rgb(t.surface_panel))
            .overflow_hidden()
            .whitespace_nowrap()
            .when(!status.is_empty(), |d| {
                d.child(status.clone())
                    .child(scope.control("text", &status, true))
            });

        root.child(top)
            .child(body)
            .children(notice)
            .child(bottom)
            .child(status_line)
    }
}

/// Opens `doc`'s Select and Modify panel, or brings its open one to the
/// front (never a second): floating, 620×460 (at least 580×320), centred
/// on the document window. A failure is the alert "Window Failed". Then
/// the panel is the key window and its document the front one. Call it
/// outside `AppState::update`.
pub fn open_modify_panel(doc: DocId, cx: &mut App) {
    let app = AppState::global(cx);
    let open = app.read(cx).modify_panel_of(doc).cloned();
    if let Some(w) = open {
        if w.handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            app.update(cx, |s, cx| {
                s.store.focus_document(doc, w.token);
                cx.notify();
            });
            return;
        }
        // The window is gone and its entry stale: forget it, open anew.
        app.update(cx, |s, _| s.remove_window(w.token));
    }
    let Some(title) = app.read(cx).store.document(doc).map(modify_panel_title) else {
        return;
    };
    let token = app.update(cx, |s, _| s.new_token());
    let open_size = size(px(WIDTH), px(HEIGHT));
    let owner = app.read(cx).window_of(doc).map(|w| w.handle);
    let centre = owner.and_then(|h| h.update(cx, |_, window, _| window.bounds().center()).ok());
    let bounds = match centre {
        Some(c) => Bounds::centered_at(c, open_size),
        None => Bounds::centered(None, open_size, cx),
    };
    let state = app.clone();
    let window_title = title.clone();
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
            titlebar: Some(TitlebarOptions {
                title: Some(window_title.into()),
                ..Default::default()
            }),
            kind: gpui::WindowKind::Floating,
            ..Default::default()
        },
        move |window, cx| cx.new(|cx| ModifyPanel::new(state, doc, token, title, window, cx)),
    );
    match opened {
        Ok(h) => {
            app.update(cx, |s, cx| {
                s.add_window(doc, token, h, WindowKind::ModifyPanel);
                s.store.focus_document(doc, token);
                cx.notify();
            });
            h.update(cx, |v, window, cx| {
                window.focus(&v.focus, cx);
                window.activate_window();
            })
            .ok();
        }
        Err(e) => {
            app.update(cx, |s, cx| {
                s.store
                    .post_alert(AlertItem::new("Window Failed", e.to_string()));
                cx.notify();
            });
            crate::alerts::present(cx);
        }
    }
}
