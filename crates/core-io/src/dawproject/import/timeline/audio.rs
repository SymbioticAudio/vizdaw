//! Audio regions, their media items (Ruling 19), fades and crossfades
//! (Ruling 10).

use super::*;

impl<'p> Tl<'_, '_, 'p> {
    /// Lists warps that stretch time (Ruling 4: the audio is placed
    /// unstretched either way).
    pub(super) fn warps(&mut self, w: &DpWarps, f: &Frame, to: To) {
        let secs = |tl: &Self, unit: DpTimeUnit, t: f64| tl.media_secs(f, unit, t);
        let stretched = w.warps.iter().any(|&(t, ct)| {
            let a = secs(self, f.unit, t);
            let b = secs(self, w.content_time_unit, ct);
            !(a - b).abs().le(&WARP_SLACK)
        });
        if stretched {
            let detail = match to {
                To::Track(Some(t)) => Some(self.name_of(t)),
                _ => None,
            };
            self.cx.add(ItemKind::LeftOut, TIME_STRETCH, detail);
        }
    }

    // ---- audio

    /// The plan's media items for audio file `key`, one per channel part;
    /// made on first use (Ruling 19).
    pub(super) fn media_for(&mut self, key: usize) -> Vec<usize> {
        if let Some(v) = self.media_at.get(&key) {
            return v.clone();
        }
        let Some(aref) = self.cx.pkg.audio().get(key).cloned() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let why = match self.sources.get(&key) {
            Some(MediaSource::Online(items)) if !items.is_empty() => {
                for it in items {
                    self.media.push(MediaPlan {
                        path: it.path.clone(),
                        name: it.name.clone(),
                        rate: it.sample_rate,
                        channels: it.channels,
                        frames: it.length_frames,
                        hash: Some(it.content_hash.clone()),
                        original: it.original_name.clone(),
                    });
                    out.push(self.media.len() - 1);
                }
                None
            }
            Some(MediaSource::Offline { why }) => Some(why.clone()),
            _ => Some("it was not read".to_owned()),
        };
        if let Some(why) = why {
            self.cx.add(
                ItemKind::Note,
                OFFLINE,
                Some(format!("{}: {why}", aref.name)),
            );
            if aref.sample_rate > 0 && aref.channels > 0 && aref.frames > 0 {
                let path = self.offline_path(&aref.name);
                let original = match &aref.source {
                    crate::dawproject::package::AudioSource::Entry(p) => p.clone(),
                    crate::dawproject::package::AudioSource::External(p) => {
                        p.to_string_lossy().into_owned()
                    }
                };
                let original = original
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                self.media.push(MediaPlan {
                    path,
                    name: aref.name.clone(),
                    rate: aref.sample_rate,
                    channels: aref.channels,
                    frames: aref.frames,
                    hash: None,
                    original,
                });
                out.push(self.media.len() - 1);
            }
        }
        self.media_at.insert(key, out.clone());
        out
    }

    /// `Media/<name>.caf`, made safe ([`crate::names::safe_part`], with
    /// `\` mapped too, which a media path may not hold) and unique among
    /// the import's items, online ones included, ignoring case (as the
    /// disk does).
    pub(super) fn offline_path(&mut self, name: &str) -> String {
        // Room for ".caf" and " <n>".
        const RESERVE: usize = ".caf".len() + " 4294967295".len();
        let stem = crate::names::safe_part(&name.replace('\\', "-"), RESERVE);
        let key = stem.to_lowercase();
        let mut path = format!("Media/{stem}.caf");
        // The next number to try for this stem, so many files of one name
        // stay linear.
        let mut n = self.path_next.get(&key).copied().unwrap_or(2);
        while self.paths.contains(&path.to_lowercase()) {
            path = format!("Media/{stem} {n}.caf");
            n = n.saturating_add(1);
        }
        self.path_next.insert(key, n);
        self.paths.insert(path.to_lowercase());
        path
    }

    pub(super) fn audio(&mut self, a: &'p DpAudio, f: &Frame, to: To) -> Result<(), IoError> {
        let track = match to {
            To::Pass => {
                self.pass_other = true;
                return Ok(());
            }
            To::Track(None) => {
                self.left_out(NO_TRACK);
                return Ok(());
            }
            To::Track(Some(t)) => t,
        };
        if a.sample_rate <= 0 {
            self.bad_value("Audio@sampleRate");
            return Ok(());
        }
        if a.channels <= 0 {
            self.bad_value("Audio@channels");
            return Ok(());
        }
        let Some(key) = self.cx.pkg.audio_key(&a.file) else {
            self.left_out(AUDIO_UNLISTED);
            return Ok(());
        };
        let items = self.media_for(key);
        let Some(first) = items.first().and_then(|i| self.media.get(*i)).cloned() else {
            return Ok(());
        };
        let rate = f64::from(first.rate.max(1));
        // The media window in seconds: the clip's content window, cut by
        // the outer windows.
        let (mut from_s, mut to_s) = match f.cut {
            Some((from, to, DpTimeUnit::Seconds)) => (from, to),
            Some((from, to, unit)) => (
                self.media_secs(f, unit, from),
                if to.is_finite() {
                    self.media_secs(f, unit, to)
                } else {
                    f64::INFINITY
                },
            ),
            None => (0.0, f64::INFINITY),
        };
        if f.lo > self.clock.tick(f.zs + from_s) {
            from_s = self.clock.secs(f.lo) - f.zs;
        }
        if f.hi < END && f.hi < self.clock.tick(f.zs + to_s) {
            to_s = self.clock.secs(f.hi) - f.zs;
        }
        from_s = from_s.max(0.0);
        if !(from_s.is_finite() && to_s > from_s) {
            return Ok(());
        }
        let tick = self.clock.tick(f.zs + from_s);
        if !(0..END).contains(&tick) {
            self.left_out(PAST_END);
            return Ok(());
        }
        let offset = (from_s * rate).round();
        let mut end = if to_s.is_finite() {
            (to_s * rate).round()
        } else {
            f64::INFINITY
        };
        let frames = first.frames as f64;
        if offset >= frames {
            self.count(ItemKind::LeftOut, AUDIO_PAST_FILE);
            return Ok(());
        }
        if end > frames {
            if end.is_finite() {
                self.cx.add(ItemKind::Changed, AUDIO_CUT, None);
            }
            end = frames;
        }
        // Within 0 … the file's frames, so the casts are exact.
        let (mut source_offset, mut length) = (offset as u64, (end - offset) as u64);
        if length == 0 {
            self.count(ItemKind::LeftOut, AUDIO_TOO_SHORT);
            return Ok(());
        }
        let mut tick = tick;
        let (mut fade_in, mut fade_out, mut negative) = (0u64, 0u64, false);
        if let Some(fd) = f.fades {
            let start_s = self.clock.secs(tick);
            let end_tick = self.clock.tick(f.zs + to_s.min(frames / rate));
            if let Some(fin) = fd.fin {
                if fin < 0.0 {
                    // The format's crossfade form: the clip starts |f|
                    // earlier, fading in over it.
                    let ext_s = match fd.unit {
                        DpTimeUnit::Seconds => -fin,
                        DpTimeUnit::Beats => crate::dawproject::units::ticks(-fin)
                            .map_or(0.0, |(t, _)| start_s - self.clock.secs(tick - t.0)),
                    };
                    let want = (ext_s * rate).round().max(0.0) as u64;
                    let ext = want
                        .min(source_offset)
                        .min((start_s * rate).round().max(0.0) as u64);
                    // The file (or the timeline) starts too soon for all of
                    // it: what is left is a plain fade, or none.
                    if ext < want {
                        self.count(ItemKind::Changed, CROSSFADES_PLAIN);
                    }
                    source_offset -= ext;
                    length = length.saturating_add(ext);
                    tick = self.clock.tick(start_s - ext as f64 / rate);
                    fade_in = ext;
                    negative = ext > 0;
                } else {
                    fade_in = self.fade_frames(fin, fd.unit, tick, rate, true);
                }
            }
            if let Some(fout) = fd.fout {
                fade_out = self.fade_frames(fout, fd.unit, end_tick, rate, false);
            }
        }
        if fade_in.saturating_add(fade_out) > length {
            self.cx.add(ItemKind::Changed, FADES_CUT, None);
            fade_out = fade_out.min(length.saturating_sub(fade_in.min(length)));
            fade_in = fade_in.min(length);
        }
        for (part, &m) in items.iter().enumerate() {
            let t = if part == 0 {
                track
            } else {
                self.part_track(track, part)
            };
            self.regs.push(Reg {
                track: t,
                tick,
                mute: f.muted,
                rate,
                negative,
                dropped: false,
                plan: RegionPlan {
                    media: m,
                    source_offset,
                    length,
                    fade_in,
                    fade_out,
                    name: f.name.and_then(|i| self.clip_names.get(i)).cloned(),
                    crossfade: false,
                },
            });
        }
        Ok(())
    }

    /// A fade of `v` in `unit` as frames: from `at` forward (`fade_in`) or
    /// back.
    pub(super) fn fade_frames(
        &self,
        v: f64,
        unit: DpTimeUnit,
        at: i64,
        rate: f64,
        fade_in: bool,
    ) -> u64 {
        let secs = match unit {
            DpTimeUnit::Seconds => v,
            DpTimeUnit::Beats => match crate::dawproject::units::ticks(v) {
                Some((t, _)) if fade_in => {
                    self.clock.secs(at.saturating_add(t.0)) - self.clock.secs(at)
                }
                Some((t, _)) => self.clock.secs(at) - self.clock.secs(at.saturating_sub(t.0)),
                None => 0.0,
            },
        };
        let f = (secs * rate).round();
        if f.is_finite() && f > 0.0 {
            f.min(u64::MAX as f64) as u64
        } else {
            0
        }
    }

    /// The track for channel `part` (from 0) of a wide file on `track`.
    pub(super) fn part_track(&mut self, track: usize, part: usize) -> usize {
        if let Some(&t) = self.extra_at.get(&(track, part)) {
            return t;
        }
        let name = format!("{} ch {}", self.name_of(track), part.saturating_add(1));
        let name = self.made_name(&name);
        let t = self.new_track(name.clone(), true);
        self.extra_at.insert((track, part), t);
        self.cx.add(ItemKind::Changed, WIDE_AUDIO, Some(name));
        t
    }

    /// Ruling 10's import rule over one track's regions (in time order):
    /// a region that starts inside the one before it, with a fade-in that
    /// fits the overlap (and the earlier fade-out covering it, unless the
    /// fade-in came from a negative `fadeInTime`), crossfades with it.
    pub(super) fn crossfades(&mut self, regs: &mut [Reg]) -> Result<(), IoError> {
        // (index, tick, seconds, length in seconds) of the regions still
        // sounding, as `crossfade::overlaps` finds partners. A region that
        // would start over MAX_SOUNDING of them is left out, so the set
        // (here and in the model's sweep) stays that small.
        let mut sounding: Vec<(usize, i64, f64, f64)> = Vec::new();
        let frames_at = |s: f64, rate: f64| (s * rate).round() as i128;
        for j in 0..regs.len() {
            self.tick()?;
            let Some(b) = regs.get(j) else { continue };
            let (tick, rate, negative) = (b.tick, b.rate, b.negative);
            let (fin, fout, len) = (b.plan.fade_in, b.plan.fade_out, b.plan.length);
            let secs = self.clock.secs(tick);
            let start = frames_at(secs, rate);
            let ends_after =
                |&&(_, _, s, l): &&(usize, i64, f64, f64)| frames_at(s + l, rate) > start;
            // Counted before `retain`, which the model never runs for a
            // region it does not hold.
            if sounding.iter().filter(ends_after).count() >= MAX_SOUNDING {
                if let Some(b) = regs.get_mut(j) {
                    b.dropped = true;
                }
                self.count(ItemKind::LeftOut, TOO_MUCH);
                continue;
            }
            sounding.retain(|p| ends_after(&p));
            let partner = sounding.iter().rev().find(|p| p.1 < tick).copied();
            let mut made = false;
            if let Some((i, _, s, l)) = partner {
                let overlap = u64::try_from(frames_at(s + l, rate) - start).unwrap_or(0);
                let a_out = regs.get(i).map_or(0, |a| a.plan.fade_out);
                let covered = negative || a_out.saturating_add(1) >= overlap;
                let room = overlap.min(len.saturating_sub(fout));
                if fin >= 1 && fin <= overlap.saturating_add(1) && covered && room >= 1 {
                    made = true;
                    if let Some(b) = regs.get_mut(j) {
                        b.plan.crossfade = true;
                        b.plan.fade_in = fin.min(room);
                    }
                    if a_out.abs_diff(overlap) <= 1
                        && let Some(a) = regs.get_mut(i)
                    {
                        a.plan.fade_out = 0;
                    }
                }
            }
            if negative && !made && partner.is_some() {
                self.cx.add(ItemKind::Changed, CROSSFADES_PLAIN, None);
            }
            sounding.push((j, tick, secs, len as f64 / rate));
        }
        Ok(())
    }
}
