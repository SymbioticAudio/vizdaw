//! The track header column beside the arrangement: one row per track of
//! the shown sequence (kind icon, name, the monitor button, M, S, R; the
//! input row under them; the input meter with its clip mark along the
//! bottom) and the "+ Add Track" row under the last, below a band as tall
//! as the arrangement's ruler (Swift `LaneHeaderView`,
//! `LaneHeaderLayout`). The rows scroll with the canvas (one shared
//! vertical offset). The column records as a `group` "Track Headers";
//! each row is one `button` whose label names its kind, name, routing,
//! input and monitoring (the M/S/R toggles are not separate elements, as
//! in the Swift app), and "Add Track" is the last child. A row's own
//! children are its monitor `button` (labelled with its help, which is
//! also its tooltip and its value), its input `menu` ("Input of <track>",
//! the text it shows as its value) and, while it shows, its input meter
//! (`level` "Input level of <track>").
//!
//! A click on a row's icon, name, input row (which also opens the input
//! picker) or empty space selects the track (Shift or the secondary
//! modifier adds or removes it); the monitor button, M, S and R set their
//! state and leave the selection. A control-channel click on a row
//! toggles mute. A double-click on the name opens the rename field, which
//! the document window owns (see [`DocumentWindow::begin_rename`]).

use gpui::prelude::FluentBuilder;
use std::rc::Rc;

use gpui::{
    AnyElement, ClickEvent, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Subscription, Window, anchored, deferred, div, point, px,
    rgb, rgba,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::lane_inputs::{InputChoice, LaneInputs};
use vizdaw_app::mixer::MixerMath;
use vizdaw_ffi::edit::MonitorModeRow;
use vizdaw_ffi::snapshot::LaneRow;
use vizdaw_ffi::tracks::TrackKindRow;

use crate::control;
use crate::input_picker::{self, PickerRow};
use crate::menus::RunCommand;
use crate::registry::RegistryScope;
use crate::sheet_host::with_help;
use crate::shell::DocumentWindow;
use crate::strip_palette::tokens;
use crate::text_input::TextInput;
use crate::transport_bar::colors;
use vizdaw_app::arrangement::RULER_HEIGHT;

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
    /// The track whose input picker is open.
    pub(crate) input_picker: Option<u64>,
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

    /// The track whose input picker is open.
    pub fn input_picker(&self) -> Option<u64> {
        self.input_picker
    }

    /// The open picker's rows for `d`'s track (`None`: none open, or its
    /// track is gone).
    pub(crate) fn picker_rows(&self, d: &DocumentModel) -> Option<(u64, Vec<PickerRow>)> {
        let id = self.input_picker?;
        let lane = d.lanes().iter().find(|l| l.id == id)?;
        Some((
            id,
            input_picker::rows(d.inputs().groups(lane), lane.input.as_ref()),
        ))
    }
}

/// Whether a row this tall has room for the input meter under its
/// controls (Swift `LaneHeaderLayout.showsMeter(laneHeight:)`).
pub fn shows_meter(lane_height: f32) -> bool {
    lane_height >= 46.
}

/// What a header shows of its track's input, read before drawing (held
/// data only: no session call).
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderInput {
    /// The input row's text after its arrow: "[⇠ <synth> · ]<input>".
    pub text: String,
    /// The synth whose return the track records.
    pub return_name: Option<String>,
    /// The input meter, while the track shows one.
    pub meter: Option<HeaderMeter>,
    /// The input is opening while this track meters.
    pub opening: bool,
}

/// A header's input meter as the poll left it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaderMeter {
    /// 0…2, falling back slowly.
    pub level: f32,
    /// The clip mark is lit.
    pub clipped: bool,
    /// The input is still opening (the track is the mid colour, dimmed).
    pub opening: bool,
}

impl HeaderInput {
    /// Track `lane`'s input as `d` holds it.
    pub fn of(d: &DocumentModel, lane: &LaneRow) -> Self {
        let inputs = d.inputs();
        let badge = d.input_badge(lane);
        let title = LaneInputs::title_of(lane);
        let text = match &badge {
            Some(b) => format!("{b} \u{b7} {title}"),
            None => title,
        };
        let metered = inputs.shows_meter(lane);
        HeaderInput {
            text,
            return_name: d.input_return_name(lane),
            meter: metered.then(|| HeaderMeter {
                level: inputs.level(lane.id),
                clipped: inputs.is_clipped(lane.id),
                opening: inputs.opening(),
            }),
            opening: metered && inputs.opening(),
        }
    }

    /// Every shown track's, in order.
    pub fn all(d: &DocumentModel) -> Vec<HeaderInput> {
        d.lanes().iter().map(|l| HeaderInput::of(d, l)).collect()
    }

    /// No input known (a track drawn before its input was read).
    fn none(lane: &LaneRow) -> Self {
        HeaderInput {
            text: LaneInputs::title_of(lane),
            return_name: None,
            meter: None,
            opening: false,
        }
    }
}

/// The input meter's value in the tree: its level, and ", clipped" while
/// the clip mark is lit.
pub fn meter_value(m: &HeaderMeter) -> String {
    let level = format!("{:.2}", MixerMath::sanitized_peak(m.level));
    if m.clipped {
        format!("{level}, clipped")
    } else {
        level
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

/// The header's words after its kind (Swift `ArrangementA11y.header`).
/// `return_of`: the synth whose return the track records (its badge).
pub fn header(lane: &LaneRow, return_of: Option<&str>) -> String {
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
    if lane.input.is_some() {
        parts.push(format!("input {}", LaneInputs::title_of(lane)));
    }
    if let Some(r) = return_of {
        parts.push(format!("recording the return of {r}"));
    }
    match lane.monitor {
        MonitorModeRow::Auto => {}
        MonitorModeRow::Input => parts.push("monitoring input".into()),
        MonitorModeRow::Off => parts.push("monitoring off".into()),
    }
    parts.join(", ")
}

/// A row's registry label (Swift `LaneHeaderView.accessibilityChildren`):
/// "<Kind> track, <header>", then ", input opening" while the input opens
/// and the track meters.
pub fn row_label(lane: &LaneRow, input: &HeaderInput) -> String {
    let opening = if input.opening { ", input opening" } else { "" };
    format!(
        "{}, {}{opening}",
        kind_name(lane.kind),
        header(lane, input.return_name.as_deref())
    )
}

/// The input row's text (Swift: "▾ [⇠ badge · ]<input>").
pub fn input_text(input: &HeaderInput) -> String {
    format!("\u{25BE} {}", input.text)
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
    /// Each track's input, in `lanes`' order.
    pub inputs: &'a [HeaderInput],
    /// The open input picker: its track and rows.
    pub picker: Option<(u64, Vec<PickerRow>)>,
    /// The rows' scroll, shared with the arrangement canvas.
    pub vertical_offset: f32,
}

/// A row's controls (Swift `LaneHeaderLayout.controls`): x, y from the
/// row's top, width, height.
mod place {
    pub const KIND: (f32, f32, f32, f32) = (8., 5., 16., 16.);
    pub const NAME: (f32, f32, f32, f32) = (28., 4., 81., 18.);
    pub const MONITOR: (f32, f32, f32, f32) = (113., 4., 18., 18.);
    pub const MUTE: (f32, f32, f32, f32) = (134., 4., 18., 18.);
    pub const SOLO: (f32, f32, f32, f32) = (155., 4., 18., 18.);
    pub const ARM: (f32, f32, f32, f32) = (176., 4., 18., 18.);
    pub const INPUT: (f32, f32, f32, f32) = (104., 24., 90., 17.);

    /// The input meter (3 high at 48 points and up, else 2 high a point
    /// higher, clear of the separator).
    pub fn meter(lane_height: f32) -> (f32, f32, f32, f32) {
        if lane_height >= 48. {
            (8., 43., 178., 3.)
        } else {
            (8., 42., 178., 2.)
        }
    }

    /// The clip mark at the meter's end.
    pub fn clip(lane_height: f32) -> (f32, f32, f32, f32) {
        let (_, y, _, h) = meter(lane_height);
        (189., y, 5., h)
    }
}

fn at(r: (f32, f32, f32, f32)) -> gpui::Div {
    div()
        .absolute()
        .left(px(r.0))
        .top(px(r.1))
        .w(px(r.2))
        .h(px(r.3))
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
        // The column stays put: a band as tall as the arrangement's ruler,
        // then the rows, moved by the vertical offset the canvas shares
        // (the wheel is the arrangement's). The group records the column
        // itself, the rows where they are drawn.
        let mut rows = div()
            .id("track-headers")
            .absolute()
            .left_0()
            .top(px(-state.vertical_offset))
            .w_full()
            .flex()
            .flex_col();
        for (i, lane) in state.lanes.iter().enumerate() {
            let selected = state.selected.contains(&lane.id);
            let input = state
                .inputs
                .get(i)
                .cloned()
                .unwrap_or_else(|| HeaderInput::none(lane));
            let picker = state
                .picker
                .as_ref()
                .filter(|(id, _)| *id == lane.id)
                .map(|(_, rows)| rows.clone());
            rows = rows.child(self.row(lane, &input, picker, selected, scope, window, cx));
        }
        rows = rows.child(self.add_row(scope, window, cx));
        let column = div()
            .relative()
            .w(px(WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(COLUMN))
            .child(scope.this("group", "Track Headers", true))
            .child(
                div()
                    .h(px(RULER_HEIGHT))
                    .flex_none()
                    .border_b_1()
                    .border_color(rgb(SEPARATOR)),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(rows)
                    .child(scope.clip()),
            );
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

    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        lane: &LaneRow,
        input: &HeaderInput,
        picker: Option<Vec<PickerRow>>,
        selected: bool,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<DocumentWindow>,
    ) -> AnyElement {
        let id = lane.id;
        // The row is a container: its monitor button, input row and meter
        // are its children in the tree.
        let mut rs = scope.container();
        let renaming = self
            .rename
            .as_ref()
            .filter(|r| r.lane == id)
            .map(|r| r.field.clone());
        let (nx, _, nw, _) = place::NAME;
        let name = match renaming {
            // The field, a little larger than the name (Swift
            // `insetBy(dx: -2, dy: -2)`).
            Some(field) => at((nx - 2., 2., nw + 4., 22.))
                .id(("track-name", id))
                .overflow_hidden()
                .text_color(rgb(0x000000))
                .child(field),
            None => at(place::NAME)
                .id(("track-name", id))
                .flex()
                .items_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_sm()
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
        let help = LaneInputs::monitor_help(lane.monitor);
        let monitor = with_help(
            at(place::MONITOR)
                .id(("monitor", id))
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .text_xs()
                .cursor_pointer()
                .bg(if lane.monitor == MonitorModeRow::Off {
                    rgb(colors::button())
                } else {
                    tokens().monitor(lane.monitor)
                })
                .text_color(rgb(colors::text()))
                .child(LaneInputs::monitor_label(lane.monitor))
                .child(rs.control_with("button", help, true, Some(help), false))
                .on_click(cx.listener(move |this, _, _, cx| {
                    // The monitor button never changes the selection.
                    cx.stop_propagation();
                    this.cycle_monitor(id, cx);
                })),
            help,
        );
        let input_label = format!("Input of {}", lane.name);
        let mut input_row = at(place::INPUT)
            .id(("track-input", id))
            .flex()
            .items_center()
            .cursor_pointer()
            .text_xs()
            .text_color(rgb(colors::dim()))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_middle()
                    .child(input_text(input)),
            )
            .child(rs.control_with("menu", &input_label, true, Some(&input.text), false))
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                // Swift: the press selects the track too, then the menu
                // opens.
                cx.stop_propagation();
                if !control::is_channel_click() {
                    let m = e.modifiers();
                    let extend = m.shift || m.secondary(cx);
                    this.select_track(id, extend, cx);
                }
                this.toggle_input_picker(id, cx);
            }));
        if let Some(rows) = picker {
            let weak = cx.entity().downgrade();
            let closing = weak.clone();
            let close: input_picker::Close = Rc::new(move |_, cx| {
                closing
                    .update(cx, |this, cx| {
                        this.track_headers_mut().input_picker = None;
                        cx.notify();
                    })
                    .ok();
            });
            let choose: input_picker::Choose = Rc::new(move |c: &InputChoice, _, cx| {
                let c = c.clone();
                weak.update(cx, |this, cx| this.choose_input(id, &c, cx))
                    .ok();
            });
            input_row = input_row.children(input_picker::popup(
                "input-picker",
                rows,
                &mut rs,
                window,
                close,
                choose,
            ));
        }
        let mut row = div()
            .id(("track-row", id))
            .relative()
            .w_full()
            .h(px(ROW_HEIGHT))
            .flex_none()
            .border_b_1()
            .border_color(rgb(SEPARATOR))
            .when(selected, |d| d.bg(rgba(SELECTED)))
            .child(rs.this("button", &row_label(lane, input), true))
            .child(
                at(place::KIND)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(colors::dim()))
                    .child(kind_glyph(lane.kind)),
            )
            .child(name)
            .child(monitor)
            .child(toggle(
                ("mute", id),
                place::MUTE,
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
                place::SOLO,
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
                place::ARM,
                "R",
                lane.arm,
                ARM_ON,
                cx,
                move |this, cx| {
                    let arm = this.lane(id, cx).map(|l| !l.arm);
                    this.set_flags(id, None, None, arm, cx);
                },
            ))
            .child(input_row);
        if let Some(m) = input.meter.filter(|_| shows_meter(ROW_HEIGHT)) {
            row = row.children(meter(lane, &m, ROW_HEIGHT, &mut rs));
        }
        row.on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
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
            .text_color(rgb(colors::dim()))
            .hover(|s| s.text_color(rgb(colors::text())))
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

#[allow(clippy::too_many_arguments)]
fn toggle(
    id: (&'static str, u64),
    r: (f32, f32, f32, f32),
    text: &'static str,
    on: bool,
    on_color: u32,
    cx: &mut Context<DocumentWindow>,
    action: impl Fn(&mut DocumentWindow, &mut Context<DocumentWindow>) + 'static,
) -> impl IntoElement {
    at(r)
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .text_xs()
        .cursor_pointer()
        .bg(rgb(if on { on_color } else { colors::button() }))
        .text_color(rgb(colors::text()))
        .child(text)
        .on_click(cx.listener(move |this, _, _, cx| {
            // The toggles never change the selection.
            cx.stop_propagation();
            action(this, cx);
        }))
}

/// The thin input meter and its clip mark (Swift `LaneHeaderView.meter`):
/// the meter background (the mid colour, dimmed, while the input opens),
/// the level in the low colour, and the clip mark lit in the clip colour.
fn meter(
    lane: &LaneRow,
    m: &HeaderMeter,
    lane_height: f32,
    scope: &mut RegistryScope,
) -> [AnyElement; 2] {
    let t = tokens();
    let track = if m.opening {
        let mut c = rgb(t.meter_mid);
        c.a = 0.5;
        c
    } else {
        rgb(t.meter_background)
    };
    let fill = MixerMath::meter_fill(m.level) as f32;
    let r = place::meter(lane_height);
    let label = format!("Input level of {}", lane.name);
    let bar = at(r)
        .bg(track)
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .h_full()
                .w(px(r.2 * fill))
                .bg(rgb(t.meter_low)),
        )
        .child(scope.control_with("level", &label, true, Some(&meter_value(m)), false))
        .into_any_element();
    let clip = at(place::clip(lane_height))
        .bg(rgb(if m.clipped {
            t.meter_clip
        } else {
            t.meter_background
        }))
        .into_any_element();
    [bar, clip]
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
        .text_color(rgb(colors::text()));
    for (row, (title, id)) in ADD_TRACK_ITEMS.into_iter().enumerate() {
        list = list.child(
            div()
                .id(("add-track-item", row))
                .relative()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors::button())))
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

impl DocumentWindow {
    /// The monitor button: the track's next mode (one undo step).
    pub(crate) fn cycle_monitor(&mut self, lane: u64, cx: &mut Context<Self>) {
        let Some(l) = self.lane(lane, cx) else { return };
        self.with_model(cx, |d| d.cycle_lane_monitor(&l));
    }

    /// Opens track `lane`'s input picker (reading the sources again, off
    /// the UI thread), or closes it when it is open.
    pub(crate) fn toggle_input_picker(&mut self, lane: u64, cx: &mut Context<Self>) {
        let headers = self.track_headers_mut();
        if headers.input_picker == Some(lane) {
            headers.input_picker = None;
        } else {
            headers.input_picker = Some(lane);
            headers.add_menu_open = false;
            self.refresh_inputs(cx);
        }
        cx.notify();
    }

    /// A picker's choice for track `lane` (nothing when it is the input
    /// now; a refusal is the status line).
    pub(crate) fn choose_input(&mut self, lane: u64, choice: &InputChoice, cx: &mut Context<Self>) {
        let Some(l) = self.lane(lane, cx) else { return };
        self.with_model(cx, |d| d.choose_input(choice, &l));
    }
}
