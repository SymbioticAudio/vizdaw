//! The editor state of one document that is not document state (Swift
//! `EditorModel`'s tool, layout and detail parts): the tool, the saved
//! layout and whether a drag is open. `DocumentModel` holds one and
//! exposes it; the selection and edits that need the session are
//! `DocumentModel`'s (`document/editing.rs`).

mod detail;
mod gesture;
mod layout;
mod nudge;
mod snap;

pub use detail::{DetailEditorKind, DetailState, EditorTarget, ListFilter};
pub use gesture::{Gesture, LANE_REMOVED};
pub(crate) use layout::field;
pub use layout::{
    BlockRuleSetting, EditorLayout, LightMode, MainView, PaneSetup, PaneSide, PaneState, PaneView,
    TransportMarks, ViewLayouts, controllers_key, off_list,
};
pub use nudge::NudgeDirection;
pub use snap::{Division, Snap};

use vizdaw_ffi::snapshot::DocSnapshot;

/// Shown when an edit, undo or redo arrives while a drag is open (it
/// would join the drag's undo step, or be lost when it is cancelled).
pub const GESTURE_OPEN: &str = "Finish the drag first.";

/// The editing tool. Not saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tool {
    #[default]
    Arrow,
    Pencil,
    Eraser,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::Arrow, Tool::Pencil, Tool::Eraser];

    pub fn title(self) -> &'static str {
        match self {
            Tool::Arrow => "Arrow",
            Tool::Pencil => "Pencil",
            Tool::Eraser => "Eraser",
        }
    }

    /// The control channel's name.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Arrow => "arrow",
            Tool::Pencil => "pencil",
            Tool::Eraser => "eraser",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EditorModel {
    pub tool: Tool,
    layout: EditorLayout,
    /// The drag holding an open edit, if any.
    pub(crate) gesture: Option<Gesture>,
    /// How many gestures have begun (the last one's serial).
    pub(crate) gestures_begun: u64,
    /// The `ui.json` last read or stored: what a save merges into when the
    /// session cannot answer.
    pub(crate) ui_json: Option<String>,
    /// The detail pane's list (cursor, last inserted kind, cached rows).
    pub(crate) list: crate::list::ListModel,
    /// The detail pane's piano roll (edit lane, reveal, cached notes).
    pub(crate) roll: crate::piano_roll::PianoRollModel,
    /// The audition notes held, the note-offs to retry.
    pub(crate) audition: crate::audition::Audition,
    /// Exact Mode: a note or block drag opens the Modify panel with its
    /// amounts instead of applying (Swift `EditorModel.exactMode`). Not
    /// saved.
    pub(crate) exact_mode: bool,
}

impl EditorModel {
    /// The editor state of a document whose `ui.json` is `json`.
    pub fn new(json: Option<&str>) -> Self {
        EditorModel {
            tool: Tool::Arrow,
            layout: EditorLayout::decode(json),
            gesture: None,
            gestures_begun: 0,
            ui_json: json.map(String::from),
            list: crate::list::ListModel::default(),
            roll: crate::piano_roll::PianoRollModel::default(),
            audition: crate::audition::Audition::default(),
            exact_mode: false,
        }
    }

    /// Exact Mode is on (off in every new document).
    pub fn exact_mode(&self) -> bool {
        self.exact_mode
    }

    pub fn set_exact_mode(&mut self, on: bool) {
        self.exact_mode = on;
    }

    pub fn layout(&self) -> &EditorLayout {
        &self.layout
    }

    pub fn update_layout(&mut self, change: impl FnOnce(&mut EditorLayout)) {
        change(&mut self.layout);
    }

    /// A drag holds an open edit (until its end or abort went through).
    pub fn gesture_open(&self) -> bool {
        self.gesture.is_some()
    }

    pub fn gesture(&self) -> Option<&Gesture> {
        self.gesture.as_ref()
    }

    /// Shows `target` in the detail pane in `kind`, else the last-used
    /// editor (the Piano Roll when that one is not in this app yet); the current view's bottom pane shows the editor. The piano
    /// roll's extra lanes stay only while the target is the same; the
    /// list filter and the other saved fields stay. A pending piano roll
    /// reveal is dropped.
    pub fn open_detail(&mut self, target: EditorTarget, kind: Option<DetailEditorKind>) {
        let main = self.layout.views.main;
        self.open_detail_in(target, kind, main);
    }

    /// [`EditorModel::open_detail`], with `main`'s bottom pane showing it
    /// (`ui.pane {main}` sets a view that is not the current one).
    pub fn open_detail_in(
        &mut self,
        target: EditorTarget,
        kind: Option<DetailEditorKind>,
        main: MainView,
    ) {
        self.roll.reveal = None;
        let l = &mut self.layout;
        let bottom = &mut l.views.setup_mut(main).bottom;
        bottom.shown = true;
        bottom.view = PaneView::Editor;
        let kind = match kind {
            Some(k) => {
                l.last_kind = k;
                k
            }
            // A last-used editor this app does not have yet (a song saved
            // with the Strip Chart or Drum Grid) opens the Piano Roll and
            // stays the saved one.
            None if !l.last_kind.is_available() => DetailEditorKind::PianoRoll,
            None => l.last_kind,
        };
        let mut next = DetailState::new(kind, target);
        if l.detail.as_ref().is_none_or(|old| old.target != target) {
            self.list.retarget();
        }
        if let Some(old) = l.detail.take() {
            next.list_filter = old.list_filter;
            next.extra = old.extra;
            if old.target != target {
                next.set_lanes(None);
            }
        }
        l.detail = Some(next);
    }

    pub fn close_detail(&mut self) {
        self.layout.detail = None;
    }

    /// The control channel's detail kind: "none" when nothing is open
    /// (whether or not the pane shows).
    pub fn detail_kind_name(&self) -> &'static str {
        self.layout
            .detail
            .as_ref()
            .map_or("none", |d| d.kind.name())
    }

    /// Drops the detail when its sequence or track is gone from `s`; the
    /// piano roll's shown tracks that are gone leave it silently (none
    /// left is none).
    pub fn prune(&mut self, s: &DocSnapshot) {
        let Some(d) = &mut self.layout.detail else {
            return;
        };
        let t = d.target;
        let alive = |lane: u64| {
            s.lane_revs
                .iter()
                .any(|r| r.seq == t.seq() && r.lane == lane)
        };
        let seq_gone = !s.sequences.iter().any(|q| q.id == t.seq());
        let lane_gone = t.lane().is_some_and(|lane| !alive(lane));
        if seq_gone || lane_gone {
            self.layout.detail = None;
            return;
        }
        if let Some(extra) = d.lanes() {
            // As Swift: an empty list (a sequence showing none) becomes
            // the default set too.
            let kept: Vec<u64> = extra.iter().copied().filter(|&l| alive(l)).collect();
            if kept.is_empty() || kept.len() != extra.len() {
                d.set_lanes((!kept.is_empty()).then_some(kept));
            }
        }
    }
}
