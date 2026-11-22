//! The track header column beside the arrangement: one row per track of
//! the shown sequence (kind icon, name, M, S, R) and the "+ Add Track" row
//! under the last. The column records as a `group` "Track Headers"; each
//! row is one `button` whose label names its kind, name and routing (the
//! M/S/R toggles are not separate elements, as in the Swift app), and
//! "Add Track" is the last child.
//!
//! A click on a row's icon or name (or its empty space) selects the track
//! (Shift or the secondary modifier adds or removes it); M, S and R set
//! the flags and leave the selection. A control-channel click on a row
//! toggles mute. A double-click on the name opens the rename field, which
//! the document window owns (see [`DocumentWindow::begin_rename`]).

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Subscription, Window, anchored, deferred, div, point, px,
    rgb, rgba,
};
use vizdaw_ffi::snapshot::LaneRow;
use vizdaw_ffi::tracks::TrackKindRow;

use crate::control;
use crate::menus::RunCommand;
use crate::registry::RegistryScope;
use crate::shell::DocumentWindow;
use crate::text_input::TextInput;
use crate::transport_bar::colors;

/// The column's width.
pub const WIDTH: f32 = 200.;
/// A row's height (Swift `ArrangementLayout.laneHeight`).
pub const ROW_HEIGHT: f32 = 48.;

const COLUMN: u32 = 0x22222a;
const SEPARATOR: u32 = 0x3a3a46;
/// The selection fill at 0.22 alpha (one fixed colour until themes come).
const SELECTED: u32 = 0x4a7dff38;
const LIST: u32 = 0x2e2e38;
const BORDER: u32 = 0x4a4a58;
const MUTE_ON: u32 = 0xc89b2e;
const SOLO_ON: u32 = 0x3f8f4f;
const ARM_ON: u32 = 0xb03a3a;

/// Swift `TrackCommands.addTrackOnChannel`: the status a control-channel
/// click on "+ Add Track" sets.
pub const ADD_TRACK_ON_CHANNEL: &str = "Add Track opens a menu: use track.new_audio, \
     track.new_instrument or track.new_midi over the control channel.";

/// The Add Track menu: title and registry command.
pub const ADD_TRACK_ITEMS: [(&str, &str); 3] = [
    ("New Audio Track", "track.new_audio"),
    ("New Instrument Track", "track.new_instrument"),
    ("New MIDI Track", "track.new_midi"),
];

/// The header column's state, owned by the document window.
#[derive(Default)]
pub struct TrackHeaders {
    pub(crate) rename: Option<Rename>,
    pub(crate) add_menu_open: bool,
}

/// An open rename field.
pub(crate) struct Rename {
    pub lane: u64,
    pub field: Entity<TextInput>,
    pub _subscriptions: Vec<Subscription>,
}

impl TrackHeaders {
    /// The track whose name is being edited.
    pub fn renaming(&self) -> Option<u64> {
        self.rename.as_ref().map(|r| r.lane)
    }

    /// True while the Add Track menu is open.
    pub fn add_menu_open(&self) -> bool {
        self.add_menu_open
    }
}

/// The start of a header's label (Swift `ArrangementA11y.kindName`).
pub fn kind_name(kind: TrackKindRow) -> &'static str {
    match kind {
        TrackKindRow::Audio => "Audio track",
        TrackKindRow::Instrument => "Instrument track",
        TrackKindRow::Midi => "MIDI track",
    }
}

/// The name a header shows (Swift `LaneHeaderLayout.nameText`):
/// "<name> · <playing take>" with more than one take.
pub fn name_text(lane: &LaneRow) -> String {
    if lane.takes.len() > 1
        && let Some(take) = lane.takes.iter().find(|t| t.id == lane.active_take_id)
    {
        return format!("{} \u{b7} {}", lane.name, take.name);
    }
    lane.name.clone()
}

/// The header's words after its kind (Swift `ArrangementA11y.header`; the
/// input and monitor words come with the input slice).
pub fn header(lane: &LaneRow) -> String {
    // An audio track plays to an output path, not a MIDI destination.
    let routing = if lane.kind == TrackKindRow::Audio {
        format!(
            "output {}",
            lane.audio_out_name.as_deref().unwrap_or("none")
        )
    } else {
        lane.destination
            .clone()
            .unwrap_or_else(|| "no destination".into())
    };
    let mut parts = vec![name_text(lane), routing];
    if lane.mute {
        parts.push("muted".into());
    }
    if lane.solo {
        parts.push("soloed".into());
    }
    if lane.arm {
        parts.push("armed".into());
    }
    parts.join(", ")
}

/// A row's registry label: "<Kind> track, <header>".
pub fn row_label(lane: &LaneRow) -> String {
    format!("{}, {}", kind_name(lane.kind), header(lane))
}

fn kind_glyph(kind: TrackKindRow) -> &'static str {
    match kind {
        TrackKindRow::Audio => "\u{223f}",
        TrackKindRow::Instrument => "\u{266b}",
        TrackKindRow::Midi => "\u{266a}",
    }
}

/// What the column draws.
pub struct HeadersState<'a> {
    pub lanes: &'a [LaneRow],
    pub selected: &'a [u64],
}

impl TrackHeaders {
    /// The column, drawn into document window `cx` (whose
    /// `track_headers` this is) and recorded in `scope`.
    pub fn render(
        &self,
        state: &HeadersState,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<DocumentWindow>,
    ) -> AnyElement {
        let mut column = div()
            .id("track-headers")
            .relative()
            .flex()
            .flex_col()
            .w(px(WIDTH))
            .h_full()
            .flex_none()
            .overflow_hidden()
            .bg(rgb(COLUMN))
            .child(scope.this("group", "Track Headers", true));
        for lane in state.lanes {
            let selected = state.selected.contains(&lane.id);
            column = column.child(self.row(lane, selected, scope, cx));
        }
        column = column.child(self.add_row(scope, window, cx));
        // The rows are the full 200 px; the line to the arrangement is
        // outside them.
        div()
            .flex()
            .flex_row()
            .h_full()
            .flex_none()
            .child(column)
            .child(div().w(px(1.)).h_full().bg(rgb(SEPARATOR)))
            .into_any_element()
    }

    fn row(
        &self,
        lane: &LaneRow,
        selected: bool,
        scope: &mut RegistryScope,
        cx: &mut Context<DocumentWindow>,
    ) -> AnyElement {
        let id = lane.id;
        let renaming = self
            .rename
            .as_ref()
            .filter(|r| r.lane == id)
            .map(|r| r.field.clone());
        let name = match renaming {
            Some(field) => div()
                .id(("track-name", id))
                .flex_1()
                .min_w_0()
                .h(px(ROW_HEIGHT - 8.))
                .overflow_hidden()
                .text_color(rgb(0x000000))
                .child(field),
            None => div()
                .id(("track-name", id))
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(name_text(lane))
                .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                    let m = e.modifiers();
                    let extend = m.shift || m.secondary(cx);
                    if !control::is_channel_click() && e.click_count() >= 2 && !extend {
                        cx.stop_propagation();
                        this.begin_rename(id, window, cx);
                    }
                })),
        };
        let toggles = div()
            .flex()
            .flex_row()
            .gap(px(2.))
            .child(toggle(
                ("mute", id),
                "M",
                lane.mute,
                MUTE_ON,
                cx,
                move |this, cx| {
                    let mute = this.lane(id, cx).map(|l| !l.mute);
                    this.set_flags(id, mute, None, None, cx);
                },
            ))
            .child(toggle(
                ("solo", id),
                "S",
                lane.solo,
                SOLO_ON,
                cx,
                move |this, cx| {
                    let solo = this.lane(id, cx).map(|l| !l.solo);
                    this.set_flags(id, None, solo, None, cx);
                },
            ))
            .child(toggle(
                ("arm", id),
                "R",
                lane.arm,
                ARM_ON,
                cx,
                move |this, cx| {
                    let arm = this.lane(id, cx).map(|l| !l.arm);
                    this.set_flags(id, None, None, arm, cx);
                },
            ));
        div()
            .id(("track-row", id))
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .w_full()
            .h(px(ROW_HEIGHT))
            .flex_none()
            .px(px(6.))
            .border_b_1()
            .border_color(rgb(SEPARATOR))
            .when(selected, |d| d.bg(rgba(SELECTED)))
            .child(
                div()
                    .w(px(16.))
                    .flex_none()
                    .text_color(rgb(colors::DIM))
                    .child(kind_glyph(lane.kind)),
            )
            .child(name)
            .child(toggles)
            .child(scope.control("button", &row_label(lane), true))
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                if control::is_channel_click() {
                    // Swift parity: the row's press toggles mute.
                    let mute = this.lane(id, cx).map(|l| !l.mute);
                    this.set_flags(id, mute, None, None, cx);
                    return;
                }
                let m = e.modifiers();
                let extend = m.shift || m.secondary(cx);
                this.select_track(id, extend, cx);
            }))
            .into_any_element()
    }

    fn add_row(
        &self,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<DocumentWindow>,
    ) -> AnyElement {
        let open = self.add_menu_open;
        let mut row = div()
            .id("add-track")
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h(px(ROW_HEIGHT))
            .flex_none()
            .px(px(6.))
            .cursor_pointer()
            .text_color(rgb(colors::DIM))
            .hover(|s| s.text_color(rgb(colors::TEXT)))
            .child("+ Add Track")
            .child(scope.control("button", "Add Track", true))
            .on_click(cx.listener(|this, _, _, cx| {
                if control::is_channel_click() {
                    // A menu would track modally: name the commands instead.
                    this.set_status(ADD_TRACK_ON_CHANNEL, cx);
                    return;
                }
                let headers = this.track_headers_mut();
                headers.add_menu_open = !headers.add_menu_open;
                cx.notify();
            }));
        if open {
            let size = window.viewport_size();
            row = row
                .child(
                    deferred(
                        anchored().position(point(px(0.), px(0.))).child(
                            div()
                                .id("add-track-backdrop")
                                .w(size.width)
                                .h(size.height)
                                .occlude()
                                .on_any_mouse_down(cx.listener(|this, _, _, cx| {
                                    this.track_headers_mut().add_menu_open = false;
                                    cx.notify();
                                })),
                        ),
                    )
                    .with_priority(1),
                )
                .child(
                    div().absolute().top_full().left(px(6.)).child(
                        deferred(
                            anchored()
                                .snap_to_window()
                                .child(add_menu(&mut scope.container(), cx)),
                        )
                        .with_priority(2),
                    ),
                );
        }
        row.into_any_element()
    }
}

fn toggle(
    id: (&'static str, u64),
    text: &'static str,
    on: bool,
    on_color: u32,
    cx: &mut Context<DocumentWindow>,
    action: impl Fn(&mut DocumentWindow, &mut Context<DocumentWindow>) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .w(px(20.))
        .h(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .text_xs()
        .cursor_pointer()
        .bg(rgb(if on { on_color } else { colors::BUTTON }))
        .text_color(rgb(colors::TEXT))
        .child(text)
        .on_click(cx.listener(move |this, _, _, cx| {
            // The toggles never change the selection.
            cx.stop_propagation();
            action(this, cx);
        }))
}

fn add_menu(scope: &mut RegistryScope, cx: &mut Context<DocumentWindow>) -> AnyElement {
    let mut list = div()
        .id("add-track-menu")
        .occlude()
        .flex()
        .flex_col()
        .min_w(px(200.))
        .py_1()
        .bg(rgb(LIST))
        .border_1()
        .border_color(rgb(BORDER))
        .rounded_md()
        .shadow_md()
        .text_color(rgb(colors::TEXT));
    for (row, (title, id)) in ADD_TRACK_ITEMS.into_iter().enumerate() {
        list = list.child(
            div()
                .id(("add-track-item", row))
                .relative()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors::BUTTON)))
                .child(title)
                .child(scope.control("menuitem", title, true))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.track_headers_mut().add_menu_open = false;
                    window.dispatch_action(Box::new(RunCommand::new(id)), cx);
                    cx.notify();
                })),
        );
    }
    list.into_any_element()
}
