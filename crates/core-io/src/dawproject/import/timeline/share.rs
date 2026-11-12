//! Making the plan: re-sharing candidates as sequences (spec §7.4),
//! channels, crossfades and the items of each track.

use super::notes::pair_banks;
use super::*;

impl<'p> Tl<'_, '_, 'p> {
    /// Makes the plan's items: new tracks join the plan, candidates group
    /// into sequences, regions get their crossfades, channels their
    /// overrides.
    pub(super) fn finish(mut self, plan: &mut Plan) -> Result<Arranged, IoError> {
        for (name, audio) in std::mem::take(&mut self.extra) {
            plan.tracks.push(TrackPlan {
                name,
                midi: None,
                audio,
                mix: Mix::default(),
                flags: None,
            });
        }
        let n = plan.tracks.len();
        // Notes or audio on a track that did not say it holds them.
        let mut midi_used = vec![false; n];
        for (i, v) in self.evs.iter().enumerate() {
            if !v.is_empty()
                && let Some(u) = midi_used.get_mut(i)
            {
                *u = true;
            }
        }
        for t in self
            .cands
            .iter()
            .map(|c| c.track)
            .chain(self.slots.iter().map(|s| s.track))
        {
            if let Some(u) = midi_used.get_mut(t) {
                *u = true;
            }
        }
        for (i, t) in plan.tracks.iter_mut().enumerate() {
            if midi_used.get(i).copied().unwrap_or(false) && t.midi.is_none() {
                t.midi = Some(Midi::Builtin);
                self.cx
                    .report
                    .add(ItemKind::Note, PLAYS_BUILTIN, Some(t.name.clone()));
            }
        }
        if plan.builtin.is_none() && plan.tracks.iter().any(|t| t.midi == Some(Midi::Builtin)) {
            plan.builtin = Some(Mix::default());
        }
        for r in &self.regs {
            if let Some(t) = plan.tracks.get_mut(r.track) {
                t.audio = true;
            }
        }

        let mut items: Vec<Vec<Item>> = vec![Vec::new(); n];
        let mut seqs: Vec<RawSeq> = Vec::new();
        self.group(plan, &mut items, &mut seqs)?;
        seqs.append(&mut self.slots);

        // Each track's main channel: the one most of its notes are on.
        let mut counts = vec![[0u64; 16]; n];
        let mut count = |t: usize, e: &Ev| {
            if matches!(e.body, EventBody::Note { .. })
                && let Some(c) = counts.get_mut(t).and_then(|c| c.get_mut(usize::from(e.ch)))
            {
                *c = c.saturating_add(1);
            }
        };
        for (t, v) in self.evs.iter().enumerate() {
            v.iter().for_each(|e| count(t, e));
        }
        for s in &seqs {
            s.evs.iter().for_each(|e| count(s.track, e));
        }
        let main: Vec<u8> = counts
            .iter()
            .map(|c| {
                let mut best = 0u8;
                for (ch, k) in (0u8..).zip(c.iter()) {
                    if *k > c.get(usize::from(best)).copied().unwrap_or(0) {
                        best = ch;
                    }
                }
                best
            })
            .collect();
        let gc = |main: u8, ch: u8| (ch != u8::MAX && ch != main).then_some((0, ch));

        for (t, v) in self.evs.iter_mut().enumerate() {
            pair_banks(v);
            let m = main.get(t).copied().unwrap_or(0);
            if let Some(out) = items.get_mut(t) {
                for e in v.drain(..) {
                    out.push(Item {
                        tick: Tick(e.tick),
                        mute: e.mute,
                        body: Body::Midi(e.body, gc(m, e.ch)),
                    });
                }
            }
        }

        // Regions, by track, with their crossfades (Ruling 10).
        let mut regs = std::mem::take(&mut self.regs);
        regs.sort_by_key(|r| (r.track, r.tick));
        let mut from = 0;
        while from < regs.len() {
            self.tick()?;
            let track = regs.get(from).map_or(0, |r| r.track);
            let to = from
                + regs
                    .get(from..)
                    .map_or(0, |r| r.partition_point(|x| x.track == track));
            if let Some(slice) = regs.get_mut(from..to) {
                self.crossfades(slice);
            }
            from = to.max(from + 1);
        }
        for r in regs {
            if let Some(out) = items.get_mut(r.track) {
                out.push(Item {
                    tick: Tick(r.tick),
                    mute: r.mute,
                    body: Body::Region(r.plan),
                });
            }
        }
        for a in std::mem::take(&mut self.autos) {
            let Some(t) = a.owner else { continue };
            if let Some(out) = items.get_mut(t) {
                out.push(Item {
                    tick: Tick(a.tick),
                    mute: a.mute,
                    body: Body::Auto {
                        at: a.at,
                        value: a.value,
                    },
                });
            }
        }
        for v in &mut items {
            v.sort_by_key(|i| i.tick);
        }

        let seqs = seqs
            .into_iter()
            .map(|mut s| {
                pair_banks(&mut s.evs);
                let m = main.get(s.track).copied().unwrap_or(0);
                SeqPlan {
                    name: s.name,
                    track: s.track,
                    length: Tick(s.len.max(1)),
                    events: s
                        .evs
                        .into_iter()
                        .map(|e| EventSpec {
                            tick: Tick(e.tick),
                            dest: None,
                            group_channel: gc(m, e.ch),
                            mute: e.mute,
                            body: e.body,
                        })
                        .collect(),
                }
            })
            .collect();
        Ok(Arranged {
            items,
            seqs,
            media: self.media,
        })
    }

    /// Groups the candidates (spec §7.4): clips that play one timeline,
    /// or share a name and have the same content, are one sequence per
    /// destination; a lone named clip goes straight onto its track.
    pub(super) fn group(
        &mut self,
        plan: &Plan,
        items: &mut [Vec<Item>],
        seqs: &mut Vec<RawSeq>,
    ) -> Result<(), IoError> {
        struct Group {
            first: usize,
            members: Vec<usize>,
        }
        let mut cands = std::mem::take(&mut self.cands);
        let mut groups: Vec<Group> = Vec::new();
        let mut buckets: HashMap<u64, Vec<usize>> = HashMap::new();
        for (ci, c) in cands.iter().enumerate() {
            self.tick()?;
            let h = fingerprint(c);
            let same = |g: &Group| {
                cands
                    .get(g.first)
                    .is_some_and(|f| f.kind == c.kind && f.len == c.len && f.pass == c.pass)
            };
            let found = buckets
                .get(&h)
                .and_then(|v| v.iter().copied().find(|&g| groups.get(g).is_some_and(same)));
            match found.and_then(|g| groups.get_mut(g)) {
                Some(g) => g.members.push(ci),
                None => {
                    buckets.entry(h).or_default().push(groups.len());
                    groups.push(Group {
                        first: ci,
                        members: vec![ci],
                    });
                }
            }
        }
        for g in groups {
            self.tick()?;
            let Some(first) = cands.get(g.first) else {
                continue;
            };
            let shared = matches!(first.kind, Kind::Ref(_))
                || g.members
                    .iter()
                    .any(|&m| cands.get(m).is_some_and(|c| c.looped))
                || (g.members.len() >= 2 && matches!(first.kind, Kind::Name(Some(_))));
            if !shared {
                for &m in &g.members {
                    let Some(c) = cands.get(m) else { continue };
                    if let Some(v) = self.evs.get_mut(c.track) {
                        v.extend(c.pass.iter().map(|e| Ev {
                            tick: e.tick.saturating_add(c.tick),
                            mute: e.mute || c.mute,
                            ..e.clone()
                        }));
                    }
                }
                continue;
            }
            // One sequence per destination: a plug-in track's own, or the
            // shared built-in synth.
            let dest = |t: usize| match plan.tracks.get(t).and_then(|t| t.midi.as_ref()) {
                Some(Midi::Plugin(_)) => Some(t),
                _ => None,
            };
            let mut subs: Vec<(Option<usize>, Vec<usize>)> = Vec::new();
            for &m in &g.members {
                let Some(c) = cands.get(m) else { continue };
                let d = dest(c.track);
                match subs.iter_mut().find(|(k, _)| *k == d) {
                    Some((_, v)) => v.push(m),
                    None => subs.push((d, vec![m])),
                }
            }
            if subs.len() > 1 {
                self.cx.add(ItemKind::Note, SHARED_SPLIT, None);
            }
            for (_, ms) in subs {
                let Some(head) = ms.first().and_then(|&m| cands.get(m)) else {
                    continue;
                };
                let name = head.name.clone().unwrap_or_else(|| {
                    clean_name(&format!("{} clip {}", self.name_of(head.track), head.n)).0
                });
                let (track, len) = (head.track, head.len);
                // Every member's pass is the same: the sequence takes the
                // first's.
                let evs = ms
                    .first()
                    .and_then(|&m| cands.get_mut(m))
                    .map(|c| std::mem::take(&mut c.pass))
                    .unwrap_or_default();
                let seq = seqs.len();
                seqs.push(RawSeq {
                    name,
                    track,
                    len,
                    evs,
                });
                for m in ms {
                    let Some(c) = cands.get(m) else { continue };
                    let (length, loop_count) = if c.looped {
                        looped_use(c.total, c.len)
                    } else {
                        (None, LoopCount::Count(1))
                    };
                    if let Some(v) = items.get_mut(c.track) {
                        v.push(Item {
                            tick: Tick(c.tick),
                            mute: c.mute,
                            body: Body::Use {
                                seq,
                                length,
                                loop_count,
                            },
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

/// A use of a looped clip's sequence: whole passes as a count, else filled
/// to its length.
pub(super) fn looped_use(total: i64, len: i64) -> (Option<Tick>, LoopCount) {
    if len > 0 && total % len == 0 {
        if let Some(n) = u32::try_from(total / len)
            .ok()
            .filter(|n| (1..=MAX_LOOPS).contains(n))
        {
            return (None, LoopCount::Count(n));
        }
    }
    (Some(Tick(total)), LoopCount::Fill)
}

/// A hash of what makes candidates the same (equal ones hash the same).
pub(super) fn fingerprint(c: &Cand<'_>) -> u64 {
    let mut h = DefaultHasher::new();
    c.kind.hash(&mut h);
    c.len.hash(&mut h);
    c.pass.len().hash(&mut h);
    for e in &c.pass {
        (e.tick, e.ch, e.mute).hash(&mut h);
        match &e.body {
            EventBody::Note {
                pitch,
                velocity,
                duration,
                ..
            } => (0u8, *pitch, *velocity, duration.0).hash(&mut h),
            EventBody::Cc { index, value } => (1u8, *index, *value).hash(&mut h),
            EventBody::PitchBend { value } => (2u8, *value).hash(&mut h),
            EventBody::ChannelPressure { value } => (3u8, *value).hash(&mut h),
            EventBody::PolyPressure { pitch, value } => (4u8, *pitch, *value).hash(&mut h),
            EventBody::Program { program, .. } => (5u8, *program).hash(&mut h),
            _ => 6u8.hash(&mut h),
        }
    }
    h.finish()
}
