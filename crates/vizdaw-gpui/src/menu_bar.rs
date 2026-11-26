//! The in-window menu bar, for platforms where GPUI shows no menus (the
//! document window draws it off macOS). It draws a [`MenuModel`]: the
//! top-level titles in a row; clicking one opens its items in an overlay;
//! clicking an enabled item runs it and closes the list. Escape (the
//! window passes it on, see [`MenuBar::close`]) or a click outside the
//! list closes it. Titles record as `menu`, items as `menuitem`.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, point, px, rgb,
};

use crate::menus::{self, ItemSpec, MenuModel, MenuSpec};
use crate::registry::RegistryScope;
use crate::shell::DocumentWindow;
use crate::transport_bar::colors;

pub struct MenuBar {
    model: MenuModel,
    /// The index of the menu whose list is open.
    open: Option<usize>,
    /// The row of the open list whose submenu is open.
    sub: Option<usize>,
}

impl MenuBar {
    pub fn new(model: MenuModel) -> Self {
        MenuBar {
            model,
            open: None,
            sub: None,
        }
    }

    pub fn model(&self) -> &MenuModel {
        &self.model
    }

    /// Draws `model` from now on; an open menu that is gone closes.
    pub fn set_model(&mut self, model: MenuModel) {
        if self.open.is_some_and(|i| i >= model.menus.len()) {
            self.open = None;
            self.sub = None;
        }
        self.model = model;
    }

    /// The open menu's title.
    pub fn open_menu(&self) -> Option<&str> {
        self.open
            .and_then(|i| self.model.menus.get(i))
            .map(|m| m.title.as_str())
    }

    /// Opens menu `index`, or closes the open one with `None`.
    pub fn set_open(&mut self, index: Option<usize>) {
        self.open = index.filter(|i| *i < self.model.menus.len());
        self.sub = None;
    }

    /// Closes the open list; false when none was open.
    pub fn close(&mut self) -> bool {
        self.sub = None;
        self.open.take().is_some()
    }

    /// The bar, drawn into the document window `cx`, whose
    /// [`DocumentWindow::menu_bar_mut`] is this bar.
    pub fn render(
        &self,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<DocumentWindow>,
    ) -> AnyElement {
        let mut bar = div()
            .flex()
            .flex_row()
            .items_center()
            .px_1()
            .bg(rgb(colors::bar()))
            .border_b_1()
            .border_color(rgb(colors::border()));
        for (index, spec) in self.model.menus.iter().enumerate() {
            bar = bar.child(self.title(index, spec, scope, cx));
        }
        if self.open.is_some() {
            // Behind the list and over everything else: a click there
            // closes the list and does nothing more.
            let size = window.viewport_size();
            bar = bar.child(
                deferred(
                    anchored().position(point(px(0.), px(0.))).child(
                        div()
                            .id("menu-backdrop")
                            .w(size.width)
                            .h(size.height)
                            .occlude()
                            .on_any_mouse_down(cx.listener(|this, _, _, cx| {
                                if let Some(bar) = this.menu_bar_mut() {
                                    bar.close();
                                }
                                cx.notify();
                            })),
                    ),
                )
                .with_priority(1),
            );
        }
        bar.into_any_element()
    }

    fn title(
        &self,
        index: usize,
        spec: &MenuSpec,
        scope: &mut RegistryScope,
        cx: &mut Context<DocumentWindow>,
    ) -> AnyElement {
        let open = self.open == Some(index);
        div()
            .id(("menu", index))
            .relative()
            .px_2()
            .py_1()
            .rounded_sm()
            .cursor_pointer()
            .when(open, |d| d.bg(rgb(colors::button())))
            .child(spec.title.clone())
            .child(scope.control("menu", &spec.title, true))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(bar) = this.menu_bar_mut() {
                    let next = (bar.open != Some(index)).then_some(index);
                    bar.set_open(next);
                }
                cx.notify();
            }))
            .when(open, |d| {
                d.child(
                    div().absolute().top_full().left_0().child(
                        deferred(anchored().snap_to_window().child(Self::list(
                            &spec.items,
                            Some(self.sub),
                            &mut scope.container(),
                            cx,
                        )))
                        .with_priority(2),
                    ),
                )
            })
            .into_any_element()
    }

    /// A list of `items`; `sub` is the row whose submenu is open (`None`:
    /// a submenu's own list, whose rows open nothing further).
    fn list(
        items: &[ItemSpec],
        sub: Option<Option<usize>>,
        scope: &mut RegistryScope,
        cx: &mut Context<DocumentWindow>,
    ) -> AnyElement {
        let mut list = div()
            .id("menu-list")
            .occlude()
            .flex()
            .flex_col()
            .min_w(px(220.))
            .py_1()
            .bg(rgb(colors::list()))
            .border_1()
            .border_color(rgb(colors::border()))
            .rounded_md()
            .shadow_md();
        for (row, item) in items.iter().enumerate() {
            list = list.child(match item {
                ItemSpec::Separator => div()
                    .h(px(1.))
                    .my_1()
                    .bg(rgb(colors::border()))
                    .into_any_element(),
                ItemSpec::Heading(title) => {
                    Self::row(row, title, None, false, None, scope).into_any_element()
                }
                ItemSpec::Command {
                    id,
                    title,
                    keys,
                    enabled,
                    checked,
                } => {
                    let id = *id;
                    Self::row(row, title, keys.clone(), *enabled, *checked, scope)
                        .when(*enabled, |d| {
                            d.on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(bar) = this.menu_bar_mut() {
                                    bar.close();
                                }
                                window.dispatch_action(menus::action_for(id), cx);
                                cx.notify();
                            }))
                        })
                        .into_any_element()
                }
                ItemSpec::Mixer { op, title, enabled } => {
                    let op = *op;
                    Self::row(row, title, None, *enabled, None, scope)
                        .when(*enabled, |d| {
                            d.on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(bar) = this.menu_bar_mut() {
                                    bar.close();
                                }
                                window.dispatch_action(Box::new(menus::MixerMenuItem { op }), cx);
                                cx.notify();
                            }))
                        })
                        .into_any_element()
                }
                ItemSpec::Submenu {
                    title,
                    enabled,
                    items,
                } => {
                    let open = *enabled && sub == Some(Some(row));
                    let opens = *enabled && sub.is_some();
                    Self::row(
                        row,
                        &format!("{title} \u{25B8}"),
                        None,
                        *enabled,
                        None,
                        scope,
                    )
                    .when(opens, |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(bar) = this.menu_bar_mut() {
                                bar.sub = (bar.sub != Some(row)).then_some(row);
                            }
                            cx.notify();
                        }))
                    })
                    .when(open, |d| {
                        d.child(div().absolute().top_0().left_full().child(Self::list(
                            items,
                            None,
                            &mut scope.container(),
                            cx,
                        )))
                    })
                    .into_any_element()
                }
            });
        }
        list.into_any_element()
    }

    /// One row: its title (a check mark for a toggle), its keys, recorded
    /// as a `menuitem` (a toggle's value `on` / `off`). The record keeps
    /// the plain title.
    fn row(
        row: usize,
        title: &str,
        keys: Option<String>,
        enabled: bool,
        checked: Option<bool>,
        scope: &mut RegistryScope,
    ) -> gpui::Stateful<gpui::Div> {
        let mark = match checked {
            Some(true) => "\u{2713} ",
            Some(false) => "    ",
            None => "",
        };
        let label = title.strip_suffix(" \u{25B8}").unwrap_or(title);
        div()
            .id(("menuitem", row))
            .relative()
            .flex()
            .flex_row()
            .justify_between()
            .gap_4()
            .px_3()
            .py_1()
            .text_color(rgb(if enabled {
                colors::text()
            } else {
                colors::disabled()
            }))
            .child(format!("{mark}{title}"))
            .child(
                div()
                    .text_color(rgb(if enabled {
                        colors::dim()
                    } else {
                        colors::disabled()
                    }))
                    .child(keys.unwrap_or_default()),
            )
            .child(scope.control_with(
                "menuitem",
                label,
                enabled,
                checked.map(|on| if on { "on" } else { "off" }),
                false,
            ))
            .when(enabled, |d| {
                d.cursor_pointer().hover(|s| s.bg(rgb(colors::button())))
            })
    }
}
