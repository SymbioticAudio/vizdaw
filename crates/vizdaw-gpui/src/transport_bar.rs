//! The transport bar, drawn from the app's [`TransportLayout`] (Swift:
//! `TransportRowView`, `TransportItemViews`, `TransportReadoutsView`,
//! `TransportFieldsView`): the cluster (Return to Start, Rewind, Fast
//! Forward, Play/Stop), Loop, the four readouts, the Position and Edit
//! In/Out and Punch In/Out fields, flexible space, the Main View picker
//! and the pane toggles, Faults with its popover (document window only),
//! and the record items: Record in the cluster, Replace/Overdub,
//! Count-in, Record Options with its popover ([`crate::record_popover`])
//! and the Step Entry summary; Click with its Level/Mode menu, DSP Load,
//! Audio Device and command buttons; what does not fit folds, last first,
//! into the » menu, where a folded field item is a popover of its live
//! fields.

use crate::appearance::ThemedCorners as _;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, Div, ElementId, Focusable, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, Rgba, Stateful, StatefulInteractiveElement, Styled, TextRun,
    Window, anchored, canvas, deferred, div, point, px, rgb,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::bar_items::{self, CommandButtonFace, EntryKind, MenuEntry};
use vizdaw_app::editor::{MainView, PaneSide};
use vizdaw_app::transport::{ReadoutBox, TransportReadout};
use vizdaw_app::transport_bar::{Fit, ItemKind, TransportItem, TransportLayout, fit};
use vizdaw_app::transport_fields::TransportField;

use crate::control;
use crate::menus::{PlayStop, RunCommand};
use crate::panes;
use crate::record_popover::{self, PopoverState, RecordPopover};
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
    /// The record popover, while it is open in this window.
    pub record_popover: Option<PopoverState>,
    /// The Click item.
    pub click: ClickState,
    /// DSP Load's text (`DSP 12%`), as the poll last read it.
    pub dsp: String,
    /// Audio Device's text.
    pub device: String,
    /// The command buttons, in bar order: each one's id and face.
    pub commands: Vec<(String, CommandButtonFace)>,
}

/// What the Click item draws (Swift `ClickItemView`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickState {
    /// The session has given the settings (else both parts are off).
    pub known: bool,
    pub on: bool,
    /// Its own menu: Level › and Mode ›.
    pub menu: Vec<MenuEntry>,
    /// Its » entry: the submenu `Click`.
    pub entry: MenuEntry,
}

impl ClickState {
    pub fn of(d: &DocumentModel) -> ClickState {
        ClickState {
            known: d.last_transport().click.is_some(),
            on: bar_items::click_is_on(d),
            menu: bar_items::click_menu(d),
            entry: bar_items::click_entry(d),
        }
    }
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
            ..Shows::default()
        }
    }

    fn field_text(&self, f: TransportField) -> &str {
        self.field_texts
            .iter()
            .find(|(g, _)| *g == f)
            .map_or("", |(_, t)| t.as_str())
    }
}

/// The chrome's colours (0xRRGGBB), each read from the current theme when
/// asked, never kept: a theme change redraws every window and each draw
/// asks again. The Swift bar and sheets use the system's controls tinted
/// with the theme's text, accent and state colours; where GPUI draws its
/// own control, the nearest semantic token stands in.
pub mod colors {
    use crate::appearance::current;
    use vizdaw_app::theme::Theme;

    /// `base` with `over` laid on it at `alpha` (0.0 to 1.0).
    pub fn mix(base: u32, over: u32, alpha: f32) -> u32 {
        let ch = |shift: u32| {
            let b = ((base >> shift) & 0xff) as f32;
            let o = ((over >> shift) & 0xff) as f32;
            (b + (o - b) * alpha).round() as u32
        };
        (ch(16) << 16) | (ch(8) << 8) | ch(0)
    }

    /// The bar: Swift's `.bar` material is `surface.panel`.
    pub fn bar() -> u32 {
        current().surface.panel.rgb
    }
    /// A button's face.
    pub fn button() -> u32 {
        current().surface.raised.rgb
    }
    /// The play button while playing (`state.play`).
    pub fn playing() -> u32 {
        current().state.play.rgb
    }
    /// A toggle that is on: the accent at 0.18 over the bar (Swift
    /// `TransportItemViews.swift:204`).
    pub fn on() -> u32 {
        on_of(current())
    }
    /// [`on`] for `t`.
    pub fn on_of(t: &Theme) -> u32 {
        mix(t.surface.panel.rgb, t.accent.accent.rgb, 0.18)
    }
    /// The time and tempo readouts' background (`readout.background`).
    pub fn readout() -> u32 {
        current().readout.background.rgb
    }
    /// The readouts' label: `readout.text` at 0.72 over `readout.background`
    /// (Swift `TransportReadouts.swift:119-121`).
    pub fn readout_label() -> u32 {
        let t = current();
        mix(t.readout.background.rgb, t.readout.text.rgb, 0.72)
    }
    /// The readouts' text (`readout.text`).
    pub fn readout_text() -> u32 {
        current().readout.text.rgb
    }
    /// A text field's face (`surface.inset`).
    pub fn field() -> u32 {
        current().surface.inset.rgb
    }
    /// Text typed into a field.
    pub fn field_text() -> u32 {
        current().text.primary.rgb
    }
    /// Text (`text.primary`).
    pub fn text() -> u32 {
        current().text.primary.rgb
    }
    /// Disabled text (`text.disabled`).
    pub fn disabled() -> u32 {
        current().text.disabled.rgb
    }
    /// Secondary text (`text.secondary`).
    pub fn dim() -> u32 {
        current().text.secondary.rgb
    }
    /// A popover or menu list (`surface.inset`).
    pub fn list() -> u32 {
        current().surface.inset.rgb
    }
    /// A hairline (`surface.separator`).
    pub fn border() -> u32 {
        current().surface.separator.rgb
    }
    /// A document window's background (`surface.window`).
    pub fn window() -> u32 {
        current().surface.window.rgb
    }
    /// The accent (`accent.accent`) and the text on it.
    pub fn accent() -> u32 {
        current().accent.accent.rgb
    }
    pub fn on_accent() -> u32 {
        current().accent.on_accent.rgb
    }
    /// Warning and error text (`state.warning`, `state.error`).
    pub fn warning() -> u32 {
        current().state.warning.rgb
    }
    pub fn error() -> u32 {
        current().state.error.rgb
    }
    /// Text on a state-coloured fill.
    pub fn inverse() -> u32 {
        current().text.inverse.rgb
    }
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

/// The Click toggle's and its ⌄'s widths.
const CLICK_TOGGLE: f32 = 52.;
const CLICK_CHEVRON: f32 = 20.;
/// A command button's padding, each side (`px_3`).
const COMMAND_PADDING: f32 = 12.;
/// The widths DSP Load, Audio Device and a command button take in a bar
/// planned with no window.
const DSP_WIDTH: f32 = 64.;
const DEVICE_WIDTH: f32 = 120.;
const COMMAND_WIDTH: f32 = 96.;

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
    PunchFields,
    Space,
    MainView,
    Panes,
    Faults,
    ReplaceOverdub,
    CountIn,
    Summary,
    RecordOptions,
    Separator,
    Click,
    DspLoad,
    Device,
    /// The bar's `n`th command button (0 the first).
    Command(usize),
}

/// What shows only now and then: an item that is absent takes no room
/// (Swift: a zero-width item is neither shown nor folded). With it, the
/// widths measured from the texts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shows {
    /// Faults: there are faults or a report.
    pub faults: bool,
    /// The Step Entry summary: step entry is on.
    pub summary: bool,
    /// The width the cluster's readouts after Record take (count-in, the
    /// pass, the time left), gaps included.
    pub cluster_readouts: f32,
    /// The fields' label widths.
    pub labels: LabelWidths,
    /// DSP Load's width (for `DSP 100%`) and Audio Device's, when
    /// measured.
    pub dsp: Option<f32>,
    pub device: Option<f32>,
    /// Each command button's width, in bar order, when measured.
    pub commands: Vec<f32>,
}

impl Shows {
    /// What `bar` shows, with its texts measured in `window`'s font.
    pub fn measured(bar: Option<&BarState>, window: &Window) -> Shows {
        let caption = window.rem_size() * 0.75;
        let body = window.rem_size() * 0.875;
        let mut shows = bar.map(BarState::shows).unwrap_or_default();
        shows.labels = LabelWidths::measure(window);
        shows.dsp = Some(text_width(window, &bar_items::dsp_text(1.), caption));
        if let Some(b) = bar {
            shows.device = Some(text_width(window, &b.device, caption));
            shows.commands = b
                .commands
                .iter()
                .map(|(_, f)| text_width(window, &f.label, body) + 2. * COMMAND_PADDING)
                .collect();
        }
        shows
    }
}

/// `text`'s width at `size` in `window`'s font, rounded up.
fn text_width(window: &Window, text: &str, size: gpui::Pixels) -> f32 {
    let run = TextRun {
        len: text.len(),
        font: window.text_style().font(),
        color: gpui::Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(text.to_owned().into(), size, &[run], None);
    f32::from(line.width).ceil()
}

/// The fields' label widths, measured from their titles at the caption
/// size (`text_xs`) so none clips; the default is a guess for a bar
/// planned without a window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelWidths([f32; 5]);

impl Default for LabelWidths {
    fn default() -> Self {
        LabelWidths([52., 44., 52., 56., 64.])
    }
}

impl LabelWidths {
    /// The labels' widths in `window`'s font.
    pub fn measure(window: &Window) -> LabelWidths {
        let size = window.rem_size() * 0.75;
        LabelWidths(TransportField::ALL.map(|f| text_width(window, f.title(), size)))
    }

    /// `f`'s label width.
    pub fn of(&self, f: TransportField) -> f32 {
        let i = TransportField::ALL
            .iter()
            .position(|g| *g == f)
            .expect("every field");
        self.0[i]
    }
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
/// Record Options' help (`V/RecordControls.swift:75`).
pub const RECORD_OPTIONS_HELP: &str =
    "Replace or overdub, punch, loop, count-in, pre-roll and MIDI takes";

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
            TransportItem::PunchFields => Piece::PunchFields,
            TransportItem::Space => Piece::Space,
            TransportItem::MainView => Piece::MainView,
            TransportItem::Panes => Piece::Panes,
            TransportItem::Faults => Piece::Faults,
            TransportItem::ReplaceOverdub => Piece::ReplaceOverdub,
            TransportItem::CountIn => Piece::CountIn,
            TransportItem::Summary => Piece::Summary,
            TransportItem::RecordOptions => Piece::RecordOptions,
            TransportItem::Separator => Piece::Separator,
            TransportItem::Click => Piece::Click,
            TransportItem::DspLoad => Piece::DspLoad,
            TransportItem::Device => Piece::Device,
            // Numbered by the plan.
            TransportItem::Command { .. } => Piece::Command(0),
        })
    }

    /// The width it takes, with the fields' labels `labels` wide. Fixed,
    /// so the readouts fit their widest texts ("9999.4.960",
    /// "999:59.999", "999.99", "255/128") and the bar folds the same way
    /// whatever is playing.
    pub fn width(self, labels: &LabelWidths) -> f32 {
        match self {
            // Return to Start, Rewind, Fast Forward, Play/Stop and Record.
            Piece::Cluster => 5. * CLUSTER_BUTTON + 4. * CLUSTER_GAP,
            Piece::Loop => LOOP,
            Piece::Readout(b) => readout_width(b),
            Piece::Position | Piece::EditFields | Piece::PunchFields => {
                let fields = self.fields();
                fields
                    .iter()
                    .map(|f| labels.of(*f) + LABEL_GAP + FIELD)
                    .sum::<f32>()
                    + GAP * (fields.len() - 1) as f32
            }
            Piece::Space => 0.,
            Piece::MainView => 2. * SEGMENT + 1.,
            Piece::Panes => 3. * CLUSTER_BUTTON + 2. * CLUSTER_GAP,
            Piece::Faults => CLUSTER_BUTTON,
            Piece::ReplaceOverdub => MODE,
            Piece::CountIn => COUNT_IN,
            Piece::Summary => SUMMARY,
            Piece::RecordOptions => CHEVRON,
            Piece::Separator => 1.,
            Piece::Click => CLICK_TOGGLE + CLUSTER_GAP + CLICK_CHEVRON,
            Piece::DspLoad => DSP_WIDTH,
            Piece::Device => DEVICE_WIDTH,
            Piece::Command(_) => COMMAND_WIDTH,
        }
    }

    /// Its width in a bar that `shows`: the measured one where there is.
    fn width_in(self, shows: &Shows) -> f32 {
        match self {
            Piece::Cluster => self.width(&shows.labels) + shows.cluster_readouts,
            Piece::DspLoad => shows.dsp.unwrap_or(DSP_WIDTH),
            Piece::Device => shows.device.unwrap_or(DEVICE_WIDTH),
            Piece::Command(n) => shows.commands.get(n).copied().unwrap_or(COMMAND_WIDTH),
            _ => self.width(&shows.labels),
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
            Piece::PunchFields => "Punch In and Out",
            Piece::Space => "Flexible Space",
            Piece::MainView => MAIN_VIEW,
            Piece::Panes => "Pane Toggles",
            Piece::Faults => FAULTS,
            Piece::ReplaceOverdub => "Replace/Overdub",
            Piece::CountIn => "Count-in",
            Piece::Summary => "Step Entry",
            Piece::RecordOptions => record_popover::RECORD_OPTIONS,
            Piece::Separator => "Separator",
            Piece::Click => bar_items::CLICK,
            Piece::DspLoad => bar_items::DSP_LOAD,
            Piece::Device => bar_items::AUDIO_DEVICE,
            Piece::Command(_) => bar_items::COMMAND_BUTTON,
        }
    }

    /// The fields it shows, in bar order.
    pub fn fields(self) -> &'static [TransportField] {
        match self {
            Piece::Position => &[TransportField::Position],
            Piece::EditFields => &[TransportField::EditIn, TransportField::EditOut],
            Piece::PunchFields => &[TransportField::PunchIn, TransportField::PunchOut],
            _ => &[],
        }
    }

    /// The bar item that holds field `f`.
    pub fn holding(f: TransportField) -> Piece {
        match f {
            TransportField::Position => Piece::Position,
            TransportField::EditIn | TransportField::EditOut => Piece::EditFields,
            TransportField::PunchIn | TransportField::PunchOut => Piece::PunchFields,
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

/// Which pieces a bar of `width` shows and which fold into ».
pub struct Plan {
    pieces: Vec<Piece>,
    /// The item each piece draws.
    items: Vec<TransportItem>,
    fit: Fit,
    labels: LabelWidths,
    /// DSP Load's and Audio Device's widths.
    widths: (f32, f32),
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
        let mut commands = 0;
        let (pieces, items): (Vec<Piece>, Vec<TransportItem>) = layout
            .items_for(document_window)
            .into_iter()
            .filter_map(|i| Piece::of(i).map(|p| (p, i.clone())))
            .map(|(p, i)| match p {
                Piece::Command(_) => {
                    commands += 1;
                    (Piece::Command(commands - 1), i)
                }
                _ => (p, i),
            })
            .filter(|(p, _)| match p {
                Piece::Faults => shows.faults,
                Piece::Summary => shows.summary,
                _ => true,
            })
            .unzip();
        let widths: Vec<(f32, bool, ItemKind)> = pieces
            .iter()
            .map(|p| {
                let kind = match p {
                    Piece::Space => ItemKind::Space,
                    Piece::Separator => ItemKind::Separator,
                    _ => ItemKind::Content,
                };
                (p.width_in(&shows), *p == Piece::Cluster, kind)
            })
            .collect();
        let fit = fit(&widths, width - 2. * PADDING, GAP, CHEVRON);
        Plan {
            pieces,
            items,
            fit,
            labels: shows.labels,
            widths: (
                Piece::DspLoad.width_in(&shows),
                Piece::Device.width_in(&shows),
            ),
        }
    }

    /// The bar's items, shown and folded, in bar order.
    pub fn items(&self) -> &[TransportItem] {
        &self.items
    }

    /// The items folded into ».
    pub fn folded_items(&self) -> Vec<TransportItem> {
        self.fit
            .folded
            .iter()
            .map(|&i| self.items[i].clone())
            .collect()
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
        colors::text()
    } else {
        colors::disabled()
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
pub(crate) fn face(
    id: impl Into<ElementId>,
    text: &str,
    enabled: bool,
    background: u32,
) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .h(px(28.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .round_md()
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
    cluster_button(id, glyph, label, enabled, colors::button(), scope).when(enabled, |b| {
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
        colors::button(),
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
        colors::button(),
        scope,
    )
    // Swift's Play is an untinted system button: only its glyph takes
    // `state.play` while playing.
    .when(playing && state.can_play, |b| {
        b.text_color(rgb(colors::playing()))
    })
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
    let mut button = face("record", "\u{25cf}", enabled, colors::button())
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
        // Swift tints the bezel's glyph (`RecordControls.swift:27`), no fill.
        button = button.text_color(rgb(tokens.state_record));
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
                .text_color(rgb(colors::text()))
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
            .text_color(rgb(colors::dim()))
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
            colors::on()
        } else {
            colors::button()
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
            colors::on()
        } else {
            colors::button()
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
        .round_sm()
        .bg(Rgba { r, g, b, a: 0.18 })
        .text_xs()
        .text_color(rgb(colors::text()))
        .child(text.to_owned())
        .child(scope.control("text", text, true))
        .into_any_element()
}

/// Click (Swift `ClickItemView`): the metronome's toggle and its ⌄. A
/// right-click on either, or the ⌄, opens Level › and Mode ›; both parts
/// are off while the session has not given the settings. Under the
/// channel the menu does not open (it would track modally): the status
/// line names the commands.
fn click_item(
    c: &ClickState,
    open: Option<&[usize]>,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let enabled = c.known;
    let toggle =
        face(
            "click-toggle",
            bar_items::CLICK,
            enabled,
            if c.on { colors::on() } else { colors::button() },
        )
        .w(px(CLICK_TOGGLE))
        .child(scope.control_with(
            "button",
            bar_items::CLICK,
            enabled,
            Some(if c.on { "on" } else { "off" }),
            false,
        ))
        .when(enabled, |b| {
            b.on_click(cx.listener(|this, _, _, cx| {
                this.run_bar_action(bar_items::BarAction::ToggleClick, cx)
            }))
        })
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, _, _, cx| this.open_click_menu(cx)),
        );
    let toggle = crate::sheet_host::with_help(toggle, bar_items::CLICK_HELP.to_owned());
    let options = face("click-options", "\u{2304}", enabled, colors::button())
        .w(px(CLICK_CHEVRON))
        .child(scope.control("button", bar_items::CLICK_OPTIONS, enabled))
        .when(enabled, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.toggle_click_menu(cx)))
        })
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, _, _, cx| this.open_click_menu(cx)),
        );
    let options = crate::sheet_host::with_help(options, bar_items::CLICK_OPTIONS_HELP.to_owned());
    let mut item = div()
        .relative()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .gap(px(CLUSTER_GAP))
        .child(toggle)
        .child(options);
    if let Some(path) = open {
        let size = window.viewport_size();
        let list = bar_menu_list(
            &c.menu,
            path,
            0,
            MenuOwner::Click,
            &mut scope.container(),
            cx,
        );
        item = item
            .child(
                deferred(
                    anchored().position(point(px(0.), px(0.))).child(
                        div()
                            .id("click-menu-backdrop")
                            .w(size.width)
                            .h(size.height)
                            .occlude()
                            .on_any_mouse_down(
                                cx.listener(|this, _, _, cx| this.close_bar_menus(cx)),
                            ),
                    ),
                )
                .with_priority(1),
            )
            .child(
                div()
                    .absolute()
                    .top_full()
                    .left_0()
                    .child(deferred(anchored().snap_to_window().child(list)).with_priority(2)),
            );
    }
    item.into_any_element()
}

/// Whose menu a bar menu list is: Click's own, or Click's in ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOwner {
    Click,
    More,
}

/// A bar item's menu (Swift's `TransportMenuEntry`s): an action shows ✓
/// while on and acts, closing the menus; a submenu opens on a click, its
/// list beside it. `path` holds the open submenu's row at each depth.
fn bar_menu_list(
    entries: &[MenuEntry],
    path: &[usize],
    depth: usize,
    owner: MenuOwner,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> Stateful<Div> {
    let mut list = div()
        .id(("bar-menu", depth))
        .occlude()
        .flex()
        .flex_col()
        .min_w(px(160.))
        .py_1()
        .bg(rgb(colors::list()))
        .border_1()
        .border_color(rgb(colors::border()))
        .round_md()
        .shadow_md()
        .text_color(rgb(colors::text()));
    for (row, e) in entries.iter().enumerate() {
        let mut el = div()
            .id(("bar-menu-item", depth * 1000 + row))
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .child(
                div()
                    .w(px(12.))
                    .child(if e.checked { "\u{2713}" } else { "" }),
            )
            .child(div().flex_1().whitespace_nowrap().child(e.title.clone()))
            .child(scope.control_selected("menuitem", &e.title, e.enabled, None, e.checked));
        el = if e.enabled {
            el.cursor_pointer().hover(|s| s.bg(rgb(colors::button())))
        } else {
            el.text_color(rgb(colors::disabled()))
        };
        match &e.kind {
            EntryKind::Action(action) => {
                if e.enabled {
                    let action = *action;
                    el = el.on_click(cx.listener(move |this, _, _, cx| {
                        this.close_bar_menus(cx);
                        this.run_bar_action(action, cx);
                    }));
                }
            }
            EntryKind::Submenu(items) => {
                el = el.child(div().text_color(rgb(colors::dim())).child("\u{25B8}"));
                if e.enabled {
                    el = el.on_click(cx.listener(move |this, _, _, cx| {
                        this.open_bar_submenu(owner, depth, row, cx)
                    }));
                    if path.first() == Some(&row) {
                        let nested = bar_menu_list(items, &path[1..], depth + 1, owner, scope, cx);
                        el = el.child(side_list(owner, nested, depth));
                    }
                }
            }
        }
        list = list.child(el);
    }
    list
}

/// A submenu's list beside its row: to the right of Click's own menu, to
/// the left in » (it hangs from the window's right edge).
fn side_list(owner: MenuOwner, list: Stateful<Div>, depth: usize) -> AnyElement {
    let at = div().absolute().top_0();
    let at = match owner {
        MenuOwner::Click => at.left_full(),
        MenuOwner::More => at.right_full(),
    };
    at.child(deferred(anchored().snap_to_window().child(list)).with_priority(3 + depth))
        .into_any_element()
}

/// DSP Load or Audio Device (Swift `DspLoadView`, `DeviceItem`): a
/// caption text `width` wide (DSP Load's fits `DSP 100%`, so its digits
/// never move the row).
fn caption_item(label: &str, text: &str, width: f32, scope: &mut RegistryScope) -> AnyElement {
    div()
        .relative()
        .flex_none()
        .w(px(width))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_xs()
        .text_color(rgb(colors::dim()))
        .child(text.to_owned())
        .child(scope.control_with("text", label, true, Some(text), false))
        .into_any_element()
}

/// A command button (Swift `CommandButtonItem`): its face's label, its
/// tooltip (in the tree as its value) and accessibility label; enabled as
/// its menu item, a click runs what that item runs.
fn command_button(
    n: usize,
    id: &str,
    f: &CommandButtonFace,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let id = id.to_owned();
    let button = face(("command-button", n), &f.label, f.enabled, colors::button())
        .px(px(COMMAND_PADDING))
        .whitespace_nowrap()
        .child(scope.control_with(
            "button",
            &f.accessibility_label,
            f.enabled,
            Some(&f.tooltip),
            false,
        ))
        .when(f.enabled, |b| {
            b.on_click(cx.listener(move |this, _, window, cx| this.press_command(&id, window, cx)))
        });
    crate::sheet_host::with_help(button, f.tooltip.clone()).into_any_element()
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
        if on { colors::on() } else { colors::button() },
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
        .round_md()
        .bg(rgb(colors::readout()))
        .overflow_hidden()
        .whitespace_nowrap()
        .child(
            div()
                .text_xs()
                .text_color(rgb(colors::readout_label()))
                .child(which.title()),
        )
        .child(
            div()
                .text_color(rgb(colors::readout_text()))
                .child(text.to_owned()),
        )
        .child(scope.control_with("text", which.accessibility_label(), true, Some(text), false))
        .into_any_element()
}

/// A field with its label: the model's text until a click (or its key)
/// starts an edit, then the text input.
fn field(
    f: TransportField,
    state: &BarState,
    fields: &TransportFields,
    labels: &LabelWidths,
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
        .round_sm()
        .overflow_hidden()
        .text_color(rgb(colors::field_text()));
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
            .bg(rgb(colors::field()))
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
                .w(px(labels.of(f)))
                .flex_none()
                .text_xs()
                .text_color(rgb(colors::dim()))
                .whitespace_nowrap()
                .child(f.title()),
        )
        .child(boxed)
        .into_any_element()
}

/// A field item's fields, side by side.
fn field_group(
    piece: Piece,
    state: &BarState,
    fields: &TransportFields,
    labels: &LabelWidths,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let mut group = div().flex().flex_row().flex_none().gap(px(GAP));
    for f in piece.fields() {
        group = group.child(field(*f, state, fields, labels, scope, window, cx));
    }
    group.into_any_element()
}

/// A folded field item's popover under » (Swift's `.popover` entry): its
/// live fields, edited as on the bar. A press outside it closes it
/// (cancelling an edit), apart from one on » (its opener toggles it) and
/// one on Record with a punch draft open.
fn field_popover(
    piece: Piece,
    state: &BarState,
    fields: &TransportFields,
    labels: &LabelWidths,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let mut scope = scope.container();
    let panel = div()
        .id("field-popover")
        .occlude()
        .relative()
        .p_3()
        .bg(rgb(colors::list()))
        .border_1()
        .border_color(rgb(colors::border()))
        .round_md()
        .shadow_md()
        .text_color(rgb(colors::text()))
        .child(scope.this("group", piece.title(), true))
        .child(field_group(
            piece, state, fields, labels, &mut scope, window, cx,
        ))
        .on_mouse_down_out(cx.listener(|this, e: &MouseDownEvent, window, cx| {
            this.press_outside_field_popover(e.position, window, cx)
        }));
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(
            div()
                .absolute()
                .top_full()
                .right_0()
                .child(deferred(anchored().snap_to_window().child(panel)).with_priority(2)),
        )
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
                if on { colors::on() } else { colors::button() },
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
                if shown {
                    colors::on()
                } else {
                    colors::button()
                },
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

/// » (`open`: its menu is open, `more_click`: so is Click's submenu in it).
fn more_button(
    (open, more_click): (bool, Option<&[usize]>),
    folded: &[Piece],
    state: &BarState,
    fields: &TransportFields,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let bounds = fields.more_button.clone();
    let mut chevron = face("more", "\u{bb}", true, colors::button())
        .w(px(CHEVRON))
        .child(scope.control("button", MORE, true))
        // Where it is, so a field popover knows a press on its opener.
        .child(
            canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {})
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
        )
        .on_click(cx.listener(|this, _, window, cx| {
            // Its opener closes a field popover.
            if this.close_field_popover(window, cx) {
                return;
            }
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
                        more_click,
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
    more_click: Option<&[usize]>,
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
        .bg(rgb(colors::list()))
        .border_1()
        .border_color(rgb(colors::border()))
        .round_md()
        .shadow_md()
        .text_color(rgb(colors::text()));
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
                        .hover(|s| s.bg(rgb(colors::button())))
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
        if piece == Piece::Click {
            // Swift's submenu `Click` ✓: the toggle, Level › and Mode ›.
            let e = &state.click.entry;
            let mut entry = div()
                .id(("more-item", row))
                .relative()
                .flex()
                .flex_row()
                .gap_3()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors::button())))
                .child(div().flex_1().child(format!(
                    "{}{}",
                    if e.checked { "\u{2713} " } else { "" },
                    e.title
                )))
                .child(div().text_color(rgb(colors::dim())).child("\u{25B8}"))
                .child(scope.control_selected("menuitem", &e.title, true, None, e.checked))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_more_click(cx)));
            if let (Some(path), EntryKind::Submenu(items)) = (more_click, &e.kind) {
                let nested = bar_menu_list(items, path, 1, MenuOwner::More, scope, cx);
                entry = entry.child(side_list(MenuOwner::More, nested, 0));
            }
            list = list.child(entry);
            row += 1;
            continue;
        }
        if let Piece::Command(n) = piece
            && let Some((id, f)) = state.commands.get(n)
        {
            // An action with its face's label, greyed as the button is.
            let id = id.clone();
            let mut entry = div()
                .id(("more-item", row))
                .relative()
                .flex()
                .flex_row()
                .gap_3()
                .px_3()
                .py_1()
                .child(div().flex_1().child(f.label.clone()))
                .child(scope.control_with("menuitem", &f.label, f.enabled, None, false));
            entry = if f.enabled {
                entry
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors::button())))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_more(cx);
                        this.press_command(&id, window, cx);
                    }))
            } else {
                entry.text_color(rgb(colors::disabled()))
            };
            list = list.child(entry);
            row += 1;
            continue;
        }
        // Swift's entry (`M/TransportItemEntries.swift:96-143`): a text
        // shows its title and text; an action acts, a check action shows
        // ✓ while on.
        let r = &state.record;
        let detail = match piece {
            Piece::Loop => if state.looping { "On" } else { "Off" }.to_owned(),
            Piece::Readout(b) => state.readout.texts.text(b).to_owned(),
            Piece::Position | Piece::EditFields | Piece::PunchFields => piece
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
            | Piece::RecordOptions
            | Piece::Separator
            | Piece::Click
            | Piece::DspLoad
            | Piece::Device
            | Piece::Command(_) => String::new(),
        };
        // Faults is an action, enabled with faults or a report (Swift
        // `M/TransportItemEntries.swift:132-133`); it folds only then.
        // Replace/Overdub and Count-in are disabled while recording.
        // Record Options is a popover entry: it opens the popover at ».
        let acts = match piece {
            Piece::Loop
            | Piece::RecordOptions
            | Piece::Position
            | Piece::EditFields
            | Piece::PunchFields => true,
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
        // The summary, DSP Load and Audio Device are one text each,
        // `<title>: <text>` (the summary's "off" when step entry is).
        let text = match piece {
            Piece::Summary => Some(state.summary.as_deref().unwrap_or("off")),
            Piece::DspLoad => Some(state.dsp.as_str()),
            Piece::Device => Some(state.device.as_str()),
            _ => None,
        }
        .map(|t| format!("{}: {t}", piece.title()));
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
            .when(check && !acts, |e| e.text_color(rgb(colors::disabled())))
            .child(div().flex_1().child(shown))
            .child(div().text_color(rgb(colors::dim())).child(detail))
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
                .hover(|s| s.bg(rgb(colors::button())))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.close_more(cx);
                    match piece {
                        Piece::Faults => this.toggle_faults(cx),
                        Piece::RecordOptions => this.toggle_record_popover(window, cx),
                        Piece::Position | Piece::EditFields | Piece::PunchFields => {
                            this.open_field_popover(piece, window, cx)
                        }
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

/// What the window keeps for its bar: the fields, the record popover's
/// fields, and whether » is open.
pub struct BarParts<'a> {
    pub fields: &'a TransportFields,
    pub popover: &'a RecordPopover,
    pub more_open: bool,
    /// Click's own menu, while open: its open submenus.
    pub click_menu: Option<&'a [usize]>,
    /// Click's submenu in », while open: its open submenus.
    pub more_click: Option<&'a [usize]>,
}

/// The bar: `plan`'s shown pieces, then » when anything folds.
pub fn render(
    state: &BarState,
    plan: &Plan,
    parts: BarParts,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> impl IntoElement {
    let BarParts {
        fields,
        popover,
        more_open,
        click_menu,
        more_click,
    } = parts;
    // Drawn again below, while the cluster and the chevrons show.
    fields.record_button.set(None);
    fields.more_button.set(None);
    popover.chevron.set(None);
    let mut row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(GAP))
        .p(px(PADDING))
        .bg(rgb(colors::bar()))
        .overflow_hidden();
    for piece in plan.shown() {
        let child = match piece {
            Piece::Cluster => cluster(state, fields, scope, cx),
            Piece::Loop => loop_button(state.looping, scope, cx),
            Piece::Readout(b) => readout(b, state.readout.texts.text(b), scope),
            Piece::Position | Piece::EditFields | Piece::PunchFields => {
                field_group(piece, state, fields, &plan.labels, scope, window, cx)
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
            Piece::RecordOptions => {
                record_popover::chevron(state.record_popover.as_ref(), popover, scope, window, cx)
            }
            Piece::Separator => separator(),
            Piece::Click => click_item(&state.click, click_menu, scope, window, cx),
            Piece::DspLoad => caption_item(bar_items::DSP_LOAD, &state.dsp, plan.widths.0, scope),
            Piece::Device => {
                caption_item(bar_items::AUDIO_DEVICE, &state.device, plan.widths.1, scope)
            }
            Piece::Command(n) => match state.commands.get(n) {
                Some((id, f)) => command_button(n, id, f, scope, cx),
                None => continue,
            },
        };
        row = row.child(child);
    }
    let folded = plan.folded();
    if !folded.is_empty() {
        let mut more = more_button(
            (more_open, more_click),
            &folded,
            state,
            fields,
            scope,
            window,
            cx,
        );
        // A folded field item's popover, at ».
        if let Some(p) = fields.popover().filter(|p| folded.contains(p)) {
            more = div()
                .relative()
                .flex_none()
                .child(more)
                .child(field_popover(
                    p,
                    state,
                    fields,
                    &plan.labels,
                    scope,
                    window,
                    cx,
                ))
                .into_any_element();
        }
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
        if let Some(p) = state.record_popover.as_ref()
            && folded.contains(&Piece::RecordOptions)
        {
            more = div()
                .relative()
                .flex_none()
                .child(more)
                .child(record_popover::overlay(p, popover, true, scope, window, cx))
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
    let button = face("faults", "\u{26a0}", true, colors::button())
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
        .bg(rgb(colors::list()))
        .border_1()
        .border_color(rgb(colors::border()))
        .round_md()
        .shadow_md()
        .text_color(rgb(colors::text()))
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
