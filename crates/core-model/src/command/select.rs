//! Select-and-modify.

use super::{Context, Report};
use crate::document::Document;
use crate::error::Result;
use crate::event::EventBody;
use crate::ids::{AnyId, EventId, LaneId, SeqId};
use crate::modify::{Modify, apply_modify};
use crate::rule::{Rule, SelectMode, events_matching, filter_events};
use crate::selection::Selection;
use crate::time::Tick;
use im::OrdSet;

/// Runs a select-and-modify and returns the new selection.
pub(crate) fn apply(
    d: &mut Document,
    ctx: Context<'_>,
    rule: Option<&Rule>,
    mode: SelectMode,
    modify: Option<&Modify>,
    to_copy: bool,
    r: &mut Report,
) -> Result<Selection> {
    let base = ctx.selection.retain_existing(d);
    let everything = Rule::All(Vec::new());
    let visible: OrdSet<EventId> =
        filter_events(d, base.events.iter().copied(), &everything, ctx.filter)
            .into_iter()
            .collect();
    let chosen: OrdSet<EventId> = match rule {
        None => visible,
        Some(rule) => match mode {
            SelectMode::Select => events_matching(d, &base.lanes, rule, ctx.filter)
                .into_iter()
                .collect(),
            SelectMode::Add => visible.union(
                events_matching(d, &base.lanes, rule, ctx.filter)
                    .into_iter()
                    .collect(),
            ),
            SelectMode::Refine => filter_events(d, visible.iter().copied(), rule, ctx.filter)
                .into_iter()
                .collect(),
        },
    };
    let mut targets: Vec<EventId> = chosen.iter().copied().collect();
    if let Some(m) = modify {
        if to_copy && matches!(m, Modify::Line(_)) {
            return Err(crate::error::ModelError::invalid(
                "a Strip Chart line cannot change copies",
            ));
        }
        // Deleting copies would leave the document as it was.
        if to_copy && matches!(m, Modify::Delete) {
            return Ok(Selection {
                time_range: base.time_range,
                lanes: base.lanes,
                events: targets.into_iter().collect(),
            });
        }
        if to_copy {
            let mut copies = Vec::with_capacity(targets.len());
            for id in &targets {
                let (ev, loc) = d.event(*id)?;
                let spec = ev.spec();
                let new_id = d.counters_mut().alloc_event();
                d.insert_event(loc.seq, loc.lane, Some(loc.take), spec.with_id(new_id))?;
                r.created.push(AnyId::Event(new_id));
                copies.push(new_id);
            }
            targets = copies;
        }
        targets = match m {
            // A tempo line changes no event: the selection stays what it
            // was, not every event under the line (final review Minor 2).
            Modify::Line(c) if c.target == crate::strip::LineTarget::Tempo => {
                crate::strip::apply_line(d, &base.lanes, &targets, c, r)?;
                base.events.iter().copied().collect()
            }
            // A line draws into the selection's lanes (Plan 4d Ruling 16).
            Modify::Line(c) => crate::strip::apply_line(d, &base.lanes, &targets, c, r)?,
            _ => apply_modify(d, &targets, m, r)?,
        };
    }
    Ok(Selection {
        time_range: base.time_range,
        lanes: base.lanes,
        events: targets.into_iter().collect(),
    })
}

/// A selection of the notes of `lanes`' active takes in `seq` that start
/// in `range` (`[start, end)`; `None`: all of them), for a `ModifyNotes`
/// (plan 8c Ruling 2). `Invalid` "Track <id> has no notes there." for
/// the first lane that is not in `seq` or has no such note.
pub(crate) fn notes_of(
    d: &Document,
    seq: SeqId,
    lanes: &[LaneId],
    range: Option<(Tick, Tick)>,
) -> Result<Selection> {
    let mut sel = Selection::default();
    for lane in lanes {
        let notes: Vec<EventId> = d
            .sequence(seq)
            .ok()
            .and_then(|s| s.lane(*lane))
            .and_then(|l| l.active())
            .map(|t| {
                t.events
                    .values()
                    .filter(|e| matches!(e.body, EventBody::Note { .. }))
                    .filter(|e| range.is_none_or(|(a, b)| e.tick >= a && e.tick < b))
                    .map(|e| e.id)
                    .collect()
            })
            .unwrap_or_default();
        if notes.is_empty() {
            return Err(crate::error::ModelError::invalid(format!(
                "Track {} has no notes there.",
                lane.0
            )));
        }
        sel.lanes.insert((seq, *lane));
        sel.events.extend(notes);
    }
    Ok(sel)
}
