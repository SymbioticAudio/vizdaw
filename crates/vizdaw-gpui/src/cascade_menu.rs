//! A menu with nested submenus (Swift's `NSMenu` with submenu items, as
//! the Strip Chart's row menu and the Modify panel's target pickers use):
//! a backdrop that closes it, and its list under its face (or at the
//! pointer). A submenu row shows a ▸ and opens its list beside it when
//! hovered or clicked; submenus nest to any depth. The rows are
//! `menuitem` in the tree. It looks like the mixer's menus
//! (`mixer_view.rs`'s `menu_rows`).

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Point,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, point, px, rgb,
};

use crate::registry::RegistryScope;
use crate::transport_bar::colors;

/// What a pick does.
pub(crate) type Pick = Rc<dyn Fn(&mut Window, &mut App)>;
/// What opening a submenu does, given its path.
pub(crate) type OnOpen = Rc<dyn Fn(Vec<usize>, &mut Window, &mut App)>;

/// The backdrop's side: larger than any window (`cascade` has no window
/// to measure).
const BACKDROP: f32 = 16_384.;

/// One line of a cascading menu.
pub(crate) enum CascadeEntry {
    Item {
        title: String,
        checked: bool,
        enabled: bool,
        pick: Pick,
    },
    Separator,
    Submenu {
        title: String,
        enabled: bool,
        /// Why a disabled submenu is (its help).
        help: Option<String>,
        items: Vec<CascadeEntry>,
    },
}

impl CascadeEntry {
    /// An enabled, unchecked item.
    pub(crate) fn item(
        title: impl Into<String>,
        pick: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        CascadeEntry::Item {
            title: title.into(),
            checked: false,
            enabled: true,
            pick: Rc::new(pick),
        }
    }

    /// An enabled submenu.
    pub(crate) fn submenu(title: impl Into<String>, items: Vec<CascadeEntry>) -> Self {
        CascadeEntry::Submenu {
            title: title.into(),
            enabled: true,
            help: None,
            items,
        }
    }
}

/// The callbacks every row shares.
struct Handlers {
    on_open: OnOpen,
    on_close: Pick,
}

/// The menu `entries` with the submenus along `open` (each depth's open
/// row) shown: a backdrop that closes it (`on_close`), then its list at
/// `at` (window) or under its face. Hovering or clicking a submenu row
/// opens it (`on_open` with its path); picking an item calls its `pick`,
/// then `on_close`.
pub(crate) fn cascade(
    id: &'static str,
    entries: Vec<CascadeEntry>,
    open: &[usize],
    at: Option<Point<Pixels>>,
    on_open: OnOpen,
    on_close: Pick,
    scope: &mut RegistryScope,
) -> Vec<AnyElement> {
    let closing = on_close.clone();
    let backdrop = deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("cascade-menu-backdrop")
                .w(px(BACKDROP))
                .h(px(BACKDROP))
                .occlude()
                .on_any_mouse_down(move |_, window, cx| closing(window, cx)),
        ),
    )
    .with_priority(1)
    .into_any_element();
    let handlers = Handlers { on_open, on_close };
    let list = rows(id, entries, open, &[], &handlers, scope);
    let list = match at {
        Some(at) => deferred(anchored().position(at).snap_to_window().child(list))
            .with_priority(2)
            .into_any_element(),
        None => div()
            .absolute()
            .top_full()
            .left_0()
            .child(deferred(anchored().snap_to_window().child(list)).with_priority(2))
            .into_any_element(),
    };
    vec![backdrop, list]
}

/// One list: `entries`, the submenu `open[0]` open (and its own along the
/// rest). `prefix` is this list's path.
fn rows(
    id: &'static str,
    entries: Vec<CascadeEntry>,
    open: &[usize],
    prefix: &[usize],
    handlers: &Handlers,
    scope: &mut RegistryScope,
) -> gpui::Stateful<gpui::Div> {
    let mut list = crate::arrangement::menu_list(id);
    let row_div = |row: usize, enabled: bool| {
        div()
            .id(("cascade-menu-item", row))
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .px_3()
            .py_1()
            .text_xs()
            .when(enabled, |d| {
                d.cursor_pointer().hover(|s| s.bg(rgb(colors::button())))
            })
            .when(!enabled, |d| d.text_color(rgb(colors::disabled())))
    };
    for (row, entry) in entries.into_iter().enumerate() {
        match entry {
            CascadeEntry::Separator => list = list.child(crate::arrangement::menu_separator()),
            CascadeEntry::Item {
                title,
                checked,
                enabled,
                pick,
            } => {
                let mut el = row_div(row, enabled)
                    .child(if checked { "\u{2713}" } else { "" }.to_owned())
                    .child(title.clone())
                    .child(scope.control_selected("menuitem", &title, enabled, None, checked));
                if enabled {
                    let close = handlers.on_close.clone();
                    el = el.on_click(move |_, window, cx| {
                        pick(window, cx);
                        close(window, cx);
                    });
                }
                list = list.child(el);
            }
            CascadeEntry::Submenu {
                title,
                enabled,
                help,
                items,
            } => {
                let path: Vec<usize> = prefix.iter().copied().chain([row]).collect();
                let is_open = enabled && open.first() == Some(&row);
                let value = if enabled { None } else { help.as_deref() };
                let mut el = row_div(row, enabled)
                    .child(title.clone())
                    .child(div().flex_1())
                    .child(div().text_color(rgb(colors::dim())).child("\u{25B8}"))
                    .child(scope.control_with("menuitem", &title, enabled, value, false));
                if enabled {
                    let (clicked, hovered) = (handlers.on_open.clone(), handlers.on_open.clone());
                    let (a, b) = (path.clone(), path.clone());
                    el = el
                        // The press is the row's, not the backdrop's.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(move |_, window, cx| clicked(a.clone(), window, cx))
                        .on_hover(move |over, window, cx| {
                            if *over {
                                hovered(b.clone(), window, cx);
                            }
                        });
                } else if let Some(help) = help {
                    el = crate::sheet_host::with_help(el, help);
                }
                if is_open {
                    let nested = rows(id, items, &open[1..], &path, handlers, scope);
                    el = el.child(div().absolute().top_0().left_full().child(
                        deferred(anchored().snap_to_window().child(nested)).with_priority(3),
                    ));
                }
                list = list.child(el);
            }
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui::{
        Context, IntoElement, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement,
        Render, Styled, TestAppContext, Window, div, px,
    };

    use super::{CascadeEntry, cascade};
    use crate::registry::{RegistryScope, SharedRegistry};

    /// A window showing one cascading menu open along `open`.
    struct Host {
        registry: SharedRegistry,
        open: Vec<usize>,
        log: Rc<RefCell<Vec<String>>>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let mut scope = RegistryScope::root(&self.registry);
            let (picked, opened, closed) = (self.log.clone(), self.log.clone(), self.log.clone());
            let leaf = CascadeEntry::item("Leaf", move |_, _| {
                picked.borrow_mut().push("pick".into());
            });
            let entries = vec![
                CascadeEntry::item("Top", |_, _| {}),
                CascadeEntry::Separator,
                CascadeEntry::submenu(
                    "One",
                    vec![CascadeEntry::submenu(
                        "Two",
                        vec![CascadeEntry::submenu("Three", vec![leaf])],
                    )],
                ),
            ];
            let menu = cascade(
                "test-cascade",
                entries,
                &self.open,
                None,
                Rc::new(move |path, _, _| opened.borrow_mut().push(format!("open {path:?}"))),
                Rc::new(move |_, _| closed.borrow_mut().push("close".into())),
                &mut scope,
            );
            div()
                .size_full()
                .child(div().relative().w(px(80.)).h(px(20.)).children(menu))
        }
    }

    #[gpui::test]
    fn a_nested_submenu_opens_along_its_path(cx: &mut TestAppContext) {
        let registry = SharedRegistry::default();
        let log = Rc::new(RefCell::new(Vec::new()));
        let (shared, logged) = (registry.clone(), log.clone());
        let (_, cx) = cx.add_window_view(move |_, _| Host {
            registry: shared,
            open: vec![2, 0, 0],
            log: logged,
        });
        cx.run_until_parked();
        let items: Vec<_> = registry
            .borrow()
            .elements()
            .iter()
            .filter(|e| e.role == "menuitem")
            .cloned()
            .collect();
        let labels: Vec<&str> = items.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Top", "One", "Two", "Three", "Leaf"]);
        let at = items[4].bounds.center();
        cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
        });
        cx.run_until_parked();
        let log = log.borrow();
        let picked: Vec<&str> = log
            .iter()
            .map(String::as_str)
            .filter(|l| !l.starts_with("open"))
            .collect();
        assert_eq!(picked, ["pick", "close"]);
    }

    #[gpui::test]
    fn a_closed_submenu_shows_only_its_row(cx: &mut TestAppContext) {
        let registry = SharedRegistry::default();
        let shared = registry.clone();
        let (_, cx) = cx.add_window_view(move |_, _| Host {
            registry: shared,
            open: vec![],
            log: Rc::default(),
        });
        cx.run_until_parked();
        let labels: Vec<String> = registry
            .borrow()
            .elements()
            .iter()
            .filter(|e| e.role == "menuitem")
            .map(|e| e.label.clone())
            .collect();
        assert_eq!(labels, ["Top", "One"]);
    }
}
