//! Notes with their expression (Ruling 12) and controller points
//! (Ruling 13), inverted.

use super::*;

impl<'p> Tl<'_, '_, 'p> {
    pub(super) fn notes(&mut self, n: &'p DpNotes, f: &Frame, to: To) -> Result<(), IoError> {
        for note in &n.notes {
            self.tick()?;
            if !self.spend() {
                return Ok(());
            }
            let pitch = u8::try_from(note.key).ok().filter(|k| *k <= 127);
            let ch = u8::try_from(note.channel).ok().filter(|c| *c <= 15);
            let vel = note.vel.unwrap_or(DEFAULT_VEL);
            let rel = note.rel.unwrap_or(0.0);
            let (Some(pitch), Some(ch)) = (pitch, ch) else {
                self.left_out(BAD_NOTES);
                continue;
            };
            if !(note.duration.is_finite() && note.duration >= 0.0) {
                self.left_out(BAD_NOTES);
                continue;
            }
            let Some(start) = self.start_at(f, note.time) else {
                continue;
            };
            let Some(mut end) = self.at(f, note.time + note.duration) else {
                self.left_out(BAD_NOTES);
                continue;
            };
            let cut = end > f.hi;
            if cut {
                end = f.hi;
                self.count(ItemKind::Changed, NOTES_CUT);
            }
            let duration = end.saturating_sub(start).max(0);
            let scale = |tl: &mut Self, v: f64| -> u16 {
                let c = v.clamp(0.0, 1.0);
                if c != v {
                    tl.count(ItemKind::Changed, VEL_CLAMPED);
                }
                (c * 65_535.0).round() as u16
            };
            let velocity = scale(self, vel);
            let release_velocity = scale(self, rel);
            let mut per_note: Vec<PerNoteCtl> = Vec::new();
            if let Some(content) = note.content.as_deref() {
                self.expression(
                    content,
                    f,
                    note,
                    start,
                    (duration, cut),
                    (pitch, ch),
                    &mut per_note,
                    to,
                )?;
            }
            per_note.sort_by_key(|c| c.offset);
            self.put(
                to,
                Ev {
                    tick: start,
                    ch,
                    mute: f.muted,
                    tag: f.tag,
                    body: EventBody::Note {
                        pitch,
                        velocity,
                        release_velocity,
                        duration: Tick(duration),
                        attribute: None,
                        per_note: per_note.into_iter().collect(),
                    },
                },
            );
        }
        Ok(())
    }

    /// A note's expression (Ruling 12): point times from the note's start.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn expression(
        &mut self,
        content: &'p DpTimeline,
        f: &Frame,
        note: &DpNote,
        start: i64,
        (duration, cut): (i64, bool),
        (pitch, ch): (u8, u8),
        out: &mut Vec<PerNoteCtl>,
        to: To,
    ) -> Result<(), IoError> {
        let mut stack: Vec<(&'p DpTimeline, Frame)> = vec![(content, *f)];
        while let Some((t, f)) = stack.pop() {
            self.tick()?;
            let f = f.with_unit(head_of(t).and_then(|h| h.time_unit));
            let pts = match t {
                DpTimeline::Lanes(l) => {
                    for i in l.items.iter().rev() {
                        stack.push((i, f));
                    }
                    continue;
                }
                DpTimeline::Points(p) => p,
                _ => {
                    self.left_out(OTHER_EXPRESSION);
                    continue;
                }
            };
            let kind = match pts.target.expression {
                Some(DpExpression::Transpose) => None,
                Some(DpExpression::Timbre) => Some(PerNoteKind::Registered(74)),
                Some(DpExpression::Gain) => Some(PerNoteKind::Registered(7)),
                Some(DpExpression::Pan) => Some(PerNoteKind::Registered(10)),
                Some(DpExpression::Pressure) => Some(PerNoteKind::Assignable(0)),
                Some(DpExpression::Formant) => {
                    self.left_out(FORMANT);
                    continue;
                }
                _ => {
                    self.left_out(OTHER_EXPRESSION);
                    continue;
                }
            };
            self.ramps(&pts.points);
            for pt in &pts.points {
                self.tick()?;
                if !self.spend() {
                    return Ok(());
                }
                let DpPointValue::Real(v, _) = pt.value else {
                    self.bad_value("note expression point");
                    continue;
                };
                let Some(at) = self.at(&f, note.time + pt.time) else {
                    self.bad_value("note expression point");
                    continue;
                };
                let offset = at.saturating_sub(start);
                if offset < 0 {
                    self.bad_value("note expression point");
                    continue;
                }
                // Past a cut note's end: cut with it (listed with the
                // note); past an uncut one's, left out.
                if offset > duration {
                    if !cut {
                        self.count(ItemKind::LeftOut, EXPRESSION_PAST_END);
                    }
                    continue;
                }
                match kind {
                    None => {
                        let c = v.clamp(-48.0, 48.0);
                        if c != v {
                            self.count(ItemKind::Changed, TRANSPOSE_CLAMPED);
                        }
                        const CENTRE: f64 = 2_147_483_648.0;
                        let value = (CENTRE + c * CENTRE / 48.0)
                            .round()
                            .clamp(0.0, f64::from(u32::MAX))
                            as u32;
                        out.push(PerNoteCtl {
                            kind: PerNoteKind::PitchBend,
                            offset: Tick(offset),
                            value,
                        });
                    }
                    Some(PerNoteKind::Assignable(_)) => {
                        let value = self.norm(v);
                        self.put(
                            to,
                            Ev {
                                tick: start.saturating_add(offset),
                                ch,
                                mute: f.muted,
                                tag: f.tag,
                                body: EventBody::PolyPressure { pitch, value },
                            },
                        );
                    }
                    Some(kind) => {
                        let value = self.norm(v);
                        out.push(PerNoteCtl {
                            kind,
                            offset: Tick(offset),
                            value,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// A normalised value as 32 bits; one outside 0 to 1 is clamped
    /// (Changed).
    pub(super) fn norm(&mut self, v: f64) -> u32 {
        if !(0.0..=1.0).contains(&v) {
            self.count(ItemKind::Changed, VALUES_CLAMPED);
        }
        to_u32(v)
    }

    /// Lists a linear ramp between points (MIDI keeps only the points).
    pub(super) fn ramps(&mut self, pts: &[DpPoint]) {
        let ramp = pts.split_last().is_some_and(|(_, rest)| {
            rest.iter().any(|p| {
                matches!(
                    p.value,
                    DpPointValue::Real(_, Some(DpInterpolation::Linear))
                )
            })
        });
        if ramp {
            self.cx.add(ItemKind::Changed, MIDI_RAMPS, None);
        }
    }

    /// Controller points (Ruling 13) or automation (Ruling 14).
    pub(super) fn points(&mut self, p: &'p DpPoints, f: &Frame, to: To) -> Result<(), IoError> {
        if let Some(param) = p.target.parameter.as_deref() {
            return match to {
                To::Pass => {
                    self.pass_other = true;
                    Ok(())
                }
                To::Track(track) => self.automation(p, param, f, track),
            };
        }
        let tg = &p.target;
        let ch = match tg.channel {
            None => None,
            Some(c) => match u8::try_from(c).ok().filter(|c| *c <= 15) {
                Some(c) => Some(c),
                None => {
                    self.bad_value("Target@channel");
                    return Ok(());
                }
            },
        };
        let seven = |v: Option<i32>| v.and_then(|v| u8::try_from(v).ok()).filter(|v| *v <= 127);
        enum K {
            Cc(u8),
            Bend,
            Chan,
            Poly(u8),
            Program,
        }
        let k = match tg.expression {
            Some(DpExpression::ChannelController) => match seven(tg.controller) {
                Some(n) => K::Cc(n),
                None => {
                    self.bad_value("Target@controller");
                    return Ok(());
                }
            },
            Some(DpExpression::PitchBend) => K::Bend,
            Some(DpExpression::ChannelPressure) => K::Chan,
            Some(DpExpression::PolyPressure) => match seven(tg.key) {
                Some(n) => K::Poly(n),
                None => {
                    self.bad_value("Target@key");
                    return Ok(());
                }
            },
            Some(DpExpression::ProgramChange) => K::Program,
            Some(DpExpression::Formant) => {
                self.left_out(FORMANT);
                return Ok(());
            }
            Some(_) => {
                self.left_out(OTHER_EXPRESSION);
                return Ok(());
            }
            None => {
                self.bad_value("Points target");
                return Ok(());
            }
        };
        self.ramps(&p.points);
        for pt in &p.points {
            self.tick()?;
            if !self.spend() {
                return Ok(());
            }
            let Some(tick) = self.start_at(f, pt.time) else {
                continue;
            };
            let body = match (&k, &pt.value) {
                (K::Program, DpPointValue::Integer(v) | DpPointValue::Enum(v)) => {
                    match seven(Some(*v)) {
                        Some(program) => EventBody::Program {
                            program,
                            bank: None,
                        },
                        None => {
                            self.bad_value("programChange point");
                            continue;
                        }
                    }
                }
                (K::Program, DpPointValue::Real(v, _)) if v.fract() == 0.0 => {
                    match (*v >= 0.0 && *v <= 127.0).then_some(*v as u8) {
                        Some(program) => EventBody::Program {
                            program,
                            bank: None,
                        },
                        None => {
                            self.bad_value("programChange point");
                            continue;
                        }
                    }
                }
                (K::Program, _) => {
                    self.bad_value("programChange point");
                    continue;
                }
                (_, DpPointValue::Real(v, _)) => {
                    let value = self.norm(*v);
                    match k {
                        K::Cc(index) => EventBody::Cc { index, value },
                        K::Bend => EventBody::PitchBend { value },
                        K::Chan => EventBody::ChannelPressure { value },
                        K::Poly(pitch) => EventBody::PolyPressure { pitch, value },
                        K::Program => continue,
                    }
                }
                _ => {
                    self.bad_value("controller point");
                    continue;
                }
            };
            self.put(
                to,
                Ev {
                    tick,
                    // No channel: the track's main one (filled in later).
                    ch: ch.unwrap_or(u8::MAX),
                    mute: f.muted,
                    tag: f.tag,
                    body,
                },
            );
        }
        Ok(())
    }
}

/// Joins each program with the bank select (CC 0 and 32) at its tick on
/// its channel (Ruling 13, inverted); the two controllers are dropped.
/// Each tick's controllers are indexed once by channel, so many programs
/// and controllers at one tick stay linear; `tick` is called once per
/// tick.
pub(super) fn pair_banks(
    evs: &mut Vec<Ev>,
    tick: &mut dyn FnMut() -> Result<(), IoError>,
) -> Result<(), IoError> {
    let open = |e: &Ev| matches!(e.body, EventBody::Program { bank: None, .. });
    if !evs.iter().any(open) {
        return Ok(());
    }
    evs.sort_by_key(|e| e.tick);
    let seven = |v: u32| (from_u32(v) * 127.0).round().clamp(0.0, 127.0) as u8;
    let mut drop = vec![false; evs.len()];
    // This tick's CC 0 and 32 by (channel, controller), in order, with
    // how many the programs before took.
    let mut ccs: HashMap<(u8, u8), (Vec<usize>, usize)> = HashMap::new();
    let mut from = 0;
    while from < evs.len() {
        tick()?;
        let at = evs.get(from).map_or(0, |e| e.tick);
        let to = from
            + evs
                .get(from..)
                .map_or(0, |r| r.partition_point(|e| e.tick == at));
        let to = to.max(from + 1);
        let group = evs.get(from..to).unwrap_or_default();
        if !group.iter().any(open) {
            from = to;
            continue;
        }
        ccs.clear();
        for (k, e) in (from..).zip(group) {
            if let EventBody::Cc {
                index: n @ (0 | 32),
                ..
            } = e.body
            {
                ccs.entry((e.ch, n)).or_default().0.push(k);
            }
        }
        let mut banks: Vec<(usize, (u8, u8), usize, usize)> = Vec::new();
        for (p, e) in (from..).zip(group) {
            if !open(e) {
                continue;
            }
            let next = |ccs: &HashMap<(u8, u8), (Vec<usize>, usize)>, n: u8| {
                ccs.get(&(e.ch, n))
                    .and_then(|(v, used)| v.get(*used).copied())
            };
            let (Some(m), Some(l)) = (next(&ccs, 0), next(&ccs, 32)) else {
                continue;
            };
            for n in [0, 32] {
                if let Some((_, used)) = ccs.get_mut(&(e.ch, n)) {
                    *used += 1;
                }
            }
            let value = |k: usize| match evs.get(k).map(|e| &e.body) {
                Some(EventBody::Cc { value, .. }) => seven(*value),
                _ => 0,
            };
            banks.push((p, (value(m), value(l)), m, l));
        }
        for (p, bank, m, l) in banks {
            for k in [m, l] {
                if let Some(d) = drop.get_mut(k) {
                    *d = true;
                }
            }
            if let Some(Ev {
                body: EventBody::Program { bank: b, .. },
                ..
            }) = evs.get_mut(p)
            {
                *b = Some(bank);
            }
        }
        from = to;
    }
    let mut k = 0;
    evs.retain(|_| {
        let keep = !drop.get(k).copied().unwrap_or(false);
        k += 1;
        keep
    });
    Ok(())
}
