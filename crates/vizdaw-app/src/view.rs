//! The View menu's main views and panes (Swift `ViewCommands`,
//! `WindowLayout`): one implementation for the menu items, the keys and
//! the control channel's `ui.*` commands; `state.get layout`; and the
//! rule for the strip the left pane follows (Swift `StripFocus`). None
//! opens a dialog.

use core_control::registry::ArgValue;
use core_control::{ControlError, ErrorCode};
use vizdaw_ffi::control::{LayoutStateRow, PaneRow, ViewPanesRow};
use vizdaw_ffi::mixer::{DestRow, StripKindRow, StripRow};
use vizdaw_ffi::snapshot::LaneRow;
use vizdaw_ffi::tracks::TrackKindRow;

use crate::commands::{Ran, check_names, on_flag, ran};
use crate::document::DocumentModel;
use crate::editor::{
    DetailEditorKind, EditorLayout, MainView, PaneSide, PaneState, PaneView, off_list,
};

/// `ui.pane` from a key or the menu: it needs its arguments.
pub const SET_PANE_NEEDS: &str = "Set Pane needs `pane` and a `view`, `shown` or `size`.";

/// The main view `id` shows, for `ui.view_arrange` and `ui.view_mix`.
fn main_of(id: &str) -> Option<MainView> {
    match id {
        "ui.view_arrange" => Some(MainView::Arrange),
        "ui.view_mix" => Some(MainView::Mix),
        _ => None,
    }
}

/// The pane `id` toggles, for `ui.toggle_left|bottom|right`.
fn side_of(id: &str) -> Option<PaneSide> {
    match id {
        "ui.toggle_left" => Some(PaneSide::Left),
        "ui.toggle_bottom" => Some(PaneSide::Bottom),
        "ui.toggle_right" => Some(PaneSide::Right),
        _ => None,
    }
}

/// A view item's check mark is the current main view; a toggle's is
/// whether the current view shows that pane.
pub fn is_on(id: &str, doc: &DocumentModel) -> bool {
    let v = &doc.layout().views;
    if let Some(main) = main_of(id) {
        return v.main == main;
    }
    side_of(id).is_some_and(|side| v.current().pane(side).shown)
}

/// Runs a view command. A view item always shows its view (nothing
/// changes when it already shows); a toggle shows (`on` true), hides
/// (false) or flips (absent) its pane. Text edits in the panes end before
/// anything changes, and only when something does.
pub(crate) fn run(
    id: &str,
    args: &[(String, ArgValue)],
    doc: &mut DocumentModel,
) -> Result<Ran, ControlError> {
    if id == "ui.pane" {
        return run_pane(args, doc);
    }
    check_names(id, args)?;
    if let Some(main) = main_of(id) {
        if doc.layout().views.main != main {
            doc.end_pane_edits();
            doc.update_layout(|l| l.views.main = main);
        }
        return Ok(ran(doc));
    }
    let Some(side) = side_of(id) else {
        return Err(ControlError::new(
            ErrorCode::UnknownId,
            format!("There is no command {id}."),
        ));
    };
    let on = on_flag(id, args)?;
    let now = doc.layout().views.current().pane(side).shown;
    let shown = on.unwrap_or(!now);
    if shown != now {
        doc.end_pane_edits();
        doc.update_layout(|l| l.views.current_mut().pane_mut(side).shown = shown);
    }
    Ok(ran(doc))
}

fn invalid(message: impl Into<String>) -> ControlError {
    ControlError::new(ErrorCode::InvalidParams, message)
}

fn text<'a>(args: &'a [(String, ArgValue)], name: &str) -> Option<&'a ArgValue> {
    args.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

/// `ui.pane`: every argument is checked before anything changes; the
/// answer's status names what was set (not the status line).
fn run_pane(args: &[(String, ArgValue)], doc: &mut DocumentModel) -> Result<Ran, ControlError> {
    if args.is_empty() {
        return Err(ControlError::new(ErrorCode::Unavailable, SET_PANE_NEEDS));
    }
    check_names("ui.pane", args)?;
    let side = match text(args, "pane") {
        Some(ArgValue::Text(t)) => PaneSide::parse(t),
        _ => None,
    }
    .ok_or_else(|| invalid("ui.pane: `pane` is left, bottom or right"))?;
    let main = match text(args, "main") {
        None => doc.layout().views.main,
        Some(ArgValue::Text(t)) => {
            MainView::parse(t).ok_or_else(|| invalid("ui.pane: `main` is arrange or mix"))?
        }
        Some(_) => return Err(invalid("ui.pane: `main` is arrange or mix")),
    };
    let sent = match text(args, "view") {
        None => None,
        Some(ArgValue::Text(t)) => Some(t.as_str()),
        Some(_) => return Err(invalid("ui.pane: `view` is the name of a pane's view")),
    };
    let shown = match text(args, "shown") {
        None => None,
        Some(ArgValue::Flag(f)) => Some(*f),
        Some(_) => return Err(invalid("ui.pane: `shown` is true or false")),
    };
    let size = match text(args, "size") {
        None => None,
        Some(ArgValue::Int(i)) => Some(*i as f64),
        Some(_) => return Err(invalid("ui.pane: `size` is a whole number of points")),
    };
    if sent.is_none() && shown.is_none() && size.is_none() {
        return Err(invalid("ui.pane: give `view`, `shown` or `size`"));
    }
    let mut kind = None;
    let mut view = None;
    if let Some(sent) = sent {
        kind = DetailEditorKind::ALL.into_iter().find(|k| k.name() == sent);
        view = if kind.is_some() {
            Some(PaneView::Editor)
        } else {
            PaneView::parse(sent)
        };
        if !view.is_some_and(|v| side.views(main).contains(&v)) {
            return Err(invalid(off_list(sent, side, main)));
        }
    }
    let view = view.unwrap_or(doc.layout().views.setup(main).pane(side).view);
    // Checked above: `choose` refuses nothing here.
    let _ = choose(side, view, kind, shown, size, Some(main), doc);
    let l = doc.layout();
    let p = l.views.setup(main).pane(side);
    let status = format!(
        "{} {} {} {}",
        side.name(),
        view_name(p, l),
        p.size as i64,
        if p.shown { "shown" } else { "hidden" }
    );
    Ok(Ran {
        status: Some(status),
        ..ran(doc)
    })
}

/// The one path that changes a pane once its arguments are checked
/// (`ui.pane` and the pane's picker): text edits end, the pane is set,
/// then an editor kind opens the detail (or is remembered with no
/// target), and a given `shown` wins over the detail's showing the
/// bottom pane. A view off the pane's list is refused before anything
/// changes: the refusal is returned.
pub fn choose(
    side: PaneSide,
    view: PaneView,
    kind: Option<DetailEditorKind>,
    shown: Option<bool>,
    size: Option<f64>,
    main: Option<MainView>,
    doc: &mut DocumentModel,
) -> Result<(), String> {
    let setup = main.unwrap_or(doc.layout().views.main);
    if !side.views(setup).contains(&view) {
        return Err(off_list(view.name(), side, setup));
    }
    doc.end_pane_edits();
    let mut set = Ok(());
    doc.update_layout(|l| set = l.views.set_pane(side, Some(view), shown, size, setup));
    set?;
    let Some(kind) = kind else {
        return Ok(());
    };
    let target = doc
        .layout()
        .detail
        .as_ref()
        .map(|d| d.target)
        .or_else(|| doc.detail_target());
    match target {
        Some(t) => doc.open_detail_in(t, Some(kind), setup),
        None => doc.update_layout(|l| l.last_kind = kind),
    }
    if let Some(on) = shown {
        doc.update_layout(|l| l.views.setup_mut(setup).bottom.shown = on);
    }
    Ok(())
}

/// A pane's view as the channel names it: the bottom pane's editor is its
/// kind (`piano_roll` …).
pub fn view_name(p: &PaneState, l: &EditorLayout) -> &'static str {
    if p.view != PaneView::Editor {
        return p.view.name();
    }
    l.detail.as_ref().map_or(l.last_kind, |d| d.kind).name()
}

/// `state.get layout`: both main views' panes, the strip the left-strip
/// rule picks, and what the current view's left pane shows (`strip`,
/// `midi`, `inspector`, `nothing`, `hidden` or its view's name).
pub fn layout_row(doc: &DocumentModel, strips: &[StripRow], dests: &[DestRow]) -> LayoutStateRow {
    let l = doc.layout();
    let views = MainView::ALL
        .into_iter()
        .map(|main| ViewPanesRow {
            main: main.name().into(),
            panes: PaneSide::ALL
                .into_iter()
                .map(|side| {
                    let p = l.views.setup(main).pane(side);
                    PaneRow {
                        pane: side.name().into(),
                        shown: p.shown,
                        view: view_name(p, l).into(),
                        size: p.size,
                        views: side
                            .views(main)
                            .iter()
                            .flat_map(|v| match v {
                                PaneView::Editor => DetailEditorKind::ALL
                                    .iter()
                                    .map(|k| k.name().to_owned())
                                    .collect::<Vec<_>>(),
                                v => vec![v.name().to_owned()],
                            })
                            .collect(),
                    }
                })
                .collect(),
        })
        .collect();
    let rule = doc.left_strip(strips, dests);
    let left = l.views.current().left;
    let left_shows = if !left.shown {
        "hidden"
    } else if left.view == PaneView::Strip {
        match rule {
            StripFocus::Strip(_) => "strip",
            StripFocus::Midi { .. } => "midi",
            StripFocus::Inspector => "inspector",
            StripFocus::Nothing => "nothing",
        }
    } else {
        left.view.name()
    };
    LayoutStateRow {
        main: l.views.main.name().into(),
        views,
        left_strip: match rule {
            StripFocus::Strip(id) => Some(id),
            _ => None,
        },
        left_shows: left_shows.into(),
    }
}

/// What the left pane's strip view shows (Swift `StripFocus.Result`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripFocus {
    Strip(u64),
    /// A MIDI track's strip: `dest` is its external destination (`None`
    /// when it has none), `lane` the focused track.
    Midi {
        dest: Option<u64>,
        lane: u64,
    },
    Inspector,
    Nothing,
}

impl StripFocus {
    /// The strip `lane` plays through: its audio path's, its plug-in
    /// instrument's, or the built-in synth's; `None` for an external
    /// destination or none.
    pub fn strip_of(lane: &LaneRow, dests: &[DestRow], strips: &[StripRow]) -> Option<u64> {
        let find = |kind: StripKindRow| strips.iter().find(|s| s.kind == kind).map(|s| s.id);
        if let Some(path) = lane.audio_out {
            return find(StripKindRow::AudioPath { path });
        }
        let d = lane.midi_dest?;
        let dest = dests.iter().find(|x| x.id == d)?;
        if dest.plugin.is_some() {
            return find(StripKindRow::Instrument { dest: d });
        }
        if dest.external {
            return None;
        }
        find(StripKindRow::BuiltinSynth)
    }

    /// The first of the focused lanes decides.
    pub fn of(focus: &[&LaneRow], dests: &[DestRow], strips: &[StripRow]) -> StripFocus {
        let Some(first) = focus.first() else {
            return StripFocus::Nothing;
        };
        if let Some(id) = StripFocus::strip_of(first, dests, strips) {
            return StripFocus::Strip(id);
        }
        if first.kind != TrackKindRow::Midi {
            return StripFocus::Inspector;
        }
        let Some(d) = first.midi_dest else {
            return StripFocus::Midi {
                dest: None,
                lane: first.id,
            };
        };
        if dests.iter().find(|x| x.id == d).is_some_and(|x| x.external) {
            StripFocus::Midi {
                dest: Some(d),
                lane: first.id,
            }
        } else {
            StripFocus::Inspector
        }
    }
}

impl DocumentModel {
    /// The selected tracks of the shown sequence, else its armed tracks:
    /// the first is the one the side panes follow.
    pub fn focus_lanes(&self) -> Vec<&LaneRow> {
        let seq = self.selected_seq();
        let selected: Vec<&LaneRow> = self
            .selection()
            .lanes
            .iter()
            .filter(|r| Some(r.seq) == seq)
            .filter_map(|r| self.lanes().iter().find(|l| l.id == r.lane))
            .collect();
        if selected.is_empty() {
            self.lanes().iter().filter(|l| l.arm).collect()
        } else {
            selected
        }
    }

    /// What the left pane's strip view shows now.
    pub fn left_strip(&self, strips: &[StripRow], dests: &[DestRow]) -> StripFocus {
        StripFocus::of(&self.focus_lanes(), dests, strips)
    }

    /// Ends the text edits of what the panes and the centre show, before
    /// they change: the mixer's strip rename is committed (when the mixer
    /// was made). The window's own fields (a track header's rename) are
    /// the UI's to end.
    pub fn end_pane_edits(&mut self) {
        if self.mixer().and_then(|m| m.renaming()).is_some() {
            self.with_mixer(|m, d| m.commit_rename(d));
        }
    }
}
