//! Test and demo documents built through core-model commands (cargo feature
//! `fixtures`). Shared by engine tests, `vizdaw-play` and the app (2c).

use core_model::command::{
    Command, DrumCmd, EventCmd, LaneCmd, ReferenceCmd, Report, RoutingCmd, SequenceCmd, apply,
};
use core_model::document::{DestKind, Document, Patch};
use core_model::event::{EventBody, EventSpec, LoopCount};
use core_model::ids::{AnyId, DestId, EventId, LaneId, SeqId};
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::{PPQ, Tick};
use core_model::{ModelError, Result};

/// One quarter note.
pub const Q: i64 = PPQ;
/// One 4/4 bar.
pub const BAR: i64 = 4 * PPQ;

/// Builds documents with commands, returning the IDs they create.
#[derive(Debug, Clone, Default)]
pub struct Builder {
    doc: Document,
}

fn created<T>(r: &Report, pick: impl Fn(&AnyId) -> Option<T>) -> Result<T> {
    r.created
        .iter()
        .find_map(pick)
        .ok_or_else(|| ModelError::invalid("command created nothing"))
}

impl Builder {
    /// An empty document.
    pub fn new() -> Builder {
        Builder::default()
    }

    /// The document so far.
    pub fn doc(&self) -> &Document {
        &self.doc
    }

    /// The finished document.
    pub fn finish(self) -> Document {
        self.doc
    }

    /// Applies any command.
    pub fn run(&mut self, cmd: Command) -> Result<Report> {
        let a = apply(&self.doc, &cmd)?;
        self.doc = a.doc;
        Ok(a.report)
    }

    /// Adds a destination.
    pub fn dest_kind(
        &mut self,
        name: &str,
        kind: DestKind,
        patch: Option<Patch>,
    ) -> Result<DestId> {
        let r = self.run(Command::Routing(RoutingCmd::AddDestination {
            name: name.into(),
            kind,
            default_patch: patch,
        }))?;
        created(&r, |id| match id {
            AnyId::Dest(d) => Some(*d),
            _ => None,
        })
    }

    /// Adds a MIDI-endpoint destination on group 0.
    pub fn dest(&mut self, name: &str, endpoint_ref: &str, channel: u8) -> Result<DestId> {
        self.dest_kind(
            name,
            DestKind::MidiEndpoint {
                endpoint_ref: endpoint_ref.into(),
                group: 0,
                channel,
            },
            None,
        )
    }

    /// Adds a sequence whose first lane is named `lane` and plays on `dest`.
    pub fn sequence(
        &mut self,
        name: &str,
        lane: &str,
        dest: Option<DestId>,
    ) -> Result<(SeqId, LaneId)> {
        let r = self.run(Command::Sequence(SequenceCmd::New { name: name.into() }))?;
        let seq = created(&r, |id| match id {
            AnyId::Seq(s) => Some(*s),
            _ => None,
        })?;
        let first = created(&r, |id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        })?;
        self.run(Command::Lane(LaneCmd::Rename {
            seq,
            lane: first,
            name: lane.into(),
        }))?;
        self.run(Command::Lane(LaneCmd::SetDest {
            seq,
            lane: first,
            dest,
        }))?;
        Ok((seq, first))
    }

    /// Adds a lane playing on `dest`.
    pub fn lane(&mut self, seq: SeqId, name: &str, dest: Option<DestId>) -> Result<LaneId> {
        let r = self.run(Command::Lane(LaneCmd::Add {
            seq,
            name: name.into(),
            index: None,
        }))?;
        let lane = created(&r, |id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        })?;
        self.run(Command::Lane(LaneCmd::SetDest { seq, lane, dest }))?;
        Ok(lane)
    }

    /// Inserts events into a lane's active take.
    pub fn events(
        &mut self,
        seq: SeqId,
        lane: LaneId,
        events: Vec<EventSpec>,
    ) -> Result<Vec<EventId>> {
        let r = self.run(Command::Event(EventCmd::Insert { seq, lane, events }))?;
        Ok(r.created
            .iter()
            .filter_map(|id| match id {
                AnyId::Event(e) => Some(*e),
                _ => None,
            })
            .collect())
    }

    /// Inserts notes given as `(tick, pitch, velocity16, duration)`.
    pub fn notes(
        &mut self,
        seq: SeqId,
        lane: LaneId,
        notes: &[(i64, u8, u16, i64)],
    ) -> Result<Vec<EventId>> {
        let specs = notes
            .iter()
            .map(|(t, p, v, d)| EventSpec::new(Tick(*t), EventBody::note(*p, *v, Tick(*d))))
            .collect();
        self.events(seq, lane, specs)
    }

    /// Inserts one event body at `tick`.
    pub fn event(
        &mut self,
        seq: SeqId,
        lane: LaneId,
        tick: i64,
        body: EventBody,
    ) -> Result<EventId> {
        let ids = self.events(seq, lane, vec![EventSpec::new(Tick(tick), body)])?;
        ids.first()
            .copied()
            .ok_or_else(|| ModelError::invalid("no event created"))
    }

    /// Inserts a reference to `target`.
    pub fn reference(
        &mut self,
        seq: SeqId,
        lane: LaneId,
        tick: i64,
        target: SeqId,
        loop_count: u32,
        transpose: i8,
    ) -> Result<EventId> {
        let r = self.run(Command::Reference(ReferenceCmd::Insert {
            seq,
            lane,
            tick: Tick(tick),
            target,
            length: None,
            loop_count: LoopCount::Count(loop_count),
            transpose,
        }))?;
        created(&r, |id| match id {
            AnyId::Event(e) => Some(*e),
            _ => None,
        })
    }

    /// Replaces a sequence's tempo map; points are `(tick, whole BPM, ramp)`.
    pub fn tempo(&mut self, seq: SeqId, points: &[(i64, u32, bool)]) -> Result<()> {
        let tempo = TempoMap::new(
            points
                .iter()
                .map(|(t, b, r)| TempoPoint {
                    tick: Tick(*t),
                    bpm: Bpm::whole(*b),
                    ramp: *r,
                })
                .collect(),
        )?;
        self.run(Command::Sequence(SequenceCmd::SetTempo { seq, tempo }))?;
        Ok(())
    }

    /// Makes a lane a drum lane and adds `(name, note)` rows.
    pub fn drum_rows(&mut self, seq: SeqId, lane: LaneId, rows: &[(&str, u8)]) -> Result<()> {
        self.run(Command::Drum(DrumCmd::MarkDrumLane {
            seq,
            lane,
            drum: true,
        }))?;
        for (name, note) in rows {
            self.run(Command::Drum(DrumCmd::AddRow {
                seq,
                lane,
                name: (*name).into(),
                note: *note,
                dest: None,
            }))?;
        }
        Ok(())
    }
}

/// A 7-bit value as a 32-bit MIDI 2.0 controller value.
pub fn cc32(v7: u8) -> u32 {
    u32::from(v7 & 0x7F) << 25
}

/// The demo project (2c spec §6): Song → Groove → Beat on the built-in
/// synth, 12 bars at 120 BPM with a ramp to 90 BPM over the last 2 bars.
pub fn demo_document() -> Result<Document> {
    let mut b = Builder::new();
    let piano = b.dest("Built-in Piano", "builtin:synth", 0)?;
    let bass = b.dest("Built-in Bass", "builtin:synth", 1)?;
    let drums = b.dest_kind(
        "Built-in Drums",
        DestKind::MidiEndpoint {
            endpoint_ref: "builtin:synth".into(),
            group: 0,
            channel: 9,
        },
        Some(Patch {
            program: 0,
            bank: None,
        }),
    )?;

    // Beat: one bar of kick, snare and closed hats on eighths.
    let (beat, kit) = b.sequence("Beat", "Drums", Some(drums))?;
    b.drum_rows(
        beat,
        kit,
        &[("Kick", 36), ("Snare", 38), ("Closed Hat", 42)],
    )?;
    let mut hits: Vec<(i64, u8, u16, i64)> = Vec::new();
    for i in 0..8 {
        let accent = if i % 2 == 0 { 0x9000 } else { 0x6000 };
        hits.push((i * Q / 2, 42, accent, Q / 4));
    }
    hits.extend([
        (0, 36, 0xC000, Q / 4),
        (2 * Q, 36, 0xC000, Q / 4),
        (Q, 38, 0xB000, Q / 4),
        (3 * Q, 38, 0xB000, Q / 4),
    ]);
    b.notes(beat, kit, &hits)?;

    // Groove: 4 bars of Beat plus a bass line (Fingered Bass).
    let (groove, groove_drums) = b.sequence("Groove", "Drums", Some(drums))?;
    b.reference(groove, groove_drums, 0, beat, 4, 0)?;
    let bass_lane = b.lane(groove, "Bass", Some(bass))?;
    b.event(
        groove,
        bass_lane,
        0,
        EventBody::Program {
            program: 33,
            bank: None,
        },
    )?;
    let mut line: Vec<(i64, u8, u16, i64)> = Vec::new();
    for (bar, root) in [36u8, 33, 29, 31].into_iter().enumerate() {
        let start = bar as i64 * BAR;
        for (beat_no, interval) in [0u8, 12, 7, 12].into_iter().enumerate() {
            line.push((
                start + beat_no as i64 * Q,
                root + interval,
                0xA000,
                Q * 9 / 10,
            ));
        }
    }
    b.notes(groove, bass_lane, &line)?;

    // Song: Groove twice (the second use a fourth up), chords over 12 bars,
    // ritardando over the last 2 bars.
    let (song, song_groove) = b.sequence("Song", "Groove", None)?;
    b.reference(song, song_groove, 0, groove, 1, 0)?;
    b.reference(song, song_groove, 4 * BAR, groove, 1, 5)?;
    let chords = b.lane(song, "Chords", Some(piano))?;
    b.event(
        song,
        chords,
        0,
        EventBody::Program {
            program: 0,
            bank: None,
        },
    )?;
    b.event(
        song,
        chords,
        0,
        EventBody::Cc {
            index: 7,
            value: cc32(100),
        },
    )?;
    let shapes: [[u8; 3]; 4] = [[60, 64, 67], [57, 60, 64], [53, 57, 60], [55, 59, 62]];
    let mut voicings: Vec<(i64, u8, u16, i64)> = Vec::new();
    for bar in 0..12i64 {
        let shift: u8 = if (4..8).contains(&bar) { 5 } else { 0 };
        let idx = usize::try_from(bar % 4).unwrap_or(0);
        let Some(shape) = shapes.get(idx) else {
            continue;
        };
        for p in shape {
            voicings.push((bar * BAR, p + shift, 0x7000, BAR - Q / 8));
        }
    }
    b.notes(song, chords, &voicings)?;
    b.event(
        song,
        chords,
        11 * BAR + 2 * Q,
        EventBody::PitchBend { value: 0x9000_0000 },
    )?;
    b.event(
        song,
        chords,
        11 * BAR + 3 * Q,
        EventBody::PitchBend { value: 0x8000_0000 },
    )?;
    b.tempo(
        song,
        &[
            (0, 120, false),
            (10 * BAR, 120, true),
            (12 * BAR, 90, false),
        ],
    )?;
    Ok(b.finish())
}
