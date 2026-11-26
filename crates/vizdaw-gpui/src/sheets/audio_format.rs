//! The audio format rows the Bounce and Export for Another DAW sheets
//! share (Swift `AudioFormatRows`): `Format` WAV/AIFF/CAF, `Bits`
//! 16-bit/24-bit/32-bit float, `Rate` six rates and `Other…` (which opens
//! the `Rate (Hz)` field, clamped to 8,000–384,000) and `Dither` (off at
//! 32-bit float). The choices live on the document
//! ([`vizdaw_app::audio_format::AudioFormatChoices`]); each sheet hands
//! the rows how to edit them. Also the small pieces those sheets draw
//! alike: a text line, a labelled row, a toggle, a progress bar.

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement, Styled, Window, anchored, deferred, div, px,
    relative, rgb,
};
use vizdaw_app::audio_format::{
    AudioFormat, BITS, CONTAINERS, DEPTHS, DITHER, FORMAT, OTHER, OTHER_LABEL, OTHER_PLACEHOLDER,
    RATE, RATES, rate_title,
};
use vizdaw_ffi::BitDepthRow;

use crate::registry::RegistryScope;
use crate::text_input::TextInput;
use crate::transport_bar::colors;

/// The width of a row's label column.
const LABEL_WIDTH: f32 = 90.;

/// How the rows edit the sheet's format choices (on the document).
pub(crate) type EditFormat = Rc<dyn Fn(&mut Window, &mut App, &dyn Fn(&mut AudioFormat))>;

/// Which picker is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    Format,
    Bits,
    Rate,
}

/// The rows' own state: the open picker and the `Rate (Hz)` field.
pub(crate) struct FormatRows {
    open: Entity<Option<Pick>>,
    other: Entity<TextInput>,
}

/// A text line recorded in the tree.
pub(crate) fn text(t: &str, scope: &mut RegistryScope) -> gpui::Div {
    div()
        .relative()
        .child(t.to_owned())
        .child(scope.control("text", t, true))
}

/// A labelled row: the label column, then `control`.
pub(crate) fn row(label: &str, control: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(LABEL_WIDTH))
                .flex_none()
                .text_color(rgb(colors::text()))
                .child(label.to_owned()),
        )
        .child(control)
}

/// A toggle: its box and `title`, recorded as a `checkbox` labelled
/// `label` (selected when on); a click flips it through `flip`.
pub(crate) fn toggle(
    id: (&'static str, usize),
    title: &str,
    label: &str,
    on: bool,
    enabled: bool,
    scope: &mut RegistryScope,
    flip: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .text_color(rgb(if enabled {
            colors::text()
        } else {
            colors::disabled()
        }))
        .child(if on { "\u{2611}" } else { "\u{2610}" })
        .child(title.to_owned())
        .child(scope.control_selected("checkbox", label, enabled, None, on))
        .when(enabled, |d| {
            d.cursor_pointer().on_click(move |_, window, cx| {
                flip(window, cx);
                cx.stop_propagation();
            })
        })
        .into_any_element()
}

/// A progress bar (0–1) labelled `label`, its value the progress.
pub(crate) fn progress_bar(p: f32, label: &str, scope: &mut RegistryScope) -> AnyElement {
    let p = p.clamp(0., 1.);
    let value = format!("{p:.2}");
    div()
        .relative()
        .w_full()
        .h(px(6.))
        .rounded_sm()
        .bg(rgb(colors::readout()))
        .child(
            div()
                .h_full()
                .w(relative(p))
                .rounded_sm()
                .bg(rgb(colors::on())),
        )
        .child(scope.control_with("progress", label, true, Some(&value), false))
        .into_any_element()
}

/// Picker `which`'s items and the chosen one's title.
fn items(which: Pick, f: &AudioFormat) -> (Vec<String>, String) {
    match which {
        Pick::Format => (
            CONTAINERS.iter().map(|(_, t)| (*t).to_owned()).collect(),
            CONTAINERS
                .iter()
                .find(|(k, _)| *k == f.container)
                .map_or("", |(_, t)| t)
                .to_owned(),
        ),
        Pick::Bits => (
            DEPTHS.iter().map(|(_, t)| (*t).to_owned()).collect(),
            DEPTHS
                .iter()
                .find(|(k, _)| *k == f.depth)
                .map_or("", |(_, t)| t)
                .to_owned(),
        ),
        Pick::Rate => {
            let mut all: Vec<String> = RATES.iter().map(|r| rate_title(*r)).collect();
            all.push(OTHER.to_owned());
            let chosen = if f.other_rate {
                OTHER.to_owned()
            } else {
                rate_title(f.rate)
            };
            (all, chosen)
        }
    }
}

/// Item `i` of picker `which`, chosen.
fn choose(which: Pick, i: usize, f: &mut AudioFormat) {
    match which {
        Pick::Format => f.container = CONTAINERS[i].0,
        Pick::Bits => f.depth = DEPTHS[i].0,
        Pick::Rate => f.set_rate_choice(RATES.get(i).copied().unwrap_or(0)),
    }
}

impl FormatRows {
    /// The rows, the field showing `rate`.
    pub(crate) fn new(rate: u32, cx: &mut App) -> Self {
        let text = rate.to_string();
        FormatRows {
            open: cx.new(|_| None),
            other: cx.new(|cx| {
                TextInput::new(OTHER_PLACEHOLDER, cx)
                    .compact()
                    .with_text(&text)
            }),
        }
    }

    /// Closes an open picker.
    pub(crate) fn close(&self, cx: &mut App) {
        self.open.update(cx, |o, _| *o = None);
    }

    /// The field takes the focus while "Other…" is chosen.
    pub(crate) fn focus_order(&self, f: &AudioFormat, cx: &App) -> Vec<FocusHandle> {
        if f.other_rate {
            vec![self.other.focus_handle(cx)]
        } else {
            vec![]
        }
    }

    /// The `Rate (Hz)` field follows the choice: what is typed sets the
    /// rate (clamped); left, it shows the rate.
    fn sync_other(&self, f: &AudioFormat, edit: &EditFormat, window: &mut Window, cx: &mut App) {
        let focused = self.other.focus_handle(cx).is_focused(window);
        let typed = self.other.read(cx).text().trim().to_owned();
        if focused {
            if let Ok(n) = typed.parse::<i64>() {
                let mut next = *f;
                next.set_other_rate(n);
                if next.rate != f.rate {
                    edit(window, cx, &|f| f.set_other_rate(n));
                }
            }
        } else if typed != f.rate.to_string() {
            let rate = f.rate.to_string();
            self.other.update(cx, |t, cx| t.set_text(&rate, cx));
        }
    }

    /// Picker `which`, labelled by its row: its face (`menu`, its value the
    /// choice), and its items (`menuitem`) while open.
    fn picker(
        &self,
        which: Pick,
        f: &AudioFormat,
        enabled: bool,
        edit: &EditFormat,
        scope: &mut RegistryScope,
        cx: &App,
    ) -> AnyElement {
        let label = match which {
            Pick::Format => FORMAT,
            Pick::Bits => BITS,
            Pick::Rate => RATE,
        };
        let (all, chosen) = items(which, f);
        let open = enabled && *self.open.read(cx) == Some(which);
        let toggle_open = self.open.clone();
        let face = div()
            .id(("format-picker", which as usize))
            .relative()
            .h(px(24.))
            .min_w(px(120.))
            .px_2()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(colors::border()))
            .bg(rgb(colors::button()))
            .text_color(rgb(if enabled {
                colors::text()
            } else {
                colors::disabled()
            }))
            .child(chosen.clone())
            .child(div().text_color(rgb(colors::dim())).child("\u{25BE}"))
            .child(scope.control_with("menu", label, enabled, Some(&chosen), false))
            .when(enabled, |d| {
                d.cursor_pointer().on_click(move |_, window, cx| {
                    toggle_open.update(cx, |o, _| {
                        *o = (*o != Some(which)).then_some(which);
                    });
                    window.refresh();
                })
            });
        face.when(open, |d| {
            let rows = all.iter().enumerate().map(|(i, title)| {
                let open = self.open.clone();
                let edit = edit.clone();
                div()
                    .id(("format-item", i))
                    .relative()
                    .px_3()
                    .py_1()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors::button())))
                    .child(title.clone())
                    .child(scope.control_selected("menuitem", title, true, None, *title == chosen))
                    .on_click(move |_, window, cx| {
                        open.update(cx, |o, _| *o = None);
                        edit(window, cx, &|f| choose(which, i, f));
                        cx.stop_propagation();
                    })
            });
            d.child(
                div().absolute().top_full().left_0().child(
                    deferred(
                        anchored().snap_to_window().child(
                            div()
                                .id("format-list")
                                .occlude()
                                .flex()
                                .flex_col()
                                .min_w(px(120.))
                                .py_1()
                                .bg(rgb(colors::list()))
                                .border_1()
                                .border_color(rgb(colors::border()))
                                .rounded_md()
                                .children(rows),
                        ),
                    )
                    .with_priority(3),
                ),
            )
        })
        .into_any_element()
    }

    /// The rows for `f` (off while `!enabled`): Format, Bits, Rate, the
    /// `Rate (Hz)` field for "Other…", Dither.
    pub(crate) fn render(
        &self,
        f: &AudioFormat,
        enabled: bool,
        edit: &EditFormat,
        scope: &mut RegistryScope,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<AnyElement> {
        if !enabled {
            self.close(cx);
        }
        if f.other_rate && enabled {
            self.sync_other(f, edit, window, cx);
        }
        let mut rows = vec![
            row(
                FORMAT,
                self.picker(Pick::Format, f, enabled, edit, scope, cx),
            )
            .into_any_element(),
            row(BITS, self.picker(Pick::Bits, f, enabled, edit, scope, cx)).into_any_element(),
            row(RATE, self.picker(Pick::Rate, f, enabled, edit, scope, cx)).into_any_element(),
        ];
        if f.other_rate {
            let field = if enabled {
                let typed = self.other.read(cx).text().to_owned();
                let focused = self.other.focus_handle(cx).is_focused(window);
                div()
                    .relative()
                    .w(px(120.))
                    .child(self.other.clone())
                    .child(scope.control_with(
                        "textfield",
                        OTHER_LABEL,
                        true,
                        Some(&typed),
                        focused,
                    ))
            } else {
                // Off: the rate as sent, not a field to type in.
                let rate = f.rate.to_string();
                div()
                    .relative()
                    .w(px(120.))
                    .text_color(rgb(colors::disabled()))
                    .child(rate.clone())
                    .child(scope.control_with("textfield", OTHER_LABEL, false, Some(&rate), false))
            };
            rows.push(row("", field).into_any_element());
        }
        let edit = edit.clone();
        rows.push(toggle(
            ("format-dither", 0),
            DITHER,
            DITHER,
            f.dither,
            enabled && f.depth != BitDepthRow::Float32,
            scope,
            move |window, cx| edit(window, cx, &|f| f.dither = !f.dither),
        ));
        rows
    }
}
