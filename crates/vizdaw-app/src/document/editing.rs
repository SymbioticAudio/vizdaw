//! The document's editing side (Swift `EditorModel`'s selection, edit and
//! detail parts): the parts of the editor state that need the session or
//! the selection. The state itself is [`EditorModel`].

use vizdaw_ffi::VizError;
use vizdaw_ffi::blocks::BlockRow;
use vizdaw_ffi::edit::{EditCommand, LaneRef, ViewFilterRow};
use vizdaw_ffi::selection::{SelectionRow, TickRange};
use vizdaw_ffi::snapshot::DocSnapshot;

use super::{DocumentModel, empty_selection};
use crate::alerts::error_message;
use crate::arrangement::bar_at;
use crate::editor::{
    DetailEditorKind, DetailState, EditorLayout, EditorModel, EditorTarget, GESTURE_OPEN, MainView,
    PaneView, Tool,
};

impl DocumentModel {
    pub fn editor(&self) -> &EditorModel {
        &self.editor
    }

    pub fn tool(&self) -> Tool {
        self.editor.tool
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.editor.tool = tool;
    }

    pub fn layout(&self) -> &EditorLayout {
        self.editor.layout()
    }

    /// Changes the layout; the next save writes it.
    pub fn update_layout(&mut self, change: impl FnOnce(&mut EditorLayout)) {
        self.editor.update_layout(change);
    }

    /// A drag holds an open edit: edits, undo and redo wait.
    pub fn gesture_open(&self) -> bool {
        self.editor.gesture_open()
    }

    /// A failure of a user action, in the status line.
    pub(crate) fn report(&mut self, e: &VizError) {
        self.status = Some(error_message(e));
    }

    /// Sends one editor command (one undo step): its snapshot is applied
    /// and the selection re-read. A refusal is the status; refused while a
    /// drag is open. Whether it applied.
    pub fn edit(&mut self, cmd: EditCommand) -> bool {
        self.change(|s| s.edit(cmd))
    }

    /// [`DocumentModel::edit`] through a view's filter (`None`: nothing
    /// hidden): what it hides is left alone.
    pub fn edit_filtered(&mut self, cmd: EditCommand, filter: Option<ViewFilterRow>) -> bool {
        self.change(|s| s.edit_filtered(cmd, filter))
    }

    /// Runs `cmds` as one undo step named `label`: all of them or none.
    /// One command is a plain edit, named after itself; none is done.
    pub fn edit_all(&mut self, mut cmds: Vec<EditCommand>, label: &str) -> bool {
        match cmds.len() {
            0 => true,
            1 => self.edit(cmds.remove(0)),
            _ => self.change(|s| s.edit_all(label, cmds)),
        }
    }

    fn change(
        &mut self,
        body: impl FnOnce(&dyn crate::DocSession) -> Result<DocSnapshot, VizError>,
    ) -> bool {
        if self.editor.gesture_open() {
            self.status = Some(GESTURE_OPEN.into());
            return false;
        }
        match body(self.session.as_ref()) {
            Ok(snapshot) => {
                self.apply(snapshot);
                self.refresh_selection();
                true
            }
            Err(e) => {
                self.report(&e);
                false
            }
        }
    }

    /// Makes `s` the selection. The model then holds exactly what the
    /// session kept (it drops lanes and events that do not exist); a
    /// refusal is the status, and the model goes back to what the session
    /// holds.
    pub fn set_selection(&mut self, s: SelectionRow) {
        let before = self.selection.clone();
        self.hold(s.clone());
        match self.session.set_selection(s) {
            Ok(()) => self.refresh_selection(),
            Err(e) => {
                self.report(&e);
                let held = self.session.selection().unwrap_or(before);
                self.hold(held);
            }
        }
    }

    pub fn clear_selection(&mut self) {
        self.set_selection(empty_selection());
    }

    /// Selects a block of track `lane` (of the shown sequence): its
    /// events, its track and its time range (at least one tick). `adding`
    /// unites them with the current selection.
    pub fn select_block(&mut self, lane: u64, block: &BlockRow, adding: bool) {
        let Some(seq) = self.selected_seq else { return };
        let mut s = if adding {
            self.selection.clone()
        } else {
            empty_selection()
        };
        for id in &block.events {
            if !s.events.contains(id) {
                s.events.push(*id);
            }
        }
        let r = LaneRef { seq, lane };
        if !s.lanes.contains(&r) {
            s.lanes.push(r);
        }
        let range = TickRange {
            start: block.start,
            end: block.end.max(block.start + 1),
        };
        s.time_range = Some(match s.time_range {
            Some(old) if adding => TickRange {
                start: old.start.min(range.start),
                end: old.end.max(range.end),
            },
            _ => range,
        });
        self.set_selection(s);
    }

    /// Every event of `block` is selected (an empty block never is).
    pub fn is_selected(&self, block: &BlockRow) -> bool {
        !block.events.is_empty()
            && block
                .events
                .iter()
                .all(|e| self.selected_events.contains(e))
    }

    pub fn is_event_selected(&self, id: u64) -> bool {
        self.selected_events.contains(&id)
    }

    /// What a detail editor opens on: the selection's single track (with
    /// its range when there is one), else the shown sequence.
    pub fn detail_target(&self) -> Option<EditorTarget> {
        if let [l] = self.selection.lanes.as_slice() {
            return Some(match self.selection.time_range {
                Some(r) => EditorTarget::Block {
                    seq: l.seq,
                    lane: l.lane,
                    start: r.start,
                    end: r.end,
                },
                None => EditorTarget::Lane {
                    seq: l.seq,
                    lane: l.lane,
                },
            });
        }
        self.selected_seq.map(|seq| EditorTarget::Sequence { seq })
    }

    /// Shows `target` in the detail pane, in `kind` or the last-used
    /// editor.
    pub fn open_detail(&mut self, target: EditorTarget, kind: Option<DetailEditorKind>) {
        let main = self.layout().views.main;
        self.open_detail_in(target, kind, main);
    }

    /// [`DocumentModel::open_detail`] in `main`'s bottom pane. When that
    /// pane shows something other than the editor (the mixer), its text
    /// edits end first: a strip rename is committed, not dropped (Swift
    /// `EditorModel.openDetail`).
    pub fn open_detail_in(
        &mut self,
        target: EditorTarget,
        kind: Option<DetailEditorKind>,
        main: MainView,
    ) {
        if self.layout().views.setup(main).bottom.view != PaneView::Editor {
            self.end_pane_edits();
        }
        let before = self.layout().detail.clone();
        // Opening a track in the Drum Grid gives it a drum layout first
        // (`M/EditorModel.swift:413`).
        if self.editor.opening_kind(kind) == DetailEditorKind::DrumGrid {
            self.prepare_drum_grid(target);
        }
        self.editor.open_detail_in(target, kind, main);
        self.detail_changed(before);
    }

    /// ⌘1–⌘4: the detail pane shows `kind`, on its current target or on
    /// the selection; nothing to show opens nothing.
    pub fn show_detail(&mut self, kind: DetailEditorKind) {
        let target = self
            .layout()
            .detail
            .as_ref()
            .map(|d| d.target)
            .or_else(|| self.detail_target());
        if let Some(t) = target {
            self.open_detail(t, Some(kind));
        }
    }

    /// A double-clicked events block of track `lane` (of the shown
    /// sequence): the detail opens on it in the last-used editor (the
    /// Piano Roll at first), which stays the last-used one.
    pub fn open_block_detail(&mut self, lane: u64, block: &BlockRow) {
        let Some(seq) = self.selected_seq else { return };
        let target = EditorTarget::Block {
            seq,
            lane,
            start: block.start,
            end: block.end,
        };
        self.open_detail(target, None);
    }

    /// The pane's detail changed from `before`: the Strip Chart's and the
    /// Drum Grid's follow.
    pub(crate) fn detail_changed(&mut self, before: Option<DetailState>) {
        self.strip_detail_changed(before.clone());
        self.drum_detail_changed(before.as_ref());
    }

    /// Closes the detail; a Strip Chart stroke in it is cancelled.
    pub fn close_detail(&mut self) {
        let before = self.layout().detail.clone();
        self.editor.close_detail();
        self.detail_changed(before);
    }

    /// The control channel's detail kind ("none" when nothing is open).
    pub fn detail_kind_name(&self) -> &'static str {
        self.editor.detail_kind_name()
    }

    /// The open detail's title ("Song › Chords, bars 1–2"); empty when
    /// none is open.
    pub fn detail_title(&self) -> String {
        match &self.layout().detail {
            Some(d) => self.target_title(&d.target),
            None => String::new(),
        }
    }

    /// `t`'s title: the sequence; with a track "<Seq> › <Track>"; a block
    /// adds ", bar N" or ", bars A–B".
    pub fn target_title(&self, t: &EditorTarget) -> String {
        let seq = t.seq();
        let seq_name = self
            .snapshot
            .sequences
            .iter()
            .find(|q| q.id == seq)
            .map_or_else(|| "Sequence".to_owned(), |q| q.name.clone());
        let Some(lane) = t.lane() else {
            return seq_name;
        };
        let name = |rows: &[vizdaw_ffi::snapshot::LaneRow]| {
            rows.iter()
                .find(|l| l.id == lane)
                .map_or_else(|| "Track".to_owned(), |l| l.name.clone())
        };
        let lane_name = name(&self.lane_rows(seq));
        let EditorTarget::Block { start, end, .. } = *t else {
            return format!("{seq_name} \u{203a} {lane_name}");
        };
        let meter = self
            .sequence_info(seq)
            .map(|i| i.meter.clone())
            .unwrap_or_default();
        let first = bar_at(start, &meter).number;
        let last = bar_at(end.saturating_sub(1).max(start), &meter).number;
        let bars = if first == last {
            format!("bar {first}")
        } else {
            format!("bars {first}\u{2013}{last}")
        };
        format!("{seq_name} \u{203a} {lane_name}, {bars}")
    }

    /// Hands the layout to the session, merged into the `ui.json` it holds
    /// (keys this app does not model stay); the next save writes it. A
    /// session that cannot answer merges into what was read last.
    pub(super) fn store_layout(&mut self) {
        let existing = match self.session.ui_layout() {
            Ok(json) => json,
            Err(_) => self.editor.ui_json.clone(),
        };
        let json = self.editor.layout().merge_into(existing.as_deref());
        if self.session.set_ui_layout(json.clone()).is_ok() {
            self.editor.ui_json = Some(json);
        }
    }
}
