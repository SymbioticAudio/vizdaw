//! Automation (Ruling 14, inverted).

use super::*;

impl<'p> Tl<'_, '_, 'p> {
    /// Every channel's volume, pan, mute, send levels and device
    /// parameters.
    pub(super) fn params(&mut self) {
        let p = &self.cx.pkg.project;
        let mut chans: Vec<&'p DpChannel> = Vec::new();
        let mut tracks: Vec<&'p DpTrack> = Vec::new();
        for item in &p.structure {
            match item {
                DpStructureItem::Track(t) => tracks.push(t),
                DpStructureItem::Channel(c) => chans.push(c),
            }
        }
        while let Some(t) = tracks.pop() {
            chans.extend(t.channel.as_deref());
            tracks.extend(&t.children);
        }
        for c in chans {
            let Some(chan) = c.named.id.as_deref() else {
                continue;
            };
            let mut put = |id: Option<&'p str>, role: Role<'p>| {
                if let Some(id) = id {
                    self.params.insert(id, (role, chan));
                }
            };
            if let Some(v) = c.volume.as_deref() {
                put(v.named.id.as_deref(), Role::Volume(v));
            }
            if let Some(v) = c.pan.as_deref() {
                put(v.named.id.as_deref(), Role::Pan(v));
            }
            if let Some(m) = c.mute.as_deref() {
                put(m.named.id.as_deref(), Role::Mute);
            }
            for s in &c.sends {
                if let Some(sid) = s.named.id.as_deref() {
                    put(s.volume.named.id.as_deref(), Role::Send(sid, &s.volume));
                }
            }
            for d in &c.devices {
                let Some(did) = d.named.id.as_deref() else {
                    continue;
                };
                for q in &d.parameters {
                    put(q.named.id.as_deref(), Role::Device(did, q));
                }
            }
        }
    }

    /// Automation of parameter `param` (Ruling 14, inverted).
    pub(super) fn automation(
        &mut self,
        p: &'p DpPoints,
        param: &'p str,
        f: &Frame,
        track: Option<usize>,
    ) -> Result<(), IoError> {
        let Some(&(role, chan)) = self.params.get(param) else {
            self.cx
                .add(ItemKind::LeftOut, UNPLACED, Some(param.to_owned()));
            return Ok(());
        };
        let strip = match self.cx.chan_track.get(chan) {
            Some(&i) => Some(StripAt::Track(i)),
            None => match self.cx.targets.get(chan) {
                Some(Target::Master) => Some(StripAt::Master),
                Some(Target::Bus(b)) => Some(StripAt::Bus(*b)),
                _ => None,
            },
        };
        let at = strip.and_then(|s| match role {
            Role::Volume(_) => Some(AutoAt::Fader(s)),
            Role::Pan(_) if s != StripAt::Master => Some(AutoAt::Pan(s)),
            Role::Pan(_) => None,
            Role::Mute => Some(AutoAt::Mute(s)),
            Role::Send(id, _) => self
                .cx
                .send_at
                .get(id)
                .filter(|(_, i)| *i != usize::MAX)
                .map(|(_, i)| AutoAt::Send(s, *i)),
            Role::Device(dev, q) => {
                let slot = self.cx.dev_slot.get(dev).copied()?;
                let id = q.parameter_id?;
                // The parameter's 32 bits (Ruling 7).
                Some(AutoAt::Param(s, slot, ParamId(u64::from(id as u32))))
            }
        });
        // Whether the track owns a strip, and whether it shares the
        // built-in synth's, is known once every track's notes and audio
        // are (`finish`).
        let Some(at) = at else {
            self.cx
                .add(ItemKind::LeftOut, UNPLACED, Some(param.to_owned()));
            return Ok(());
        };
        let owner = track.or(match strip {
            Some(StripAt::Track(i)) => Some(i),
            _ => None,
        });
        if owner.is_none() && self.automation_track.is_none() {
            let i = self.new_track(AUTOMATION_NAME.to_owned(), false);
            self.automation_track = Some(i);
            self.cx.add(ItemKind::Note, AUTOMATION_TRACK, None);
        }
        let owner = owner.or(self.automation_track);
        let unit = p.unit.unwrap_or(match role {
            Role::Volume(r) | Role::Pan(r) | Role::Send(_, r) | Role::Device(_, r) => r.unit,
            Role::Mute => DpUnit::Normalized,
        });
        let stepped = matches!(at, AutoAt::Mute(_));
        let mut held = false;
        let mut last: Option<(i64, f64, bool)> = None;
        for pt in &p.points {
            self.tick()?;
            if !self.spend() {
                return Ok(());
            }
            let Some(tick) = self.start_at(f, pt.time) else {
                continue;
            };
            let Some(value) = self.auto_value(role, unit, &pt.value) else {
                self.bad_value("automation point");
                continue;
            };
            if let Some((t0, v0, true)) = last
                && !stepped
                && tick > t0.saturating_add(1)
            {
                held = true;
                self.autos.push(AutoEv {
                    owner,
                    param,
                    tick: tick - 1,
                    mute: f.muted,
                    at,
                    value: v0,
                    tag: f.tag,
                });
            }
            let hold = matches!(pt.value, DpPointValue::Real(_, Some(DpInterpolation::Hold)));
            last = Some((tick, value, hold));
            self.autos.push(AutoEv {
                owner,
                param,
                tick,
                mute: f.muted,
                at,
                value,
                tag: f.tag,
            });
        }
        if held {
            self.cx.add(ItemKind::Changed, HELD_STEPS, None);
        }
        Ok(())
    }

    /// An automation point's value, 0 to 1.
    pub(super) fn auto_value(
        &mut self,
        role: Role<'p>,
        unit: DpUnit,
        v: &DpPointValue,
    ) -> Option<f64> {
        let scaled = |r: &DpRealParam, v: f64| -> Option<f64> {
            match (r.min, r.max) {
                (Some(lo), Some(hi)) if lo.is_finite() && hi.is_finite() && hi > lo => {
                    Some((v - lo) / (hi - lo))
                }
                _ => None,
            }
        };
        let raw = match (role, v) {
            (Role::Mute, DpPointValue::Bool(b)) => f64::from(u8::from(*b)),
            (Role::Mute, DpPointValue::Real(x, _)) => f64::from(u8::from(*x >= 0.5)),
            (Role::Mute, DpPointValue::Integer(x)) => f64::from(u8::from(*x != 0)),
            (Role::Volume(_) | Role::Send(..), DpPointValue::Real(x, _)) => match unit {
                DpUnit::Linear | DpUnit::Normalized => fader_norm(db_of_linear(*x)),
                DpUnit::Decibel => fader_norm(*x),
                _ => return None,
            },
            (Role::Pan(r) | Role::Device(_, r), DpPointValue::Real(x, _)) => match unit {
                DpUnit::Normalized => *x,
                _ => scaled(r, *x)?,
            },
            (Role::Device(_, r), DpPointValue::Integer(x) | DpPointValue::Enum(x)) => {
                scaled(r, f64::from(*x))?
            }
            (Role::Device(..), DpPointValue::Bool(b)) => f64::from(u8::from(*b)),
            _ => return None,
        };
        if !raw.is_finite() {
            return None;
        }
        if !(0.0..=1.0).contains(&raw) {
            self.count(ItemKind::Changed, VALUES_CLAMPED);
        }
        Some(raw.clamp(0.0, 1.0))
    }
}
