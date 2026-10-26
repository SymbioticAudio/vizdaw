//! Rules the model enforces: no reference cycles, referential integrity and
//! value ranges. Commands call the `check_*` functions; loading calls
//! [`validate`], which reports every problem instead of stopping at the first.

use crate::document::{DestKind, Document, MAX_RETURN_LATENCY_MS};
use crate::error::{ModelError, Result};
use crate::event::{EventBody, EventSpec, LoopCount};
use crate::ids::{AnyId, EventId, SeqId};
use crate::lane::LaneInput;
use crate::time::Tick;
use std::collections::{BTreeMap, BTreeSet};

/// The sequences directly referenced from `seq` (any take).
pub fn direct_refs(doc: &Document, seq: SeqId) -> BTreeSet<SeqId> {
    let mut out = BTreeSet::new();
    if let Ok(s) = doc.sequence(seq) {
        for l in &s.lanes {
            for t in &l.takes {
                for e in t.events.values() {
                    if let EventBody::Reference { seq: target, .. } = e.body {
                        out.insert(target);
                    }
                }
            }
        }
    }
    out
}

/// A reference path from `from` to `to` (inclusive), if one exists.
pub fn path_between(doc: &Document, from: SeqId, to: SeqId) -> Option<Vec<SeqId>> {
    let mut parent: BTreeMap<SeqId, SeqId> = BTreeMap::new();
    let mut seen = BTreeSet::from([from]);
    let mut stack = vec![from];
    while let Some(cur) = stack.pop() {
        if cur == to {
            let mut path = vec![to];
            let mut c = to;
            while let Some(p) = parent.get(&c) {
                path.push(*p);
                c = *p;
            }
            path.reverse();
            return Some(path);
        }
        for next in direct_refs(doc, cur) {
            if seen.insert(next) {
                parent.insert(next, cur);
                stack.push(next);
            }
        }
    }
    None
}

/// Refuses a reference from `owner` to `target` that would close a cycle.
pub fn check_reference(doc: &Document, owner: SeqId, target: SeqId) -> Result<()> {
    doc.sequence(target)?;
    if owner == target {
        return Err(ModelError::Cycle {
            path: vec![owner, owner],
        });
    }
    if let Some(back) = path_between(doc, target, owner) {
        let mut path = vec![owner];
        path.extend(back);
        return Err(ModelError::Cycle { path });
    }
    Ok(())
}

/// Every reference event (any take) that targets `seq`.
pub fn references_to(doc: &Document, seq: SeqId) -> Vec<EventId> {
    doc.all_events()
        .filter(|(_, e)| matches!(e.body, EventBody::Reference { seq: t, .. } if t == seq))
        .map(|(_, e)| e.id)
        .collect()
}

/// Any reference cycle in the document.
pub fn find_cycle(doc: &Document) -> Option<Vec<SeqId>> {
    for &s in doc.sequences().keys() {
        for t in direct_refs(doc, s) {
            if t == s {
                return Some(vec![s, s]);
            }
            if let Some(back) = path_between(doc, t, s) {
                let mut path = vec![s];
                path.extend(back);
                return Some(path);
            }
        }
    }
    None
}

/// The furthest position and the longest length a document may hold:
/// 2^40 ticks (about 400 days at 120 BPM). Beyond it the editors' and the
/// engine's time arithmetic could overflow, so a damaged file or a
/// hostile edit holding such a value is refused (control spec §11).
pub const MAX_TICKS: i64 = 1 << 40;

/// The most loops a reference may play.
pub const MAX_LOOPS: u32 = 1 << 20;

/// Checks that `t` is a position or length within `±MAX_TICKS`.
pub fn check_ticks(field: &'static str, t: i64) -> Result<()> {
    need((-MAX_TICKS..=MAX_TICKS).contains(&t), field, t)
}

fn need(cond: bool, field: &'static str, value: impl std::fmt::Display) -> Result<()> {
    if cond {
        Ok(())
    } else {
        Err(ModelError::range(field, value))
    }
}

/// Checks a media path: relative to the package, normalized (no empty or
/// `.` components, no trailing slash), no `..`, no backslashes, no drive or
/// absolute root.
pub fn check_media_path(path: &str) -> Result<()> {
    let ok = !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|c| !matches!(c, "" | "." | ".."));
    if ok {
        Ok(())
    } else {
        Err(ModelError::invalid(format!(
            "media path must be relative and normalized: {path:?}"
        )))
    }
}

/// Checks a group/channel pair (each 0–15).
pub fn check_group_channel(gc: Option<(u8, u8)>) -> Result<()> {
    if let Some((g, c)) = gc {
        need(g <= 15, "group", g)?;
        need(c <= 15, "channel", c)?;
    }
    Ok(())
}

/// Checks an event's ranges and IDs, but not reference cycles.
pub fn check_event_fields(doc: &Document, ev: &EventSpec) -> Result<()> {
    if let Some(d) = ev.dest {
        if !doc.instruments().contains_key(&d) {
            return Err(ModelError::UnknownId(AnyId::Dest(d)));
        }
    }
    check_group_channel(ev.group_channel)?;
    check_ticks("tick", ev.tick.0)?;
    match &ev.body {
        EventBody::Note {
            pitch, duration, ..
        } => {
            need(*pitch <= 127, "pitch", pitch)?;
            need(duration.0 >= 0, "duration", duration.0)?;
            check_ticks("duration", duration.0)?;
        }
        EventBody::Cc { index, .. } => need(*index <= 127, "cc.index", index)?,
        EventBody::PolyPressure { pitch, .. } => need(*pitch <= 127, "pitch", pitch)?,
        EventBody::Program { program, bank } => {
            need(*program <= 127, "program", program)?;
            if let Some((m, l)) = bank {
                need(*m <= 127 && *l <= 127, "bank", format!("{m}/{l}"))?;
            }
        }
        EventBody::SysEx { data, eight_bit } => {
            need(
                *eight_bit || data.iter().all(|b| *b < 0x80),
                "sysex.data",
                "8-bit byte in 7-bit sysex",
            )?;
        }
        EventBody::Reference {
            seq,
            length,
            loop_count,
            ..
        } => {
            doc.sequence(*seq)?;
            if let Some(l) = length {
                need(l.0 > 0, "reference.length", l.0)?;
                check_ticks("reference.length", l.0)?;
            }
            if let LoopCount::Count(n) = loop_count {
                need((1..=MAX_LOOPS).contains(n), "reference.loop_count", n)?;
            }
        }
        EventBody::AudioRegion {
            media,
            source_offset,
            length,
            gain_db,
            fade_in,
            fade_out,
        } => {
            let m = doc
                .media()
                .get(media)
                .ok_or(ModelError::UnknownId(AnyId::Media(*media)))?;
            need(*length >= 1, "region.length", length)?;
            need(
                source_offset.saturating_add(*length) <= m.length_frames,
                "region.source_end",
                source_offset.saturating_add(*length),
            )?;
            need(
                gain_db.is_finite() && (-144.0..=24.0).contains(gain_db),
                "region.gain_db",
                gain_db,
            )?;
            let fades = fade_in.length_frames.saturating_add(fade_out.length_frames);
            need(fades <= *length, "region.fades", fades)?;
        }
        EventBody::Automation { value, .. } => need(
            crate::automation::check_value(*value),
            "automation.value",
            value,
        )?,
        EventBody::PitchBend { .. }
        | EventBody::ChannelPressure { .. }
        | EventBody::Text { .. } => {}
    }
    Ok(())
}

/// Full check for inserting `ev` into sequence `owner`, including cycles.
pub fn check_event(doc: &Document, owner: SeqId, ev: &EventSpec) -> Result<()> {
    check_event_fields(doc, ev)?;
    if let EventBody::Reference { seq, .. } = ev.body {
        check_reference(doc, owner, seq)?;
    }
    Ok(())
}

/// Checks that a length in ticks is greater than zero.
pub fn check_positive(field: &'static str, t: Tick) -> Result<()> {
    need(t.0 > 0, field, t.0)
}

/// Runs every integrity check and returns all problems found (empty when
/// the document is valid).
pub fn validate(doc: &Document) -> Vec<String> {
    let mut p: Vec<String> = Vec::new();
    let c = doc.counters();
    let mut push = |r: Result<()>, ctx: &str| {
        if let Err(e) = r {
            p.push(format!("{ctx}: {e}"));
        }
    };
    for d in doc.instruments().values() {
        if d.id.0 >= c.dest {
            push(Err(ModelError::range("counters.dest", d.id)), "destination");
        }
        if let DestKind::MidiEndpoint { group, channel, .. } = d.kind {
            push(
                check_group_channel(Some((group, channel))),
                &d.id.to_string(),
            );
        }
        if let Some(patch) = d.default_patch {
            let ok = patch.program <= 127 && patch.bank.is_none_or(|(m, l)| m <= 127 && l <= 127);
            push(need(ok, "default_patch", patch.program), &d.id.to_string());
        }
        let ms = d.return_latency_ms;
        push(
            need(
                ms.is_finite() && (0.0..=MAX_RETURN_LATENCY_MS).contains(&ms),
                "return_latency_ms",
                ms,
            ),
            &d.id.to_string(),
        );
    }
    for o in doc.outputs().values() {
        if o.id.0 >= c.output {
            push(Err(ModelError::range("counters.output", o.id)), "output");
        }
    }
    let f = doc.settings().format;
    push(
        need(f.is_valid(), "settings.format", f.sample_rate),
        "settings",
    );
    for m in doc.media().values() {
        if m.id.0 >= c.media {
            push(Err(ModelError::range("counters.media", m.id)), "media");
        }
        push(check_media_path(&m.path), &m.id.to_string());
        push(
            need(
                m.sample_rate > 0 && m.channels > 0,
                "media.format",
                m.sample_rate,
            ),
            &m.id.to_string(),
        );
    }
    let mut lane_ids = BTreeSet::new();
    let mut take_ids = BTreeSet::new();
    let mut row_ids = BTreeSet::new();
    let mut slots = BTreeSet::new();
    for s in doc.sequences().values() {
        let ctx = s.id.to_string();
        if s.id.0 >= c.seq {
            push(Err(ModelError::range("counters.seq", s.id)), &ctx);
        }
        push(s.tempo.check(), &ctx);
        push(s.meter.check(), &ctx);
        push(
            need(s.start_offset.0 >= 0, "start_offset", s.start_offset.0),
            &ctx,
        );
        push(check_ticks("start_offset", s.start_offset.0), &ctx);
        if let crate::sequence::SeqLength::Locked(t) = s.length {
            push(check_positive("length", t), &ctx);
            push(check_ticks("length", t.0), &ctx);
        }
        if let Some(k) = &s.keyboard_slot {
            if !slots.insert(k.clone()) {
                push(Err(ModelError::invalid("duplicate keyboard slot")), &ctx);
            }
        }
        for l in &s.lanes {
            let lctx = format!("{ctx}/{}", l.id);
            if l.id.0 >= c.lane || !lane_ids.insert(l.id) {
                push(Err(ModelError::range("lane.id", l.id)), &lctx);
            }
            if let Some(d) = l.midi_dest {
                if !doc.instruments().contains_key(&d) {
                    push(Err(ModelError::UnknownId(AnyId::Dest(d))), &lctx);
                }
            }
            if let Some(o) = l.audio_out {
                if !doc.outputs().contains_key(&o) {
                    push(Err(ModelError::UnknownId(AnyId::Output(o))), &lctx);
                }
            }
            if let Some(len) = l.length {
                push(check_positive("lane.length", len), &lctx);
                push(check_ticks("lane.length", len.0), &lctx);
            }
            if let Some(q) = &l.play_quantize {
                push(q.check(), &lctx);
            }
            if let Some(i) = &l.input {
                push(i.check(), &lctx);
            }
            // Plan 5c-2 Ruling 3: a synth's return names a destination.
            if let Some(dest) = l.input.as_ref().and_then(LaneInput::return_of)
                && !doc.instruments().contains_key(&dest)
            {
                push(Err(ModelError::UnknownId(AnyId::Dest(dest))), &lctx);
            }
            if l.take(l.active_take).is_none() {
                push(
                    Err(ModelError::UnknownId(AnyId::Take(l.active_take))),
                    &lctx,
                );
            }
            if let Some(drum) = &l.drum {
                for r in &drum.rows {
                    if r.id.0 >= c.drum_row || !row_ids.insert(r.id) {
                        push(Err(ModelError::range("drum_row.id", r.id)), &lctx);
                    }
                    push(need(r.note <= 127, "drum_row.note", r.note), &lctx);
                    if let Some(d) = r.dest {
                        if !doc.instruments().contains_key(&d) {
                            push(Err(ModelError::UnknownId(AnyId::Dest(d))), &lctx);
                        }
                    }
                }
            }
            for t in &l.takes {
                if t.id.0 >= c.take || !take_ids.insert(t.id) {
                    push(Err(ModelError::range("take.id", t.id)), &lctx);
                }
                for e in t.events.values() {
                    let ectx = format!("{lctx}/{}", e.id);
                    if e.id.0 >= c.event {
                        push(Err(ModelError::range("counters.event", e.id)), &ectx);
                    }
                    push(check_event_fields(doc, &e.spec()), &ectx);
                }
            }
        }
    }
    if let Some(path) = find_cycle(doc) {
        p.push(ModelError::Cycle { path }.to_string());
    }
    p.extend(crate::mixer_rules::mixer_problems(doc));
    p
}
