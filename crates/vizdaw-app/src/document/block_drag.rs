//! The arrangement's block drags on the document (Swift
//! `ArrangementView.handle`'s drag part, `beginDrag`, `target`): the tool
//! machine's events become one gesture. Clicks are the canvas's own.

use super::DocumentModel;
use crate::arrangement::{
    ArrangementFrame, ArrangementHit, BlockDrag, BlockDragState, MachineEvent, Point, ToolMachine,
};
use crate::editor::Tool;
use vizdaw_ffi::edit::{EditCommand, LaneRef};

impl DocumentModel {
    /// Runs the block-drag part of `events` (a begin on a block, moves,
    /// the end, a cancel); `drag` is the canvas's drag in progress. Clicks
    /// and drags from empty space are left to the caller.
    pub fn block_drag_events(
        &mut self,
        drag: &mut Option<BlockDragState>,
        events: &[MachineEvent<ArrangementHit>],
        frame: &ArrangementFrame,
    ) {
        for (i, e) in events.iter().enumerate() {
            match e {
                MachineEvent::Begin {
                    hit: ArrangementHit::Block { lane, index },
                    from,
                    modifiers,
                } => {
                    // The first move comes with the begin: its row is the
                    // target the gesture starts out touching.
                    let to = match events.get(i + 1) {
                        Some(MachineEvent::Move { to, .. }) => *to,
                        _ => *from,
                    };
                    *drag =
                        self.begin_block_drag(*lane, *index, *from, to, modifiers.option, frame);
                }
                MachineEvent::Move { to, .. } => {
                    if let Some(d) = drag.as_ref() {
                        let target = self.drag_target(d, frame, *to);
                        self.update_gesture(target);
                    }
                }
                MachineEvent::End { to, .. } => {
                    if let Some(d) = drag.take() {
                        let target = self.drag_target(&d, frame, *to);
                        self.end_gesture(target);
                    }
                }
                MachineEvent::Cancel => {
                    if drag.take().is_some() {
                        self.cancel_gesture();
                    }
                }
                MachineEvent::Begin { .. } | MachineEvent::Click { .. } => {}
            }
        }
    }

    /// Escape on the canvas (Swift `ArrangementView.contentCancel`): the
    /// machine's cancel runs, the drag is dropped, and a gesture still
    /// open is cancelled too, which also aborts a final edit still
    /// pending on Busy after the mouse-up. The machine then swallows the
    /// matching mouse-up.
    pub fn cancel_block_drag(
        &mut self,
        machine: &mut ToolMachine<ArrangementHit>,
        drag: &mut Option<BlockDragState>,
        frame: &ArrangementFrame,
    ) {
        let events = machine.cancel();
        self.block_drag_events(drag, &events, frame);
        *drag = None;
        if self.gesture_open() {
            self.cancel_gesture();
        }
    }

    /// Opens the drag of block `index` of track `lane` pressed at `from`,
    /// the pointer now at `to`: only with the arrow tool; ⌥ (`copy`) read
    /// here only. The selection is not changed. `None` when nothing opened.
    pub fn begin_block_drag(
        &mut self,
        lane: u64,
        index: usize,
        from: Point,
        to: Point,
        copy: bool,
        frame: &ArrangementFrame,
    ) -> Option<BlockDragState> {
        if self.tool() != Tool::Arrow {
            return None;
        }
        let seq = self.selected_seq()?;
        let blocks = self.blocks(lane);
        let pressed = blocks.get(index)?;
        let d = BlockDragState {
            source: LaneRef { seq, lane },
            block_start: pressed.start,
            events: BlockDrag::events(pressed, &blocks, &self.selected_events),
            rows: if copy {
                self.shown_lane_events(lane).to_vec()
            } else {
                vec![]
            },
            copy,
            start_tick: frame.axis.tick_at(from.x),
        };
        let lanes = d.touched(frame, &self.lane_ids(), to);
        self.begin_gesture(BlockDrag::label(copy), lanes)
            .then_some(d)
    }

    /// Watches the source and target tracks with the pointer at `to`, and
    /// gives the drag's full target there.
    fn drag_target(
        &mut self,
        d: &BlockDragState,
        frame: &ArrangementFrame,
        to: Point,
    ) -> Option<EditCommand> {
        let lanes = self.lane_ids();
        self.set_gesture_lanes(d.touched(frame, &lanes, to));
        let snap = self.layout().snap;
        let meter = self
            .sequence_info(d.source.seq)
            .map(|i| i.meter.as_slice())
            .unwrap_or_default();
        d.target(frame, &lanes, to, &snap, meter)
    }

    fn lane_ids(&self) -> Vec<u64> {
        self.lanes().iter().map(|l| l.id).collect()
    }
}
