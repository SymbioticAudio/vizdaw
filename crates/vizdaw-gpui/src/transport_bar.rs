//! The transport bar, drawn from the app's [`TransportLayout`] (Swift:
//! `TransportRowView`, `TransportItemViews`, `TransportReadoutsView`,
//! `TransportFieldsView`): the cluster (Return to Start, Rewind, Fast
//! Forward, Play/Stop), Loop, the four readouts, the Position and Edit
//! In/Out fields, flexible space, the Main View picker and the pane
//! toggles, and Faults with its popover (document window only); what
//! does not fit folds, last first, into the » menu. Recording, the click
//! and step entry come with their slices and are not drawn yet.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, Div, ElementId, Focusable, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, Rgba, Stateful, StatefulInteractiveElement, Styled, Window,
    anchored, canvas, deferred, div, point, px, rgb,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::editor::{MainView, PaneSide};
use vizdaw_app::transport::{ReadoutBox, TransportReadout};
use vizdaw_app::transport_bar::{Fit, ItemKind, TransportItem, TransportLayout, fit};
use vizdaw_app::transport_fields::TransportField;

use crate::control;
use crate::menus::{PlayStop, RunCommand};
use crate::panes;
use crate::registry::RegistryScope;
use crate::shell::DocumentWindow;
use crate::transport_fields::TransportFields;

/// What the bar draws.
pub struct BarState {
    pub readout: TransportReadout,
    pub can_play: bool,
    /// A sequence is shown (Return to Start, Rewind and Fast Forward need
    /// one).
    pub has_sequence: bool,
    pub looping: bool,
    /// What each field shows when it is not being typed in.
    pub field_texts: Vec<(TransportField, String)>,
    /// The window's main view.
    pub main: MainView,
    /// Whether the main view shows each pane (left, bottom, right).
    pub panes: [bool; 3],
    /// Faults, while there are faults or a report (document window only).
    pub faults: Option<FaultsState>,
    /// The cluster's Record part and the record items.
    pub record: RecordBar,
    /// The Step Entry summary, while step entry is on.
    pub summary: Option<String>,
}

/// What the Record part of the cluster, Replace/Overdub and Count-in draw
/// (Swift `RecordClusterPart`, `ReplaceOverdubButton`, `CountInItem`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordBar {
    /// A pass records (counting in too): Stop Recording, in red.
    pub recording: bool,
    /// Record is enabled (a field typing does not turn it off).
    pub enabled: bool,
    pub counting_in: bool,
    /// The loop pass text (`Pass N · K kept · D discarded`).
    pub pass: Option<String>,
    /// The time left on the project's disk.
    pub remaining: Option<String>,
    /// `Replace` or `Overdub`.
    pub mode_title: &'static str,
    pub overdub: bool,
    /// Count-in is on (bars > 0).
    pub count_in: bool,
}

impl RecordBar {
    pub fn of(d: &DocumentModel) -> RecordBar {
        RecordBar {
            recording: d.record().is_recording(),
            enabled: d.record_button_enabled(),
            counting_in: d.counting_in(),
            pass: d.record_pass_text(),
            remaining: d.record_remaining_text(),
            mode_title: d.record_mode_title(),
            overdub: !d.record_replaces(),
            count_in: d.record().count_in_bars() > 0,
        }
    }

    /// About the width the readouts after Record take, gaps included,
    /// so what follows the cluster folds rather than being pushed out.
    fn readouts_width(&self) -> f32 {
        let text = |t: &str, per_char: f32| CLUSTER_GAP + per_char * t.chars().count() as f32;
        let mut w = 0.;
        if self.counting_in {
            w += text(COUNTING_IN, READOUT_CHAR_SM);
        }
        if let Some(p) = &self.pass {
            w += text(p, READOUT_CHAR_MONO);
        }
        if let Some(t) = &self.remaining {
            w += text(t, READOUT_CHAR_XS);
        }
        w
    }

    /// The mode switch's accessibility label (Swift `modeLabel`).
    fn mode_label(&self) -> String {
        format!("Record mode: {}", self.mode_title)
    }
}

/// What the Faults item draws (Swift `FaultsItem`).
pub struct FaultsState {
    /// The popover's lines (Swift `messages`).
    pub lines: Vec<String>,
    /// Real-time faults: the error tint, else the warning tint.
    pub error: bool,
    /// The popover is open.
    pub open: bool,
}

impl BarState {
    /// Which of the now-and-then items are there.
    pub fn shows(&self) -> Shows {
        Shows {
            faults: self.faults.is_some(),
            summary: self.summary.is_some(),
            cluster_readouts: self.record.readouts_width(),
        }
    }

    fn field_text(&self, f: TransportField) -> &str {
        self.field_texts
            .iter()
            .find(|(g, _)| *g == f)
            .map_or("", |(_, t)| t.as_str())
    }
}

pub(crate) mod colors {
    pub const BAR: u32 = 0x26262e;
    pub const BUTTON: u32 = 0x3a3f4f;
    pub const PLAYING: u32 = 0x2f6f4f;
    pub const ON: u32 = 0x3f5f8f;
    pub const READOUT: u32 = 0x15151a;
    pub const FIELD: u32 = 0xf4f4f6;
    pub const TEXT: u32 = 0xe8e8ec;
    pub const DISABLED: u32 = 0x77777f;
    pub const DIM: u32 = 0x9a9aa6;
    pub const LIST: u32 = 0x2e2e38;
    pub const BORDER: u32 = 0x4a4a56;
}

/// The row's padding and the gap between its pieces (Swift: 8 and the
/// theme's medium spacing).
pub const PADDING: f32 = 8.;
pub const GAP: f32 = 8.;
/// The » button's width.
pub const CHEVRON: f32 = 28.;
const CLUSTER_BUTTON: f32 = 32.;
const CLUSTER_GAP: f32 = 4.;
const LOOP: f32 = 48.;
/// A field's width (Swift `fieldWidth`).
const FIELD: f32 = 86.;
const LABEL_GAP: f32 = 4.;
const HEIGHT: f32 = 36.;
/// A Main View segment's width.
const SEGMENT: f32 = 60.;
/// Replace/Overdub's and Count-in's widths.
const MODE: f32 = 72.;
const COUNT_IN: f32 = 72.;
/// The summary pill's widest ("Step Entry · <a long track name> · 1/16
/// dotted triplet" in the caption size).
const SUMMARY: f32 = 260.;
/// About a character's width in the cluster's readouts (small, caption
/// and the pass's monospaced caption).
const READOUT_CHAR_SM: f32 = 8.;
const READOUT_CHAR_XS: f32 = 7.;
const READOUT_CHAR_MONO: f32 = 7.5;

/// The Main View picker's label (Swift `MainViewItem`).
pub const MAIN_VIEW: &str = "Main View";

/// The Faults button's label and help (Swift
/// `V/TransportItemViews.swift:229-230`), and its popover's least width.
pub const FAULTS: &str = "Faults";
pub const FAULTS_HELP: &str = "Faults and compile messages";
const FAULTS_MIN_WIDTH: f32 = 240.;

/// The pane toggles' commands, in bar order (Swift
/// `TransportItemEntries.panes`).
const PANE_TOGGLES: [(&str, PaneSide, &str); 3] = [
    ("ui.toggle_left", PaneSide::Left, "\u{25e7}"),
    ("ui.toggle_bottom", PaneSide::Bottom, "\u{2b13}"),
    ("ui.toggle_right", PaneSide::Right, "\u{25e8}"),
];

/// The commands that show each main view.
fn view_command(main: MainView) -> &'static str {
    match main {
        MainView::Arrange => "ui.view_arrange",
        MainView::Mix => "ui.view_mix",
    }
}

/// The » button's label, and what a channel click on it says (Swift
/// `TransportRowView.channelNote`).
pub const MORE: &str = "More Transport Items";
pub const MORE_ON_CHANNEL: &str =
    "The \u{bb} menu does not open for the control channel; use ui.set_transport_bar.";

/// One drawn piece of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece {
    Cluster,
    Loop,
    Readout(ReadoutBox),
    Position,
    EditFields,
    Space,
    MainView,
    Panes,
    Faults,
    ReplaceOverdub,
    CountIn,
    Summary,
    Separator,
}

/// What shows only now and then: an item that is absent takes no room
/// (Swift: a zero-width item is neither shown nor folded).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Shows {
    /// Faults: there are faults or a report.
    pub faults: bool,
    /// The Step Entry summary: step entry is on.
    pub summary: bool,
    /// The width the cluster's readouts after Record take (count-in, the
    /// pass, the time left), gaps included.
    pub cluster_readouts: f32,
}

/// The Record button's help (Swift `V/RecordControls.swift:27`).
pub const RECORD_HELP: &str = "Record the armed tracks (R)";
pub const STOP_RECORDING_HELP: &str = "Stop recording (R)";
/// The time left's help (`V/RecordControls.swift:46`).
pub const REMAINING_HELP: &str =
    "Recording time left on the project's disk for the armed audio tracks";
/// Count-in's help (`V/TransportItemViews.swift:188`).
pub const COUNT_IN_HELP: &str = "Count in before recording (the bars are in Record Options)";
/// The readout while counting in (`V/RecordControls.swift:37`).
pub const COUNTING_IN: &str = "Count-in";

impl Piece {
    fn of(item: &TransportItem) -> Option<Piece> {
        Some(match item {
            TransportItem::Cluster => Piece::Cluster,
            TransportItem::Loop => Piece::Loop,
            TransportItem::Bar(_) => Piece::Readout(ReadoutBox::Bar),
            TransportItem::Time(_) => Piece::Readout(ReadoutBox::Time),
            TransportItem::Tempo(_) => Piece::Readout(ReadoutBox::Tempo),
            TransportItem::Signature => Piece::Readout(ReadoutBox::Signature),
            TransportItem::PositionField => Piece::Position,
            TransportItem::EditFields => Piece::EditFields,
            TransportItem::Space => Piece::Space,
            TransportItem::MainView => Piece::MainView,
            TransportItem::Panes => Piece::Panes,
            TransportItem::Faults => Piece::Faults,
            TransportItem::ReplaceOverdub => Piece::ReplaceOverdub,
            TransportItem::CountIn => Piece::CountIn,
            TransportItem::Summary => Piece::Summary,
            TransportItem::Separator => Piece::Separator,
            _ => return None,
        })
    }

    /// The width it takes. Fixed, so the readouts fit their widest texts
    /// ("9999.4.960", "999:59.999", "999.99", "255/128") and the bar
    /// folds the same way whatever is playing.
    pub fn width(self) -> f32 {
        match self {
            // Return to Start, Rewind, Fast Forward, Play/Stop and Record.
            Piece::Cluster => 5. * CLUSTER_BUTTON + 4. * CLUSTER_GAP,
            Piece::Loop => LOOP,
            Piece::Readout(b) => readout_width(b),
            Piece::Position => label_width(TransportField::Position) + LABEL_GAP + FIELD,
            Piece::EditFields => {
                label_width(TransportField::EditIn)
                    + label_width(TransportField::EditOut)
                    + 2. * (LABEL_GAP + FIELD)
                    + GAP
            }
            Piece::Space => 0.,
            Piece::MainView => 2. * SEGMENT + 1.,
            Piece::Panes => 3. * CLUSTER_BUTTON + 2. * CLUSTER_GAP,
            Piece::Faults => CLUSTER_BUTTON,
            Piece::ReplaceOverdub => MODE,
            Piece::CountIn => COUNT_IN,
            Piece::Summary => SUMMARY,
            Piece::Separator => 1.,
        }
    }

    /// Its title in the » menu (Swift `TransportItemID.title`).
    pub fn title(self) -> &'static str {
        match self {
            Piece::Cluster => "Transport Controls",
            Piece::Loop => "Loop",
            Piece::Readout(ReadoutBox::Bar) => "Position in Bars",
            Piece::Readout(ReadoutBox::Time) => "Position in Time",
            Piece::Readout(ReadoutBox::Tempo) => "Tempo",
            Piece::Readout(ReadoutBox::Signature) => "Time Signature",
            Piece::Position => "Position Field",
            Piece::EditFields => "Edit In and Out",
            Piece::Space => "Flexible Space",
            Piece::MainView => MAIN_VIEW,
            Piece::Panes => "Pane Toggles",
            Piece::Faults => FAULTS,
            Piece::ReplaceOverdub => "Replace/Overdub",
            Piece::CountIn => "Count-in",
            Piece::Summary => "Step Entry",
            Piece::Separator => "Separator",
        }
    }

    fn fields(self) -> &'static [TransportField] {
        match self {
            Piece::Position => &[TransportField::Position],
            Piece::EditFields => &[TransportField::EditIn, TransportField::EditOut],
            _ => &[],
        }
    }
}

fn readout_width(b: ReadoutBox) -> f32 {
    match b {
        ReadoutBox::Bar | ReadoutBox::Time => 84.,
        ReadoutBox::Tempo => 56.,
        ReadoutBox::Signature => 60.,
    }
}

fn label_width(f: TransportField) -> f32 {
    match f {
        TransportField::Position | TransportField::EditOut => 52.,
        TransportField::EditIn => 44.,
        // The Punch In and Out item is drawn from slice 8b.
        TransportField::PunchIn => 56.,
        TransportField::PunchOut => 64.,
    }
}

/// Which pieces a bar of `width` shows and which fold into ».
pub struct Plan {
    pieces: Vec<Piece>,
    fit: Fit,
}

impl Plan {
    /// A document window's bar; Faults takes room only while it shows
    /// (`faults`).
    pub fn new(layout: &TransportLayout, width: f32, shows: Shows) -> Plan {
        Plan::for_window(layout, width, true, shows)
    }

    /// The bar of a document window (`document_window`) or of a Mixer
    /// window, which leaves out the document-only items.
    pub fn for_window(
        layout: &TransportLayout,
        width: f32,
        document_window: bool,
        shows: Shows,
    ) -> Plan {
        let pieces: Vec<Piece> = layout
            .items_for(document_window)
            .into_iter()
            .filter_map(Piece::of)
            .filter(|p| match p {
                Piece::Faults => shows.faults,
                Piece::Summary => shows.summary,
                _ => true,
            })
            .collect();
        let widths: Vec<(f32, bool, ItemKind)> = pieces
            .iter()
            .map(|p| {
                let kind = match p {
                    Piece::Space => ItemKind::Space,
                    Piece::Separator => ItemKind::Separator,
                    _ => ItemKind::Content,
                };
                let width = match p {
                    Piece::Cluster => p.width() + shows.cluster_readouts,
                    _ => p.width(),
                };
                (width, *p == Piece::Cluster, kind)
            })
            .collect();
        let fit = fit(&widths, width - 2. * PADDING, GAP, CHEVRON);
        Plan { pieces, fit }
    }

    pub fn shown(&self) -> Vec<Piece> {
        self.fit.shown.iter().map(|&i| self.pieces[i]).collect()
    }

    pub fn folded(&self) -> Vec<Piece> {
        self.fit.folded.iter().map(|&i| self.pieces[i]).collect()
    }

    /// The fields drawn, in bar order (Tab goes through them).
    pub fn shown_fields(&self) -> Vec<TransportField> {
        self.shown()
            .into_iter()
            .flat_map(|p| p.fields().iter().copied())
            .collect()
    }
}

fn color(enabled: bool) -> Rgba {
    rgb(if enabled {
        colors::TEXT
    } else {
        colors::DISABLED
    })
}

/// A push button that records itself in `scope`. Disabled buttons take no
/// clicks; the caller adds `on_click` to enabled ones.
pub(crate) fn button(
    id: impl Into<ElementId>,
    label: &str,
    enabled: bool,
    background: u32,
    scope: &mut RegistryScope,
) -> Stateful<Div> {
    face(id, label, enabled, background)
        .min_w(px(96.))
        .px_3()
        .child(scope.control("button", label, enabled))
}

/// As [`button`], with `help` (Swift `.help`) as its tooltip and its
/// `value` in the tree.
pub(crate) fn button_with_help(
    id: impl Into<ElementId>,
    label: &str,
    enabled: bool,
    help: Option<&str>,
    background: u32,
    scope: &mut RegistryScope,
) -> Stateful<Div> {
    let face = face(id, label, enabled, background)
        .min_w(px(96.))
        .px_3()
        .child(scope.control_with("button", label, enabled, help, false));
    match help {
        Some(h) => crate::sheet_host::with_help(face, h.to_owned()),
        None => face,
    }
}

/// A button's look, showing `text`, unrecorded.
fn face(id: impl Into<ElementId>, text: &str, enabled: bool, background: u32) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .h(px(28.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(rgb(background))
        .text_color(color(enabled))
        .when(enabled, |b| b.cursor_pointer())
        .child(text.to_owned())
}

/// A cluster button: a glyph, with the Swift label in the tree.
pub(crate) fn cluster_button(
    id: &'static str,
    glyph: &str,
    label: &str,
    enabled: bool,
    background: u32,
    scope: &mut RegistryScope,
) -> Stateful<Div> {
    face(id, glyph, enabled, background)
        .w(px(CLUSTER_BUTTON))
        .child(scope.control("button", label, enabled))
}

/// Rewind or Fast Forward: pressed on mouse-down (⇧: a marker step on
/// release), held it winds, the release lands. A release elsewhere is the
/// window root's (`shuttle_release(false)`).
fn hold_button(
    forward: bool,
    enabled: bool,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> Stateful<Div> {
    let (id, glyph, label) = if forward {
        ("fast-forward", "\u{25b6}\u{25b6}", "Fast Forward")
    } else {
        ("rewind", "\u{25c0}\u{25c0}", "Rewind")
    };
    cluster_button(id, glyph, label, enabled, colors::BUTTON, scope).when(enabled, |b| {
        b.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                this.shuttle_press(forward, e.modifiers.shift, window, cx)
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.shuttle_release(true, cx)),
        )
    })
}

fn cluster(
    state: &BarState,
    fields: &TransportFields,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let playing = state.readout.playing;
    let seq = state.has_sequence;
    let ret = cluster_button(
        "return-to-start",
        "|\u{25c0}",
        "Return to Start",
        seq,
        colors::BUTTON,
        scope,
    )
    .when(seq, |b| {
        b.on_click(cx.listener(|this, _, _, cx| this.return_to_start(cx)))
    });
    let rewind = hold_button(false, seq, scope, cx);
    let forward = hold_button(true, seq, scope, cx);
    let (glyph, label) = if playing {
        ("\u{25a0}", "Stop")
    } else {
        ("\u{25b6}", "Play")
    };
    let play = cluster_button(
        "play-stop",
        glyph,
        label,
        state.can_play,
        if playing {
            colors::PLAYING
        } else {
            colors::BUTTON
        },
        scope,
    )
    .when(state.can_play, |b| {
        b.on_click(cx.listener(|this, _, window, cx| this.play_stop(&PlayStop, window, cx)))
    });
    let mut row = div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .gap(px(CLUSTER_GAP))
        .child(ret)
        .child(rewind)
        .child(forward)
        .child(play)
        .child(record_button(&state.record, fields, scope, cx));
    row = record_readouts(&state.record, row, scope);
    row.into_any_element()
}

/// Record (Swift `RecordClusterPart`): `Stop Recording` in the record
/// colour while a pass records (counting in too). Enabled while a field
/// types; its mouse-down does not take the keyboard (with a punch draft
/// open the field keeps it), and a press is the R key's `press_record`,
/// a refusal in the status line.
fn record_button(
    r: &RecordBar,
    fields: &TransportFields,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let tokens = crate::strip_palette::tokens();
    let (label, help) = if r.recording {
        ("Stop Recording", STOP_RECORDING_HELP)
    } else {
        ("Record", RECORD_HELP)
    };
    let enabled = r.enabled;
    let bounds = fields.record_button.clone();
    let mut button = face("record", "\u{25cf}", enabled, colors::BUTTON)
        .w(px(CLUSTER_BUTTON))
        .child(scope.control("button", label, enabled))
        // Where it is, so a field's edit knows a press on it.
        .child(
            canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {})
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
        );
    if r.recording {
        button = button
            .bg(rgb(tokens.state_record))
            .text_color(rgb(colors::TEXT));
    } else if enabled {
        button = button.text_color(rgb(tokens.state_record));
    }
    let button = button.when(enabled, |b| {
        b.on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(cx.listener(|this, _, window, cx| this.press_record(window, cx)))
    });
    crate::sheet_host::with_help(button, help.to_owned()).into_any_element()
}

/// The cluster's readouts after Record: `Count-in` (record colour) while
/// counting in, the loop pass, and the time left.
fn record_readouts(r: &RecordBar, mut row: Div, scope: &mut RegistryScope) -> Div {
    let tokens = crate::strip_palette::tokens();
    if r.counting_in {
        row = row.child(
            div()
                .relative()
                .flex_none()
                .whitespace_nowrap()
                .text_sm()
                .text_color(rgb(tokens.state_record))
                .child(COUNTING_IN)
                .child(scope.control("text", COUNTING_IN, true)),
        );
    }
    if let Some(p) = &r.pass {
        row = row.child(
            div()
                .relative()
                .flex_none()
                .whitespace_nowrap()
                .text_xs()
                .font_family("Menlo")
                .text_color(rgb(colors::TEXT))
                .child(p.clone())
                .child(scope.control("text", p, true)),
        );
    }
    if let Some(t) = &r.remaining {
        let text = div()
            .id("record-remaining")
            .relative()
            .flex_none()
            .whitespace_nowrap()
            .text_xs()
            .text_color(rgb(colors::DIM))
            .child(t.clone())
            .child(scope.control("text", t, true));
        row = row.child(crate::sheet_host::with_help(
            text,
            REMAINING_HELP.to_owned(),
        ));
    }
    row
}

/// Replace/Overdub (Swift `ReplaceOverdubButton`): `Replace` or
/// `Overdub`, on for overdub; disabled while recording; a press runs
/// `transport.overdub` (a refusal in the status line).
fn replace_overdub(
    r: &RecordBar,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let enabled = !r.recording;
    let button = face(
        "replace-overdub",
        r.mode_title,
        enabled,
        if r.overdub {
            colors::ON
        } else {
            colors::BUTTON
        },
    )
    .w(px(MODE))
    .child(scope.control_with(
        "button",
        &r.mode_label(),
        enabled,
        Some(if r.overdub { "on" } else { "off" }),
        false,
    ))
    .when(enabled, |b| {
        b.on_click(cx.listener(|this, _, _, cx| this.press_record_mode(cx)))
    });
    crate::sheet_host::with_help(button, vizdaw_app::record::MODE_HELP.to_owned())
        .into_any_element()
}

/// Count-in (Swift `CountInItem`): on while it counts bars in; disabled
/// while recording; a press goes off and back to its last bars.
fn count_in(
    r: &RecordBar,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let enabled = !r.recording;
    let button = face(
        "count-in",
        "Count-in",
        enabled,
        if r.count_in {
            colors::ON
        } else {
            colors::BUTTON
        },
    )
    .w(px(COUNT_IN))
    .child(scope.control_with(
        "button",
        "Count-in",
        enabled,
        Some(if r.count_in { "on" } else { "off" }),
        false,
    ))
    .when(enabled, |b| {
        b.on_click(cx.listener(|this, _, _, cx| this.toggle_count_in(cx)))
    });
    crate::sheet_host::with_help(button, COUNT_IN_HELP.to_owned()).into_any_element()
}

/// The Step Entry summary (Swift `StepSummaryItem`): an accent pill; its
/// accessibility label is the text.
fn summary(text: &str, scope: &mut RegistryScope) -> AnyElement {
    let tokens = crate::strip_palette::tokens();
    let [r, g, b] = [16, 8, 0].map(|s| ((tokens.accent >> s) & 0xff) as f32 / 255.);
    div()
        .relative()
        .flex_none()
        .max_w(px(SUMMARY))
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .px_2()
        .py(px(2.))
        .rounded_sm()
        .bg(Rgba { r, g, b, a: 0.18 })
        .text_xs()
        .text_color(rgb(colors::TEXT))
        .child(text.to_owned())
        .child(scope.control("text", text, true))
        .into_any_element()
}

/// A separator: a thin line in the theme's separator colour.
fn separator() -> AnyElement {
    let tokens = crate::strip_palette::tokens();
    div()
        .flex_none()
        .w(px(1.))
        .h(px(18.))
        .bg(rgb(tokens.surface_separator))
        .into_any_element()
}

fn loop_button(
    on: bool,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let value = if on { "on" } else { "off" };
    face(
        "loop",
        "Loop",
        true,
        if on { colors::ON } else { colors::BUTTON },
    )
    .w(px(LOOP))
    .child(scope.control_with("button", "Loop", true, Some(value), false))
    .on_click(cx.listener(|this, _, _, cx| this.toggle_loop(cx)))
    .into_any_element()
}

fn readout(which: ReadoutBox, text: &str, scope: &mut RegistryScope) -> AnyElement {
    div()
        .relative()
        .flex_none()
        .w(px(readout_width(which)))
        .h(px(HEIGHT))
        .px_2()
        .flex()
        .flex_col()
        .justify_center()
        .rounded_md()
        .bg(rgb(colors::READOUT))
        .overflow_hidden()
        .whitespace_nowrap()
        .child(
            div()
                .text_xs()
                .text_color(rgb(colors::DIM))
                .child(which.title()),
        )
        .child(div().text_color(rgb(colors::TEXT)).child(text.to_owned()))
        .child(scope.control_with("text", which.accessibility_label(), true, Some(text), false))
        .into_any_element()
}

/// A field with its label: the model's text until a click (or its key)
/// starts an edit, then the text input.
fn field(
    f: TransportField,
    state: &BarState,
    fields: &TransportFields,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let input = fields.input(f).clone();
    let editing = fields.editing() == Some(f);
    let focused = editing && input.focus_handle(cx).is_focused(window);
    let value = if editing {
        input.read(cx).text().to_owned()
    } else {
        state.field_text(f).to_owned()
    };
    let boxed = div()
        .id(("transport-field", f as usize))
        .relative()
        .flex_none()
        .w(px(FIELD))
        .h(px(24.))
        .rounded_sm()
        .overflow_hidden()
        .text_color(rgb(0x000000));
    let boxed = if editing {
        boxed.child(input).on_mouse_down_out(cx.listener(
            move |this, e: &MouseDownEvent, window, cx| {
                // A press elsewhere ends the edit, then goes on to what it
                // hit (on Record with a punch draft open it keeps it).
                this.press_outside_field(f, e.position, window, cx)
            },
        ))
    } else {
        boxed
            .bg(rgb(colors::FIELD))
            .px(px(4.))
            .flex()
            .items_center()
            .text_size(px(13.))
            .whitespace_nowrap()
            .cursor_text()
            .child(state.field_text(f).to_owned())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    // The first click selects all the text; the window's
                    // root must not take the focus back.
                    window.prevent_default();
                    this.begin_field(f, window, cx)
                }),
            )
    };
    let boxed =
        boxed.child(scope.control_with("textfield", f.title(), true, Some(&value), focused));
    div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .gap(px(LABEL_GAP))
        .child(
            div()
                .w(px(label_width(f)))
                .flex_none()
                .text_xs()
                .text_color(rgb(colors::DIM))
                .whitespace_nowrap()
                .child(f.title()),
        )
        .child(boxed)
        .into_any_element()
}

/// The Main View picker (Swift `MainViewItem`, a segmented picker): one
/// segment per view, the current one selected; a click runs the View
/// menu's command. `scope` is the picker's own container.
fn main_view(
    current: MainView,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let mut picker = div()
        .relative()
        .flex()
        .flex_row()
        .flex_none()
        .gap(px(1.))
        .child(scope.this("group", MAIN_VIEW, true));
    for main in MainView::ALL {
        let on = main == current;
        let title = panes::main_title(main);
        picker = picker.child(
            face(
                ("main-view", main as usize),
                title,
                true,
                if on { colors::ON } else { colors::BUTTON },
            )
            .w(px(SEGMENT))
            .child(scope.control_selected("button", title, true, None, on))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.run_command(&RunCommand::new(view_command(main)), window, cx)
            })),
        );
    }
    picker.into_any_element()
}

/// The pane toggles (Swift `PanesItem`): one per pane, on while the main
/// view shows it; a click runs the View menu's toggle.
fn pane_toggles(
    on: [bool; 3],
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let mut row = div().flex().flex_row().flex_none().gap(px(CLUSTER_GAP));
    for (id, side, glyph) in PANE_TOGGLES {
        let shown = on[side as usize];
        row = row.child(
            face(
                ("pane-toggle", side as usize),
                glyph,
                true,
                if shown { colors::ON } else { colors::BUTTON },
            )
            .w(px(CLUSTER_BUTTON))
            .child(scope.control_with(
                "button",
                panes::side_title(side),
                true,
                Some(if shown { "on" } else { "off" }),
                false,
            ))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.run_command(&RunCommand::new(id), window, cx)
            })),
        );
    }
    row.into_any_element()
}

fn more_button(
    open: bool,
    folded: &[Piece],
    state: &BarState,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let mut chevron = face("more", "\u{bb}", true, colors::BUTTON)
        .w(px(CHEVRON))
        .child(scope.control("button", MORE, true))
        .on_click(cx.listener(|this, _, _, cx| {
            if control::is_channel_click() {
                // The menu would track modally: name the command instead.
                this.set_status(MORE_ON_CHANNEL, cx);
                return;
            }
            this.toggle_more(cx);
        }));
    if open {
        let size = window.viewport_size();
        chevron = chevron
            .child(
                deferred(
                    anchored().position(point(px(0.), px(0.))).child(
                        div()
                            .id("more-backdrop")
                            .w(size.width)
                            .h(size.height)
                            .occlude()
                            .on_any_mouse_down(cx.listener(|this, _, _, cx| this.close_more(cx))),
                    ),
                )
                .with_priority(1),
            )
            .child(
                div().absolute().top_full().right_0().child(
                    deferred(anchored().snap_to_window().child(more_menu(
                        folded,
                        state,
                        &mut scope.container(),
                        cx,
                    )))
                    .with_priority(2),
                ),
            );
    }
    chevron.into_any_element()
}

/// The folded items: Loop toggles; a readout or field shows its text.
fn more_menu(
    folded: &[Piece],
    state: &BarState,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let mut list = div()
        .id("more-menu")
        .occlude()
        .flex()
        .flex_col()
        .min_w(px(220.))
        .py_1()
        .bg(rgb(colors::LIST))
        .border_1()
        .border_color(rgb(colors::BORDER))
        .rounded_md()
        .shadow_md()
        .text_color(rgb(colors::TEXT));
    let mut row: usize = 0;
    for piece in folded.iter().copied() {
        if piece == Piece::Panes {
            // Swift's submenu, flattened: one toggle per pane.
            for (id, side, _) in PANE_TOGGLES {
                let shown = state.panes[side as usize];
                let title = panes::side_title(side);
                list = list.child(
                    div()
                        .id(("more-item", row))
                        .relative()
                        .flex()
                        .flex_row()
                        .gap_3()
                        .px_3()
                        .py_1()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(colors::BUTTON)))
                        .child(
                            div()
                                .flex_1()
                                .child(format!("{}{title}", if shown { "\u{2713} " } else { "" })),
                        )
                        .child(scope.control_selected("menuitem", title, true, None, shown))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.close_more(cx);
                            this.run_command(&RunCommand::new(id), window, cx);
                        })),
                );
                row += 1;
            }
            continue;
        }
        // Swift's entry (`M/TransportItemEntries.swift:96-143`): a text
        // shows its title and text; an action acts, a check action shows
        // ✓ while on.
        let r = &state.record;
        let detail = match piece {
            Piece::Loop => if state.looping { "On" } else { "Off" }.to_owned(),
            Piece::Readout(b) => state.readout.texts.text(b).to_owned(),
            Piece::Position | Piece::EditFields => piece
                .fields()
                .iter()
                .map(|f| state.field_text(*f))
                .collect::<Vec<_>>()
                .join(" \u{2013} "),
            Piece::MainView => panes::main_title(state.main).to_owned(),
            Piece::Cluster
            | Piece::Space
            | Piece::Panes
            | Piece::Faults
            | Piece::ReplaceOverdub
            | Piece::CountIn
            | Piece::Summary
            | Piece::Separator => String::new(),
        };
        // Faults is an action, enabled with faults or a report (Swift
        // `M/TransportItemEntries.swift:132-133`); it folds only then.
        // Replace/Overdub and Count-in are disabled while recording.
        let acts = match piece {
            Piece::Loop => true,
            Piece::Faults => state.faults.is_some(),
            Piece::ReplaceOverdub | Piece::CountIn => !r.recording,
            _ => false,
        };
        let checked = match piece {
            Piece::ReplaceOverdub => r.overdub,
            Piece::CountIn => r.count_in,
            _ => false,
        };
        let check = matches!(piece, Piece::ReplaceOverdub | Piece::CountIn);
        // The summary is one text, `Step Entry: <summary or "off">`.
        let text = (piece == Piece::Summary).then(|| {
            format!(
                "{}: {}",
                piece.title(),
                state.summary.as_deref().unwrap_or("off")
            )
        });
        let shown = match (&text, check) {
            (Some(t), _) => t.clone(),
            (None, true) => format!(
                "{}{}",
                if checked { "\u{2713} " } else { "" },
                piece.title()
            ),
            (None, false) => piece.title().to_owned(),
        };
        let mut entry = div()
            .id(("more-item", row))
            .relative()
            .flex()
            .flex_row()
            .gap_3()
            .px_3()
            .py_1()
            .when(check && !acts, |e| e.text_color(rgb(colors::DISABLED)))
            .child(div().flex_1().child(shown))
            .child(div().text_color(rgb(colors::DIM)).child(detail))
            .child(scope.control_selected(
                "menuitem",
                piece.title(),
                acts,
                text.as_deref(),
                checked,
            ));
        if acts {
            entry = entry
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors::BUTTON)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.close_more(cx);
                    match piece {
                        Piece::Faults => this.toggle_faults(cx),
                        Piece::ReplaceOverdub => this.press_record_mode(cx),
                        Piece::CountIn => this.toggle_count_in(cx),
                        _ => this.toggle_loop(cx),
                    }
                }));
        }
        list = list.child(entry);
        row += 1;
    }
    list.into_any_element()
}

/// The bar: `plan`'s shown pieces, then » when anything folds.
pub fn render(
    state: &BarState,
    plan: &Plan,
    fields: &TransportFields,
    more_open: bool,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> impl IntoElement {
    // Drawn again below, while the cluster shows.
    fields.record_button.set(None);
    let mut row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(GAP))
        .p(px(PADDING))
        .bg(rgb(colors::BAR))
        .overflow_hidden();
    for piece in plan.shown() {
        let child = match piece {
            Piece::Cluster => cluster(state, fields, scope, cx),
            Piece::Loop => loop_button(state.looping, scope, cx),
            Piece::Readout(b) => readout(b, state.readout.texts.text(b), scope),
            Piece::Position | Piece::EditFields => {
                let mut group = div().flex().flex_row().flex_none().gap(px(GAP));
                for f in piece.fields() {
                    group = group.child(field(*f, state, fields, scope, window, cx));
                }
                group.into_any_element()
            }
            Piece::Space => div().flex_1().into_any_element(),
            Piece::MainView => main_view(state.main, &mut scope.container(), cx),
            Piece::Panes => pane_toggles(state.panes, scope, cx),
            Piece::Faults => match &state.faults {
                Some(f) => faults_button(f, scope, window, cx),
                None => continue,
            },
            Piece::ReplaceOverdub => replace_overdub(&state.record, scope, cx),
            Piece::CountIn => count_in(&state.record, scope, cx),
            Piece::Summary => match &state.summary {
                Some(t) => summary(t, scope),
                None => continue,
            },
            Piece::Separator => separator(),
        };
        row = row.child(child);
    }
    let folded = plan.folded();
    if !folded.is_empty() {
        let mut more = more_button(more_open, &folded, state, scope, window, cx);
        // Folded, the popover opens at ».
        if let Some(f) = state.faults.as_ref().filter(|f| f.open)
            && folded.contains(&Piece::Faults)
        {
            more = div()
                .relative()
                .flex_none()
                .child(more)
                .child(faults_popover(f, scope, window, cx))
                .into_any_element();
        }
        row = row.child(div().flex_1()).child(more);
    }
    row
}

/// The Faults button (Swift `FaultsItem`): the error tint with faults,
/// else the warning tint (theme tokens); a click toggles the popover.
/// The tree's value names the tint.
fn faults_button(
    state: &FaultsState,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let tokens = crate::strip_palette::tokens();
    let (tint, value) = if state.error {
        (tokens.state_error, "error")
    } else {
        (tokens.state_warning, "warning")
    };
    let button = face("faults", "\u{26a0}", true, colors::BUTTON)
        .w(px(CLUSTER_BUTTON))
        .text_color(rgb(tint))
        .child(scope.control_with("button", FAULTS, true, Some(value), false))
        .on_click(cx.listener(|this, _, _, cx| this.toggle_faults(cx)));
    let button = crate::sheet_host::with_help(button, FAULTS_HELP.to_owned());
    let mut item = div().relative().flex_none().child(button);
    if state.open {
        item = item.child(faults_popover(state, scope, window, cx));
    }
    item.into_any_element()
}

/// The Faults popover: one line per message, under its opener; a press
/// outside it closes it (an in-window overlay, as the » menu).
fn faults_popover(
    state: &FaultsState,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let size = window.viewport_size();
    let mut scope = scope.container();
    let mut list = div()
        .id("faults-popover")
        .occlude()
        .flex()
        .flex_col()
        .gap_1()
        .p_4()
        .min_w(px(FAULTS_MIN_WIDTH))
        .max_w(px(3. * FAULTS_MIN_WIDTH))
        .bg(rgb(colors::LIST))
        .border_1()
        .border_color(rgb(colors::BORDER))
        .rounded_md()
        .shadow_md()
        .text_color(rgb(colors::TEXT))
        .child(scope.this("group", FAULTS, true));
    for line in &state.lines {
        list = list.child(
            div()
                .relative()
                .child(line.clone())
                .child(scope.control("text", line, true)),
        );
    }
    // Over its opener, so the list hangs from the opener's bottom edge.
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .id("faults-backdrop")
                        .w(size.width)
                        .h(size.height)
                        .occlude()
                        .on_any_mouse_down(cx.listener(|this, _, _, cx| this.close_faults(cx))),
                ),
            )
            .with_priority(1),
        )
        .child(
            div()
                .absolute()
                .top_full()
                .right_0()
                .child(deferred(anchored().snap_to_window().child(list)).with_priority(2)),
        )
        .into_any_element()
}
