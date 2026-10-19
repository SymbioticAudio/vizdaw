//! "New from Template" (parent spec §9, Plan Ruling 13): a new sequence
//! built from core commands inside the caller's transaction.

use crate::error::VizError;
use crate::events::bad;
use core_model::command::drum::DrumCmd;
use core_model::command::lane::LaneCmd;
use core_model::command::sequence::SequenceCmd;
use core_model::command::{Command, Report};
use core_model::history::History;
use core_model::ids::{AnyId, LaneId, SeqId};
use core_model::selection::ViewFilter;

/// What a new sequence starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SequenceTemplate {
    /// One empty lane.
    Empty,
    /// One drum lane "Drums" with General MIDI rows.
    DrumKit,
    /// The lanes (names, routing, drum rows, own length), tempo, meter and
    /// start offset of sequence `seq`, without events.
    SameLanesAs {
        /// The sequence to copy the shape of.
        seq: u64,
    },
}

/// The drum kit template's rows: name and General MIDI note.
pub const DRUM_KIT: [(&str, u8); 5] = [
    ("Kick", 36),
    ("Snare", 38),
    ("Closed Hat", 42),
    ("Open Hat", 46),
    ("Crash", 49),
];

fn lane_of(r: &Report) -> Result<LaneId, VizError> {
    r.created
        .iter()
        .find_map(|id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        })
        .ok_or_else(|| bad("no lane was created"))
}

fn drum_row_of(r: &Report) -> Result<core_model::ids::DrumRowId, VizError> {
    r.created
        .iter()
        .find_map(|id| match id {
            AnyId::DrumRow(x) => Some(*x),
            _ => None,
        })
        .ok_or_else(|| bad("no drum row was created"))
}

/// Builds sequence `name` from `template` in `history` (the caller holds an
/// open transaction and aborts it on error). Returns the new sequence.
pub fn build(
    history: &mut History,
    name: &str,
    template: &SequenceTemplate,
) -> Result<SeqId, VizError> {
    let source = match template {
        SequenceTemplate::SameLanesAs { seq } => Some(history.doc().sequence(SeqId(*seq))?.clone()),
        _ => None,
    };
    let filter = ViewFilter::default();
    let mut run = |cmd: Command| history.execute(&cmd, &filter).map_err(VizError::from);
    let r = run(Command::Sequence(SequenceCmd::New {
        name: name.to_string(),
    }))?;
    let seq = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Seq(s) => Some(*s),
            _ => None,
        })
        .ok_or_else(|| bad("no sequence was created"))?;
    let first = lane_of(&r)?;
    match (template, source) {
        (SequenceTemplate::DrumKit, _) => {
            run(Command::Lane(LaneCmd::Rename {
                seq,
                lane: first,
                name: "Drums".into(),
            }))?;
            run(Command::Drum(DrumCmd::MarkDrumLane {
                seq,
                lane: first,
                drum: true,
            }))?;
            for (row, note) in DRUM_KIT {
                run(Command::Drum(DrumCmd::AddRow {
                    seq,
                    lane: first,
                    name: row.into(),
                    note,
                    dest: None,
                }))?;
            }
        }
        (SequenceTemplate::SameLanesAs { .. }, Some(src)) => {
            run(Command::Sequence(SequenceCmd::SetTempo {
                seq,
                tempo: src.tempo.clone(),
            }))?;
            run(Command::Sequence(SequenceCmd::SetMeter {
                seq,
                meter: src.meter.clone(),
            }))?;
            run(Command::Sequence(SequenceCmd::SetStartOffset {
                seq,
                offset: src.start_offset,
            }))?;
            for (i, l) in src.lanes.iter().enumerate() {
                let lane = if i == 0 {
                    first
                } else {
                    lane_of(&run(Command::Lane(LaneCmd::Add {
                        seq,
                        name: l.name.clone(),
                        index: None,
                    }))?)?
                };
                run(Command::Lane(LaneCmd::Rename {
                    seq,
                    lane,
                    name: l.name.clone(),
                }))?;
                run(Command::Lane(LaneCmd::SetDest {
                    seq,
                    lane,
                    dest: l.midi_dest,
                }))?;
                run(Command::Lane(LaneCmd::SetOutput {
                    seq,
                    lane,
                    output: l.audio_out,
                }))?;
                if let Some(length) = l.length {
                    run(Command::Lane(LaneCmd::SetLength {
                        seq,
                        lane,
                        length: Some(length),
                        looped: l.looped,
                        locked: l.length_locked,
                    }))?;
                }
                if let Some(drum) = &l.drum {
                    run(Command::Drum(DrumCmd::MarkDrumLane {
                        seq,
                        lane,
                        drum: true,
                    }))?;
                    for row in &drum.rows {
                        let made = run(Command::Drum(DrumCmd::AddRow {
                            seq,
                            lane,
                            name: row.name.clone(),
                            note: row.note,
                            dest: row.dest,
                        }))?;
                        if row.show_velocity {
                            run(Command::Drum(DrumCmd::SetRowShowVelocity {
                                seq,
                                lane,
                                row: drum_row_of(&made)?,
                                show: true,
                            }))?;
                        }
                    }
                }
            }
        }
        _ => {}
    }
    Ok(seq)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_model::document::Document;

    #[test]
    fn same_lanes_as_copies_each_rows_velocity_flag() {
        let mut h = History::new(Document::new());
        let f = ViewFilter::default();
        h.begin("setup").unwrap();
        let r = h
            .execute(
                &Command::Sequence(SequenceCmd::New { name: "Src".into() }),
                &f,
            )
            .unwrap();
        let seq = r
            .created
            .iter()
            .find_map(|id| match id {
                AnyId::Seq(s) => Some(*s),
                _ => None,
            })
            .unwrap();
        let lane = lane_of(&r).unwrap();
        h.execute(
            &Command::Drum(DrumCmd::MarkDrumLane {
                seq,
                lane,
                drum: true,
            }),
            &f,
        )
        .unwrap();
        let mut rows = Vec::new();
        for (n, note) in [("A", 36u8), ("B", 38)] {
            let r = h
                .execute(
                    &Command::Drum(DrumCmd::AddRow {
                        seq,
                        lane,
                        name: n.into(),
                        note,
                        dest: None,
                    }),
                    &f,
                )
                .unwrap();
            rows.push(drum_row_of(&r).unwrap());
        }
        h.execute(
            &Command::Drum(DrumCmd::SetRowShowVelocity {
                seq,
                lane,
                row: rows[1],
                show: true,
            }),
            &f,
        )
        .unwrap();
        h.commit().unwrap();

        h.begin("copy").unwrap();
        let copy = build(
            &mut h,
            "Copy",
            &SequenceTemplate::SameLanesAs { seq: seq.0 },
        )
        .unwrap();
        h.commit().unwrap();
        let flags: Vec<(String, bool)> = h
            .doc()
            .sequence(copy)
            .unwrap()
            .lanes
            .iter()
            .flat_map(|l| l.drum.iter())
            .flat_map(|d| d.rows.iter().map(|r| (r.name.clone(), r.show_velocity)))
            .collect();
        assert_eq!(flags, vec![("A".into(), false), ("B".into(), true)]);
    }
}
