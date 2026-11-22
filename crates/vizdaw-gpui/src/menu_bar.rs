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

const LIST: u32 = 0x2e2e38;
const BORDER: u32 = 0x4a4a58;

pub struct MenuBar {
    model: MenuModel,
    /// The index of the menu whose list is open.
    open: Option<usize>,
}

impl MenuBar {
    pub fn new(model: MenuModel) -> Self {
        MenuBar { model, open: None }
    }

    pub fn model(&self) -> &MenuModel {
        &self.model
    }

    /// Draws `model` from now on; an open menu that is gone closes.
    pub fn set_model(&mut self, model: MenuModel) {
        if self.open.is_some_and(|i| i >= model.menus.len()) {
            self.open = None;
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
    }

    /// Closes the open list; false when none was open.
    pub fn close(&mut self) -> bool {
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
            .bg(rgb(colors::BAR))
            .border_b_1()
            .border_color(rgb(BORDER));
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
            .when(open, |d| d.bg(rgb(colors::BUTTON)))
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
                            spec,
                            &mut scope.container(),
                            cx,
                        )))
                        .with_priority(2),
                    ),
                )
            })
            .into_any_element()
    }

    fn list(
        spec: &MenuSpec,
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
            .bg(rgb(LIST))
            .border_1()
            .border_color(rgb(BORDER))
            .rounded_md()
            .shadow_md();
        for (row, item) in spec.items.iter().enumerate() {
            list = list.child(match item {
                ItemSpec::Separator => div().h(px(1.)).my_1().bg(rgb(BORDER)).into_any_element(),
                ItemSpec::Command {
                    id,
                    title,
                    keys,
                    enabled,
                    checked,
                } => {
                    let (id, enabled, checked) = (*id, *enabled, *checked);
                    let mark = match checked {
                        Some(true) => "\u{2713} ",
                        Some(false) => "    ",
                        None => "",
                    };
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
                            colors::TEXT
                        } else {
                            colors::DISABLED
                        }))
                        .child(format!("{mark}{title}"))
                        .child(
                            div()
                                .text_color(rgb(if enabled {
                                    colors::DIM
                                } else {
                                    colors::DISABLED
                                }))
                                .child(keys.clone().unwrap_or_default()),
                        )
                        .child(scope.control_with(
                            "menuitem",
                            title,
                            enabled,
                            checked.map(|on| if on { "on" } else { "off" }),
                            false,
                        ))
                        .when(enabled, |d| {
                            d.cursor_pointer()
                                .hover(|s| s.bg(rgb(colors::BUTTON)))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    if let Some(bar) = this.menu_bar_mut() {
                                        bar.close();
                                    }
                                    window.dispatch_action(menus::action_for(id), cx);
                                    cx.notify();
                                }))
                        })
                        .into_any_element()
                }
            });
        }
        list.into_any_element()
    }
}
