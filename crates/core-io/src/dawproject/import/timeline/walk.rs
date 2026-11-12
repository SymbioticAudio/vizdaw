//! The walk: indexes, frames, placing clips and their passes, and
//! flattening a timeline onto a track or into a pass.

use super::*;

impl<'p> Tl<'_, '_, 'p> {
    // ---- indexes

    /// Every timeline with an ID, wherever it is; sets the budget from the
    /// parts the file holds.
    pub(super) fn index(&mut self) -> Result<(), IoError> {
        let mut parts = 0usize;
        let p = &self.cx.pkg.project;
        let mut stack: Vec<&'p DpTimeline> = Vec::new();
        for s in p.scenes.iter().rev() {
            stack.extend(&s.content);
        }
        if let Some(l) = p.arrangement.as_ref().and_then(|a| a.lanes.as_ref()) {
            stack.extend(l.items.iter().rev());
        }
        while let Some(t) = stack.pop() {
            self.tick()?;
            if let Some(id) = head_of(t).and_then(|h| h.named.id.as_deref()) {
                self.index.insert(id, t);
            }
            parts = parts.saturating_add(1);
            match t {
                DpTimeline::Lanes(l) => stack.extend(l.items.iter().rev()),
                DpTimeline::Clips(c) => {
                    parts = parts.saturating_add(c.clips.len());
                    for clip in c.clips.iter().rev() {
                        self.referenced.extend(clip.reference.as_deref());
                        stack.extend(clip.content.as_deref());
                    }
                }
                DpTimeline::Notes(n) => {
                    parts = parts.saturating_add(n.notes.len());
                    for note in n.notes.iter().rev() {
                        stack.extend(note.content.as_deref());
                    }
                }
                DpTimeline::Points(p) => parts = parts.saturating_add(p.points.len()),
                DpTimeline::Warps(w) => stack.push(&w.content),
                DpTimeline::ClipSlot(_, Some(clip)) => {
                    self.referenced.extend(clip.reference.as_deref());
                    stack.extend(clip.content.as_deref());
                }
                _ => {}
            }
        }
        self.budget = EXPANSION
            .saturating_mul(parts)
            .saturating_add(EXPANSION_FLOOR)
            .min(Limits::default().max_events);
        Ok(())
    }

    // ---- the walk

    /// A track ID as the walk sees it: `Some(Some(i))` for a note or audio
    /// track, `Some(None)` for another part, `None` when unknown.
    pub(super) fn track_of(&self, id: Option<&str>) -> Option<Option<usize>> {
        id.and_then(|id| self.cx.track_index.get(id).copied())
    }

    /// The arrangement's timelines, down to the clips on each track.
    pub(super) fn walk(&mut self) -> Result<(), IoError> {
        let Some(top) = self
            .cx
            .pkg
            .project
            .arrangement
            .as_ref()
            .and_then(|a| a.lanes.as_ref())
        else {
            return Ok(());
        };
        let frame = Frame::top(top.head.time_unit.unwrap_or(DpTimeUnit::Beats));
        let track = self.track_of(top.head.track.as_deref()).flatten();
        let mut stack: Vec<(&'p DpTimeline, Frame, Option<usize>)> =
            top.items.iter().rev().map(|t| (t, frame, track)).collect();
        while let Some((t, f, track)) = stack.pop() {
            self.tick()?;
            if self.budget == 0 {
                break;
            }
            let f = f.with_unit(head_of(t).and_then(|h| h.time_unit));
            let track = match self.track_of(head_of(t).and_then(|h| h.track.as_deref())) {
                Some(t) => t,
                None => track,
            };
            match t {
                DpTimeline::Lanes(l) => {
                    for i in l.items.iter().rev() {
                        stack.push((i, f, track));
                    }
                }
                DpTimeline::Clips(c) => {
                    for clip in &c.clips {
                        self.tick()?;
                        match track {
                            Some(i) => self.top_clip(clip, &f, i)?,
                            None => self.left_out(NO_TRACK),
                        }
                    }
                }
                _ => self.flatten(Work::T(t, f, 0), To::Track(track))?,
            }
        }
        Ok(())
    }

    /// A clip on track `track`'s top level: a candidate for sharing, or
    /// straight onto the track.
    pub(super) fn top_clip(
        &mut self,
        c: &'p DpClip,
        f: &Frame,
        track: usize,
    ) -> Result<(), IoError> {
        let n = match self.clips.get_mut(track) {
            Some(k) => {
                *k = k.saturating_add(1);
                *k
            }
            None => 0,
        };
        let content = self.content_of(c);
        // The timeline it plays, when clips share it.
        let shared = c.reference.as_deref().or_else(|| {
            content
                .and_then(head_of)
                .and_then(|h| h.named.id.as_deref())
                .filter(|id| self.referenced.contains(id))
        });
        let name = c
            .named
            .name
            .as_deref()
            .map(|n| self.made_name(n))
            .filter(|n| !n.is_empty());
        let ps = c.play_start.unwrap_or(0.0);
        let lp = loop_for(c);
        let looped = lp.is_some_and(|(ls, _)| (ps - ls).abs() < 1e-9);
        if lp.is_some() && !looped {
            self.cx.add(ItemKind::Changed, UNROLLED, None);
        }
        let candidate = (shared.is_some() || looped || name.is_some()) && (lp.is_none() || looped);
        if !candidate || content.is_none() {
            return self.flatten(Work::C(c, *f, 0), To::Track(Some(track)));
        }
        // One pass.
        let mut pf = *f;
        pf.muted = false;
        let Some(mut fr) = self.one_pass(c, &pf) else {
            return Ok(());
        };
        // The use carries the clip's mute; its sequence plays.
        fr.muted = false;
        let (start, pass_end) = (fr.lo, fr.hi);
        let total = match c.duration {
            Some(d) => self
                .at(f, c.time + d)
                .map_or(pass_end, |e| e.min(END))
                .saturating_sub(start),
            None => pass_end.saturating_sub(start),
        };
        let len = pass_end.saturating_sub(start);
        if pass_end >= END || len <= 0 || total <= 0 {
            return self.flatten(Work::C(c, *f, 0), To::Track(Some(track)));
        }
        self.pass.clear();
        self.pass_other = false;
        if let Some(t) = content {
            self.flatten(Work::T(t, fr, 1), To::Pass)?;
        }
        if self.pass_other {
            self.pass.clear();
            return self.flatten(Work::C(c, *f, 0), To::Track(Some(track)));
        }
        let mut pass = std::mem::take(&mut self.pass);
        for e in &mut pass {
            e.tick = e.tick.saturating_sub(start);
        }
        let kind = match shared {
            Some(id) => Kind::Ref(id),
            None => Kind::Name(name.clone()),
        };
        self.cands.push(Cand {
            track,
            tick: start,
            kind,
            name,
            n,
            pass,
            len,
            total,
            looped,
            mute: f.muted || c.enable == Some(false),
        });
        Ok(())
    }

    /// The frame of one pass of clip `c` of `f`: its loop's span from
    /// `loopStart` (not cut by the clip's end), else its play window.
    pub(super) fn one_pass(&mut self, c: &DpClip, f: &Frame) -> Option<Frame> {
        let p = self.place_clip(c, f)?;
        match p.lp {
            Some((ls, _)) => self.pass(&p, ls, 0.0, true).0,
            None => self.plain(&p),
        }
    }

    /// The timeline a clip plays: its own, else the one it references.
    pub(super) fn content_of(&self, c: &'p DpClip) -> Option<&'p DpTimeline> {
        c.content.as_deref().or_else(|| {
            c.reference
                .as_deref()
                .and_then(|r| self.index.get(r).copied())
        })
    }

    // ---- frames

    /// Content time `c` of frame `f` as a tick; `None` when it is not one.
    pub(super) fn at(&self, f: &Frame, c: f64) -> Option<i64> {
        if !c.is_finite() {
            return None;
        }
        match f.unit {
            DpTimeUnit::Beats => crate::dawproject::units::ticks(c)
                .and_then(|(t, _)| f.zt.checked_add(t.0))
                .filter(|t| t.unsigned_abs() <= MAX_TICKS.unsigned_abs().saturating_mul(4)),
            DpTimeUnit::Seconds => {
                let s = f.zs + c;
                let t = self.clock.tick(s);
                (s.is_finite() && t.unsigned_abs() <= MAX_TICKS.unsigned_abs().saturating_mul(4))
                    .then_some(t)
            }
        }
    }

    /// The content time of `f` at tick `t`.
    pub(super) fn content_at(&self, f: &Frame, unit: DpTimeUnit, t: i64) -> f64 {
        match unit {
            DpTimeUnit::Beats => t.saturating_sub(f.zt) as f64 / PPQ as f64,
            DpTimeUnit::Seconds => self.clock.secs(t) - f.zs,
        }
    }

    /// Seconds of the media at content time `c` of a clip's content in
    /// `unit` (content time 0 is the file's start).
    pub(super) fn media_secs(&self, f: &Frame, unit: DpTimeUnit, c: f64) -> f64 {
        match unit {
            DpTimeUnit::Seconds => c,
            DpTimeUnit::Beats => {
                let t = crate::dawproject::units::ticks(c)
                    .and_then(|(t, _)| f.zt.checked_add(t.0))
                    .unwrap_or(i64::MAX);
                self.clock.secs(t) - f.zs
            }
        }
    }

    /// Where clip `c` of frame `f` lies; `None` (listed) for a clip that
    /// cannot be placed.
    pub(super) fn place_clip(&mut self, c: &DpClip, f: &Frame) -> Option<Place> {
        let Some(start) = self.at(f, c.time).filter(|t| (0..END).contains(t)) else {
            self.left_out(PAST_END);
            return None;
        };
        let end = match c.duration {
            Some(d) if d.is_finite() && d >= 0.0 => {
                Some(self.at(f, c.time + d).map_or(END, |e| e.min(END)))
            }
            Some(_) => {
                self.bad_value("Clip@duration");
                return None;
            }
            None => None,
        };
        let cu = c.content_time_unit.unwrap_or(f.unit);
        let ps = c.play_start.unwrap_or(0.0);
        if !ps.is_finite() {
            self.bad_value("Clip@playStart");
            return None;
        }
        let lp = loop_for(c);
        if c.loop_start.is_some() && c.loop_end.is_some() && lp.is_none() {
            self.bad_value("Clip@loopEnd");
        }
        let fades = (c.fade_in.is_some() || c.fade_out.is_some()).then(|| Fades {
            fin: c.fade_in.filter(|v| v.is_finite()),
            fout: c.fade_out.filter(|v| v.is_finite() && *v >= 0.0),
            unit: c.fade_time_unit.unwrap_or(f.unit),
        });
        let name = match c.named.name.as_deref().map(|n| clean_name(n).0) {
            Some(n) if !n.is_empty() => {
                self.clip_names.push(n);
                Some(self.clip_names.len() - 1)
            }
            _ => f.name,
        };
        Some(Place {
            base: Frame {
                unit: cu,
                zt: 0,
                zs: 0.0,
                lo: f.lo.max(start),
                hi: f.hi.min(end.unwrap_or(END)),
                cut: None,
                fades,
                name,
                muted: f.muted || c.enable == Some(false),
            },
            start,
            start_s: self.clock.secs(start),
            end,
            ps,
            pe: c.play_stop.filter(|v| v.is_finite()),
            duration: c.duration,
            parent: f.unit,
            lp,
        })
    }

    /// The frame of `p` whose content time `from` lies at tick `at`
    /// (second `at_s`).
    pub(super) fn zero(&self, p: &Place, from: f64, at: i64, at_s: f64) -> Option<Frame> {
        let mut fr = p.base;
        match fr.unit {
            DpTimeUnit::Beats => {
                let (t, _) = crate::dawproject::units::ticks(from)?;
                fr.zt = at.checked_sub(t.0)?;
                fr.zs = self.clock.secs(fr.zt);
            }
            DpTimeUnit::Seconds => {
                fr.zs = at_s - from;
                fr.zt = self.clock.tick(fr.zs);
            }
        }
        Some(fr)
    }

    /// The frame of a clip with no loop: its content from `playStart` to
    /// `playStop` (or for its duration).
    pub(super) fn plain(&mut self, p: &Place) -> Option<Frame> {
        let Some(mut fr) = self.zero(p, p.ps, p.start, p.start_s) else {
            self.left_out(PAST_END);
            return None;
        };
        let cu = fr.unit;
        let to = match p.pe {
            Some(pe) => {
                if let Some(t) = self.at(&fr, pe) {
                    fr.hi = fr.hi.min(t);
                }
                pe
            }
            None => match (p.end, p.duration) {
                (Some(_), Some(d)) if cu == p.parent => p.ps + d,
                (Some(e), _) => self.content_at(&fr, cu, e),
                (None, _) => f64::INFINITY,
            },
        };
        fr.cut = Some((p.ps, to, cu));
        Some(fr)
    }

    /// Pass `at` of a looped clip: content from `from` to `loopEnd`, at
    /// clip offset `offset` (content units). Returns its frame (`None` when
    /// it does not play) and the next pass's `(from, offset)` (`None` after
    /// the last). With `one_pass`, the pass is not cut by the clip's end.
    pub(super) fn pass(
        &mut self,
        p: &Place,
        from: f64,
        offset: f64,
        one_pass: bool,
    ) -> (Option<Frame>, Option<(f64, f64)>) {
        let Some((ls, le)) = p.lp else {
            return (None, None);
        };
        let (at, at_s) = match p.base.unit {
            DpTimeUnit::Beats => {
                match crate::dawproject::units::ticks(offset)
                    .and_then(|(t, _)| p.start.checked_add(t.0))
                {
                    Some(t) => (t, self.clock.secs(t)),
                    None => return (None, None),
                }
            }
            DpTimeUnit::Seconds => {
                let s = p.start_s + offset;
                (self.clock.tick(s), s)
            }
        };
        if at >= p.base.hi && !(one_pass && offset == 0.0) {
            return (None, None);
        }
        let fr = self.zero(p, from, at, at_s).map(|mut fr| {
            fr.lo = p.base.lo.max(at);
            let pass_end = self.at(&fr, le).unwrap_or(END).min(END);
            fr.hi = if one_pass {
                pass_end
            } else {
                p.base.hi.min(pass_end)
            };
            fr.cut = Some((from, le, fr.unit));
            fr
        });
        let next = (!one_pass && p.end.is_some() && fr.is_some_and(|f| f.hi > at))
            .then_some((ls, offset + (le - from)));
        (fr, next)
    }

    // ---- flattening

    /// Places `root` (and everything inside it) as `to` says.
    pub(super) fn flatten(&mut self, root: Work<'p>, to: To) -> Result<(), IoError> {
        let mut stack = vec![root];
        while let Some(w) = stack.pop() {
            self.tick()?;
            if !self.spend() {
                return Ok(());
            }
            match w {
                Work::C(c, f, depth) => {
                    let depth = depth.saturating_add(1);
                    if depth > MAX_TIMELINE_NESTING {
                        self.left_out(NESTED_TOO_DEEP);
                        continue;
                    }
                    let Some(t) = self.content_of(c) else {
                        self.left_out(EMPTY_CLIPS);
                        continue;
                    };
                    let Some(p) = self.place_clip(c, &f) else {
                        continue;
                    };
                    match p.lp {
                        Some(_) => stack.push(Work::P(p, t, depth, p.ps, 0.0)),
                        None => {
                            if let Some(fr) = self.plain(&p) {
                                stack.push(Work::T(t, fr, depth));
                            }
                        }
                    }
                }
                Work::P(p, t, depth, from, offset) => {
                    // One pass at a time, so a long loop holds no more
                    // than this one.
                    let (fr, next) = self.pass(&p, from, offset, false);
                    if let Some((from, offset)) = next {
                        stack.push(Work::P(p, t, depth, from, offset));
                    }
                    if let Some(fr) = fr {
                        stack.push(Work::T(t, fr, depth));
                    }
                }
                Work::T(t, f, depth) => {
                    let f = f.with_unit(head_of(t).and_then(|h| h.time_unit));
                    match t {
                        DpTimeline::Lanes(l) => {
                            for i in l.items.iter().rev() {
                                stack.push(Work::T(i, f, depth));
                            }
                        }
                        DpTimeline::Clips(cs) => {
                            if f.fades.is_some() {
                                self.left_out(NESTED_FADES);
                            }
                            let mut inner = f;
                            inner.fades = None;
                            for c in cs.clips.iter().rev() {
                                stack.push(Work::C(c, inner, depth));
                            }
                        }
                        DpTimeline::Notes(n) => self.notes(n, &f, to)?,
                        DpTimeline::Points(p) => self.points(p, &f, to)?,
                        DpTimeline::Audio(a) => self.audio(a, &f, to)?,
                        DpTimeline::Warps(w) => {
                            self.warps(w, &f, to);
                            stack.push(Work::T(&w.content, f, depth));
                        }
                        DpTimeline::Markers(..) => self.left_out(TRACK_MARKERS),
                        DpTimeline::Video(_) => self.left_out(VIDEO),
                        DpTimeline::ClipSlot(..) => self.left_out(SLOT_OTHER),
                        // Listed by the reader.
                        DpTimeline::Unknown(_) => {}
                    }
                }
            }
        }
        Ok(())
    }

    /// A MIDI event: into the pass, onto the track, or (listed) nowhere.
    pub(super) fn put(&mut self, to: To, e: Ev) {
        match to {
            To::Pass => self.pass.push(e),
            To::Track(Some(t)) => {
                if let Some(v) = self.evs.get_mut(t) {
                    v.push(e);
                }
            }
            To::Track(None) => self.left_out(NO_TRACK),
        }
    }

    /// A tick for a part that starts at content time `c`: `Some` inside the
    /// window; `None` outside it (listed when before 0 or past the end).
    pub(super) fn start_at(&mut self, f: &Frame, c: f64) -> Option<i64> {
        match self.at(f, c) {
            Some(t) if (f.lo..f.hi).contains(&t) => Some(t),
            Some(t) if (0..END).contains(&t) => None,
            _ => {
                self.left_out(PAST_END);
                None
            }
        }
    }
}
