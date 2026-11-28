//! Typed cell text to commands, new rows and deleting rows (Swift
//! `ListEdit.swift`).

use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef, SequenceEdit};
use vizdaw_ffi::events::{
    AutomationRow, EventKindRow, EventRow, MarkerRow, MeterEntryRow, SequenceInfo, TempoPointRow,
};

use super::format::{
    Bbu, Bend14, DurationText, KeyName, PitchName, Smpte, UNITS_PER_BEAT, Value7, Velocity7,
    parse_position, trim_ws,
};
use super::rows::{ListColumn, ListContext, ListKind, ListRow, ListSource, ListText};
use crate::arrangement::{PPQ, bar_at};
use crate::position::{swift_int, swift_uint};
use crate::strip_chart::{parse_percent, v32};

/// Why typed text was refused; the message goes to the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEditError(pub String);

impl ListEditError {
    pub fn message(&self) -> &str {
        &self.0
    }
}

fn refuse<T>(message: impl Into<String>) -> std::result::Result<T, ListEditError> {
    Err(ListEditError(message.into()))
}

/// What a committed cell does.
#[allow(clippy::large_enum_variant)] // one per commit, never stored
#[derive(Debug, Clone, PartialEq)]
pub enum ListChange {
    Unchanged,
    /// Send the command; the row then starts at the tick (the list
    /// follows it).
    Edit(EditCommand, i64),
}

fn event_edit(edit: EventEdit, tick: i64) -> ListChange {
    ListChange::Edit(EditCommand::Event { edit }, tick)
}

fn sequence_edit(edit: SequenceEdit, tick: i64) -> ListChange {
    ListChange::Edit(EditCommand::Sequence { edit }, tick)
}

/// Typed cell text to commands.
pub struct ListEdit;

type Result<T> = std::result::Result<T, ListEditError>;

impl ListEdit {
    pub fn commit(
        text: &str,
        column: ListColumn,
        row: &ListRow,
        context: &ListContext,
    ) -> Result<ListChange> {
        let c = context;
        // Free text (text data, marker names) keeps its spaces; every other
        // cell is trimmed before it is parsed.
        let free = match &row.source {
            ListSource::Event { row: e, .. } => {
                e.kind == EventKindRow::Text && column == ListColumn::Data2
            }
            ListSource::Marker { .. } => column == ListColumn::Data2,
            _ => false,
        };
        let t = if free { text } else { trim_ws(text) };
        // Text equal to what the cell shows changes nothing, so an
        // off-grid event never moves when its unchanged position is
        // committed.
        let shown = ListText::cell(row, column, c);
        let shown = if free {
            shown.as_str()
        } else {
            trim_ws(&shown)
        };
        if t == shown {
            return Ok(ListChange::Unchanged);
        }
        match &row.source {
            ListSource::Event { row: e, .. } => Self::event(t, column, e, c),
            ListSource::Tempo { index, point } => Self::tempo(t, column, *index, point, c),
            ListSource::Meter { index, entry } => Self::meter(t, column, *index, entry, c),
            ListSource::Marker { index, marker } => Self::marker(t, column, *index, marker, c),
        }
    }

    fn position(t: &str, from: i64, c: &ListContext) -> Result<i64> {
        let current = Bbu::of(from, &c.meter);
        let Some(b) = parse_position(t, current) else {
            return refuse(format!(
                "\"{t}\" is not a position: type a bar, bar.beat or bar.beat.unit."
            ));
        };
        // The same position written differently keeps the exact tick.
        Ok(if b == current { from } else { b.tick(&c.meter) })
    }

    /// A typed SMPTE time as a tick, rounded to the nearest display unit.
    fn smpte(t: &str, c: &ListContext) -> Result<i64> {
        let tick = Smpte::parse(t, c.fps).and_then(|s| (c.tick_at_seconds)(s));
        let Some(tick) = tick else {
            return refuse(format!("\"{t}\" is not an SMPTE time (hh:mm:ss:ff)."));
        };
        let unit = (c.beat(tick) / i64::from(UNITS_PER_BEAT)).max(1);
        let start = bar_at(tick, &c.meter).start;
        Ok(start + ((tick - start + unit / 2) / unit) * unit)
    }

    fn number(t: &str, lo: i64, hi: i64, what: &str) -> Result<i64> {
        match swift_int(t) {
            Some(n) if (lo..=hi).contains(&n) => Ok(n),
            _ => refuse(format!("{what} must be {lo} to {hi}.")),
        }
    }

    fn replace(e: &EventRow, change: impl FnOnce(&mut EventRow)) -> ListChange {
        let mut n = e.clone();
        change(&mut n);
        if n == *e {
            return ListChange::Unchanged;
        }
        let tick = n.tick;
        event_edit(
            EventEdit::Replace {
                event: e.id,
                row: n,
            },
            tick,
        )
    }

    fn event(t: &str, col: ListColumn, e: &EventRow, c: &ListContext) -> Result<ListChange> {
        match col {
            ListColumn::Position | ListColumn::Smpte => {
                let tick = if col == ListColumn::Position {
                    Self::position(t, e.tick, c)?
                } else {
                    Self::smpte(t, c)?
                };
                if tick == e.tick {
                    return Ok(ListChange::Unchanged);
                }
                Ok(event_edit(
                    EventEdit::Move {
                        events: vec![e.id],
                        delta: tick - e.tick,
                        to_lane: None,
                    },
                    tick,
                ))
            }
            ListColumn::End => {
                let end = Self::position(t, e.tick.saturating_add(e.duration), c)?;
                if end < e.tick {
                    return refuse("The end is before the start.");
                }
                Ok(Self::replace(e, |n| n.duration = end - e.tick))
            }
            ListColumn::Duration => {
                let beat = c.beat(e.tick);
                let Some(d) = DurationText::parse(t, e.duration, beat) else {
                    return refuse(format!(
                        "\"{t}\" is not a length: type beats.units, such as 1.480."
                    ));
                };
                if DurationText::text(d, beat) == DurationText::text(e.duration, beat) {
                    return Ok(ListChange::Unchanged);
                }
                Ok(Self::replace(e, |n| n.duration = d))
            }
            ListColumn::Release => {
                let v = Self::number(t, 0, 127, "Release velocity")?;
                if Velocity7::of(e.release_velocity) == v {
                    return Ok(ListChange::Unchanged);
                }
                Ok(Self::replace(e, |n| n.release_velocity = Velocity7::up(v)))
            }
            ListColumn::Mute => {
                let on = match t.to_lowercase().as_str() {
                    "m" | "y" | "yes" | "1" | "x" | "on" => true,
                    "" | "n" | "no" | "0" | "off" => false,
                    _ => return refuse("Type M to mute, or clear the cell."),
                };
                if on == e.mute {
                    return Ok(ListChange::Unchanged);
                }
                Ok(event_edit(
                    EventEdit::SetMute {
                        events: vec![e.id],
                        mute: on,
                    },
                    e.tick,
                ))
            }
            ListColumn::Dest => {
                let lower = t.to_lowercase();
                let dest = if t.is_empty()
                    || t == "-"
                    || t == "\u{2014}"
                    || lower == "lane"
                    || lower == "track"
                {
                    None
                } else if let Some((id, _)) =
                    c.dest_names.iter().find(|(_, n)| n.to_lowercase() == lower)
                {
                    Some(*id)
                } else {
                    return refuse(format!("No destination is named \"{t}\"."));
                };
                if dest == e.dest {
                    return Ok(ListChange::Unchanged);
                }
                Ok(event_edit(
                    EventEdit::SetDest {
                        events: vec![e.id],
                        dest,
                    },
                    e.tick,
                ))
            }
            ListColumn::Data1 => Self::data1(t, e),
            ListColumn::Data2 => Self::data2(t, e),
            ListColumn::Kind | ListColumn::Lane => Ok(ListChange::Unchanged),
        }
    }

    fn data1(t: &str, e: &EventRow) -> Result<ListChange> {
        match e.kind {
            EventKindRow::Note | EventKindRow::PolyPressure => {
                let Some(p) = PitchName::parse(t) else {
                    return refuse(format!(
                        "\"{t}\" is not a pitch: type a name such as C4 or a number 0 to 127."
                    ));
                };
                Ok(Self::replace(e, |n| n.pitch = p))
            }
            EventKindRow::Cc => {
                let v = Self::number(t, 0, 127, "The controller number")?;
                Ok(Self::replace(e, |n| n.value2 = v))
            }
            EventKindRow::Program => {
                let v = Self::number(t, 0, 127, "The program")?;
                Ok(Self::replace(e, |n| n.value = v))
            }
            EventKindRow::Text => {
                let lower = t.to_lowercase();
                let Some(k) = ListText::TEXT_KINDS
                    .into_iter()
                    .find(|k| ListText::text_title(Some(*k)).to_lowercase() == lower)
                else {
                    return refuse("Type Text, Lyric, Cue, Marker Text or Copyright.");
                };
                Ok(Self::replace(e, |n| n.text_kind = Some(k)))
            }
            _ => Ok(ListChange::Unchanged),
        }
    }

    fn data2(t: &str, e: &EventRow) -> Result<ListChange> {
        match e.kind {
            EventKindRow::Note => {
                let v = Self::number(t, 0, 127, "Velocity")?;
                if Velocity7::of(e.velocity) == v {
                    return Ok(ListChange::Unchanged);
                }
                Ok(Self::replace(e, |n| n.velocity = Velocity7::up(v)))
            }
            EventKindRow::Cc | EventKindRow::ChannelPressure | EventKindRow::PolyPressure => {
                let v = Self::number(t, 0, 127, "The value")?;
                if Value7::of(e.value) == v {
                    return Ok(ListChange::Unchanged);
                }
                Ok(Self::replace(e, |n| n.value = i64::from(Value7::up(v))))
            }
            EventKindRow::PitchBend => {
                let v = Self::number(t, -8192, 8191, "Pitch bend")?;
                if Bend14::of(e.value) == v {
                    return Ok(ListChange::Unchanged);
                }
                Ok(Self::replace(e, |n| n.value = i64::from(Bend14::up(v))))
            }
            EventKindRow::Program => {
                if t.is_empty() || t == "-" {
                    return Ok(Self::replace(e, |n| n.value2 = -1));
                }
                let parts: Vec<Option<i64>> = t
                    .split(':')
                    .filter(|p| !p.is_empty())
                    .map(|p| swift_int(trim_ws(p)))
                    .collect();
                match parts.as_slice() {
                    [Some(m), Some(l)] if (0..=127).contains(m) && (0..=127).contains(l) => {
                        let bank = m * 128 + l;
                        Ok(Self::replace(e, |n| n.value2 = bank))
                    }
                    _ => refuse("Type the bank as msb:lsb, such as 0:5, or clear it."),
                }
            }
            EventKindRow::Text => Ok(Self::replace(e, |n| n.text = Some(t.to_owned()))),
            EventKindRow::Reference => {
                let refusal = "Type repeats and transpose, such as \u{d7}3 +2 or fill 0.";
                let words: Vec<&str> = t.split(' ').filter(|w| !w.is_empty()).collect();
                if !(1..=2).contains(&words.len()) {
                    return refuse(refusal);
                }
                let first = words[0].to_lowercase();
                let loops: u32 = if first == "fill" {
                    0
                } else {
                    match swift_uint(&first.replace(['\u{d7}', 'x'], "")) {
                        Some(n) if (1..=10_000).contains(&n) => n as u32,
                        _ => return refuse(refusal),
                    }
                };
                let mut transpose = e.ref_transpose;
                if let Some(w) = words.get(1) {
                    match swift_int(&w.replace('+', "")) {
                        Some(tr) if (-127..=127).contains(&tr) => transpose = tr as i8,
                        _ => return refuse(refusal),
                    }
                }
                Ok(Self::replace(e, |n| {
                    n.ref_loops = loops;
                    n.ref_transpose = transpose;
                }))
            }
            EventKindRow::AudioRegion => {
                let lower = t.to_lowercase().replace("db", "");
                match trim_ws(&lower).parse::<f32>() {
                    Ok(g) if g.is_finite() && (-144.0..=24.0).contains(&g) => {
                        Ok(Self::replace(e, |n| n.gain_db = g))
                    }
                    _ => refuse("Gain must be -144 to 24 dB."),
                }
            }
            // Swift `ListEdit.swift:198-208`.
            EventKindRow::Automation => {
                let Some(a) = e.automation else {
                    return Ok(ListChange::Unchanged);
                };
                let Some(v) = parse_percent(t) else {
                    return refuse("Type a value from 0 to 100 %.");
                };
                if v == a.value {
                    return Ok(ListChange::Unchanged);
                }
                Ok(Self::replace(e, |n| {
                    n.automation = Some(AutomationRow {
                        target: a.target,
                        value: v,
                    });
                    n.value = v32(v);
                }))
            }
            _ => Ok(ListChange::Unchanged),
        }
    }

    fn tempo(
        t: &str,
        col: ListColumn,
        i: usize,
        p: &TempoPointRow,
        c: &ListContext,
    ) -> Result<ListChange> {
        let Some(mut points) = c.info.as_ref().map(|info| info.tempo.clone()) else {
            return Ok(ListChange::Unchanged);
        };
        if i >= points.len() {
            return Ok(ListChange::Unchanged);
        }
        let mut n = *p;
        match col {
            ListColumn::Position => {
                if i == 0 {
                    return Ok(ListChange::Unchanged);
                }
                n.tick = Self::position(t, p.tick, c)?;
                let clash = points
                    .iter()
                    .enumerate()
                    .any(|(j, q)| j != i && q.tick == n.tick);
                if n.tick <= 0 || clash {
                    return refuse("A tempo change is already at that position.");
                }
            }
            ListColumn::Data1 => {
                let lower = t.to_lowercase().replace("bpm", "");
                match trim_ws(&lower).parse::<f64>() {
                    Ok(bpm) if bpm.is_finite() && (5.0..=999.0).contains(&bpm) => n.bpm = bpm,
                    _ => return refuse("Tempo must be 5 to 999 BPM."),
                }
            }
            ListColumn::Data2 => match t.to_lowercase().as_str() {
                "ramp" => n.ramp = true,
                "jump" | "" => n.ramp = false,
                _ => return refuse("Type ramp or jump."),
            },
            _ => return Ok(ListChange::Unchanged),
        }
        if n == *p {
            return Ok(ListChange::Unchanged);
        }
        points[i] = n;
        points.sort_by_key(|q| q.tick);
        Ok(sequence_edit(
            SequenceEdit::SetTempo {
                seq: c.seq,
                tempo: points,
            },
            n.tick,
        ))
    }

    fn meter(
        t: &str,
        col: ListColumn,
        i: usize,
        m: &MeterEntryRow,
        c: &ListContext,
    ) -> Result<ListChange> {
        let Some(mut entries) = c.info.as_ref().map(|info| info.meter.clone()) else {
            return Ok(ListChange::Unchanged);
        };
        if i >= entries.len() {
            return Ok(ListChange::Unchanged);
        }
        let mut n = *m;
        match col {
            ListColumn::Position => {
                if i == 0 {
                    return Ok(ListChange::Unchanged);
                }
                n.tick = Self::position(t, m.tick, c)?;
                let clash = entries
                    .iter()
                    .enumerate()
                    .any(|(j, q)| j != i && q.tick == n.tick);
                if n.tick <= 0 || clash {
                    return refuse("A meter change is already at that position.");
                }
            }
            ListColumn::Data1 => {
                // As Swift's `compactMap`: parts that are not numbers drop out.
                let parts: Vec<u8> = t
                    .split('/')
                    .filter(|p| !p.is_empty())
                    .filter_map(|p| swift_uint(trim_ws(p)).and_then(|v| u8::try_from(v).ok()))
                    .collect();
                match parts.as_slice() {
                    [num, den]
                        if (1..=64).contains(num) && [1, 2, 4, 8, 16, 32, 64].contains(den) =>
                    {
                        n.num = *num;
                        n.den = *den;
                    }
                    _ => return refuse("Type the meter as beats/unit, such as 6/8."),
                }
            }
            ListColumn::Data2 => {
                let Some((sharps, minor)) = KeyName::parse(t) else {
                    return refuse("Type a key such as D major or F# minor.");
                };
                n.sharps = sharps;
                n.minor = minor;
            }
            _ => return Ok(ListChange::Unchanged),
        }
        if n == *m {
            return Ok(ListChange::Unchanged);
        }
        entries[i] = n;
        entries.sort_by_key(|q| q.tick);
        // The core needs every meter change on a barline of the meters
        // before it.
        for j in 1..entries.len() {
            if bar_at(entries[j].tick, &entries[..j]).start != entries[j].tick {
                return refuse(if col == ListColumn::Position {
                    "Meter changes must start on a barline."
                } else {
                    "That meter would move a later meter change off its barline."
                });
            }
        }
        Ok(sequence_edit(
            SequenceEdit::SetMeter {
                seq: c.seq,
                meter: entries,
            },
            n.tick,
        ))
    }

    fn marker(
        t: &str,
        col: ListColumn,
        i: usize,
        m: &MarkerRow,
        c: &ListContext,
    ) -> Result<ListChange> {
        let Some(mut markers) = c.info.as_ref().map(|info| info.markers.clone()) else {
            return Ok(ListChange::Unchanged);
        };
        if i >= markers.len() {
            return Ok(ListChange::Unchanged);
        }
        let mut n = m.clone();
        match col {
            ListColumn::Position => n.tick = Self::position(t, m.tick, c)?,
            ListColumn::Data2 => {
                if trim_ws(t).is_empty() {
                    return refuse("A marker needs a name.");
                }
                n.name = t.to_owned();
            }
            _ => return Ok(ListChange::Unchanged),
        }
        if n == *m {
            return Ok(ListChange::Unchanged);
        }
        let tick = n.tick;
        markers[i] = n;
        markers.sort_by_key(|q| q.tick);
        Ok(sequence_edit(
            SequenceEdit::SetMarkers {
                seq: c.seq,
                markers,
            },
            tick,
        ))
    }
}

/// New rows.
pub struct ListInsert;

impl ListInsert {
    pub fn blank_row(kind: EventKindRow, tick: i64) -> EventRow {
        EventRow::blank(0, tick, kind)
    }

    /// A new event row of `kind` with its defaults; `None` for
    /// references, regions, automation and map kinds.
    pub fn row(kind: ListKind, tick: i64) -> Option<EventRow> {
        let mut r = Self::blank_row(kind.event_kind()?, tick);
        match kind {
            ListKind::Note => {
                r.pitch = 60;
                r.velocity = Velocity7::up(100);
                r.duration = PPQ;
            }
            ListKind::Cc => {
                r.value2 = 7;
                r.value = i64::from(Value7::up(64));
            }
            ListKind::PitchBend => r.value = i64::from(Bend14::up(0)),
            ListKind::ChannelPressure => r.value = 0,
            ListKind::PolyPressure => r.pitch = 60,
            ListKind::Program => r.value2 = -1,
            ListKind::SysEx => r.data = vec![0x7E, 0x7F, 0x09, 0x01],
            ListKind::Text => {
                r.text = Some("Text".into());
                r.text_kind = Some(vizdaw_ffi::events::TextKindRow::Text);
            }
            _ => return None,
        }
        Some(r)
    }

    /// The command inserting `kind` at `tick`: into `lane` for events,
    /// into sequence `seq`'s maps otherwise; `None` when it cannot be
    /// inserted here.
    pub fn command(
        kind: ListKind,
        tick: i64,
        lane: Option<LaneRef>,
        seq: u64,
        info: Option<&SequenceInfo>,
    ) -> Option<EditCommand> {
        let edit = match kind {
            ListKind::Reference | ListKind::AudioRegion => return None,
            ListKind::Tempo => {
                let mut points = info?.tempo.clone();
                let bpm = points
                    .iter()
                    .rev()
                    .find(|p| p.tick <= tick)
                    .map_or(120., |p| p.bpm);
                points.retain(|p| p.tick != tick);
                points.push(TempoPointRow {
                    tick,
                    bpm,
                    ramp: false,
                });
                points.sort_by_key(|p| p.tick);
                SequenceEdit::SetTempo { seq, tempo: points }
            }
            ListKind::Meter => {
                let mut entries = info?.meter.clone();
                let bar = bar_at(tick, &entries);
                let at = if tick == bar.start {
                    tick
                } else {
                    bar.start + bar.length
                };
                if entries.iter().any(|e| e.tick == at) {
                    return None;
                }
                entries.push(MeterEntryRow {
                    tick: at,
                    ..bar.meter
                });
                entries.sort_by_key(|e| e.tick);
                SequenceEdit::SetMeter {
                    seq,
                    meter: entries,
                }
            }
            ListKind::Marker => {
                let mut markers = info.map(|i| i.markers.clone()).unwrap_or_default();
                let name = format!("Marker {}", markers.len() + 1);
                markers.push(MarkerRow { tick, name });
                markers.sort_by_key(|m| m.tick);
                SequenceEdit::SetMarkers { seq, markers }
            }
            _ => {
                let lane = lane?;
                let r = Self::row(kind, tick)?;
                return Some(EditCommand::Event {
                    edit: EventEdit::Insert {
                        seq: lane.seq,
                        lane: lane.lane,
                        events: vec![r],
                    },
                });
            }
        };
        Some(EditCommand::Sequence { edit })
    }
}

/// Deleting rows: one command per kind of row (events, tempo, meter,
/// markers); the first tempo point and meter entry stay.
pub struct ListDelete;

impl ListDelete {
    pub fn commands(rows: &[ListRow], seq: u64, info: Option<&SequenceInfo>) -> Vec<EditCommand> {
        let mut out = vec![];
        let events: Vec<u64> = rows
            .iter()
            .filter_map(|r| r.event())
            .map(|e| e.id)
            .collect();
        if !events.is_empty() {
            out.push(EditCommand::Event {
                edit: EventEdit::Delete { events },
            });
        }
        let Some(info) = info else { return out };
        let mut tempo = std::collections::BTreeSet::new();
        let mut meter = std::collections::BTreeSet::new();
        let mut markers = std::collections::BTreeSet::new();
        for r in rows {
            match r.source {
                ListSource::Tempo { index, .. } if index > 0 => {
                    tempo.insert(index);
                }
                ListSource::Meter { index, .. } if index > 0 => {
                    meter.insert(index);
                }
                ListSource::Marker { index, .. } => {
                    markers.insert(index);
                }
                _ => {}
            }
        }
        fn kept<T: Clone>(all: &[T], gone: &std::collections::BTreeSet<usize>) -> Vec<T> {
            all.iter()
                .enumerate()
                .filter(|(i, _)| !gone.contains(i))
                .map(|(_, v)| v.clone())
                .collect()
        }
        if !tempo.is_empty() {
            out.push(EditCommand::Sequence {
                edit: SequenceEdit::SetTempo {
                    seq,
                    tempo: kept(&info.tempo, &tempo),
                },
            });
        }
        if !meter.is_empty() {
            out.push(EditCommand::Sequence {
                edit: SequenceEdit::SetMeter {
                    seq,
                    meter: kept(&info.meter, &meter),
                },
            });
        }
        if !markers.is_empty() {
            out.push(EditCommand::Sequence {
                edit: SequenceEdit::SetMarkers {
                    seq,
                    markers: kept(&info.markers, &markers),
                },
            });
        }
        out
    }
}
