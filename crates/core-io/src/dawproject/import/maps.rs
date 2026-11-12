//! The import's tempo and meter maps, markers and song info.

use super::*;

impl Cx<'_, '_> {
    /// A beats or seconds time as ticks; seconds through `seconds`. `None`
    /// (listed) for a time before 0 or past the timeline.
    pub(super) fn time(&mut self, t: f64, unit: DpTimeUnit, tempo: &TempoMap) -> Option<Tick> {
        let tick = match unit {
            DpTimeUnit::Beats => crate::dawproject::units::ticks(t).map(|(t, _)| t),
            DpTimeUnit::Seconds if t.is_finite() => Some(tempo.seconds_to_tick(t)),
            DpTimeUnit::Seconds => None,
        };
        match tick {
            Some(k) if (0..=MAX_TICKS).contains(&k.0) => Some(k),
            _ => {
                self.add(ItemKind::LeftOut, PAST_END, None);
                None
            }
        }
    }

    /// A tempo in BPM, clamped to 5–999 (Changed); `None` (listed) when not
    /// a number.
    pub(super) fn bpm(&mut self, v: f64, what: &str) -> Option<Bpm> {
        if !v.is_finite() {
            self.add(ItemKind::LeftOut, UNREADABLE_VALUES, Some(what.to_owned()));
            return None;
        }
        let c = v.clamp(Bpm::MIN.as_f64(), Bpm::MAX.as_f64());
        if c != v {
            self.add(ItemKind::Changed, TEMPO_CLAMPED, None);
        }
        // Within 5 000–999 000 milli-BPM, so the cast is exact.
        Some(Bpm((c * 1000.0).round() as u32))
    }

    pub(super) fn meter(&mut self, num: i32, den: i32) -> Option<Meter> {
        let m = u8::try_from(num)
            .ok()
            .zip(u8::try_from(den).ok())
            .map(|(num, den)| Meter { num, den })
            .filter(|m| m.is_valid());
        if m.is_none() {
            self.add(ItemKind::LeftOut, BAD_METER, Some(format!("{num}/{den}")));
        }
        m
    }

    /// The tempo map, then the meter map (meter times in seconds go
    /// through the finished tempo map).
    pub(super) fn maps(&mut self) -> Result<(TempoMap, MeterMap), IoError> {
        let p = &self.pkg.project;
        let arr = p.arrangement.as_ref();
        let first = p
            .tempo
            .as_ref()
            .and_then(|t| t.value)
            .and_then(|v| self.bpm(v, "Tempo@value"))
            .unwrap_or(Bpm(120_000));
        let mut points = vec![TempoPoint {
            tick: Tick::ZERO,
            bpm: first,
            ramp: false,
        }];
        // Seconds at the last point, for seconds times.
        let mut secs_at_last = 0.0f64;
        if let Some(a) = arr.and_then(|a| a.tempo_automation.as_ref()) {
            let unit = a.head.time_unit.unwrap_or(DpTimeUnit::Beats);
            if a.unit.is_some_and(|u| u != DpUnit::Bpm) {
                self.add(
                    ItemKind::LeftOut,
                    UNREADABLE_VALUES,
                    Some("TempoAutomation@unit".to_owned()),
                );
            } else {
                for pt in &a.points {
                    self.tick()?;
                    let DpPointValue::Real(v, how) = pt.value else {
                        self.add(
                            ItemKind::LeftOut,
                            UNREADABLE_VALUES,
                            Some("TempoAutomation point".to_owned()),
                        );
                        continue;
                    };
                    let Some(bpm) = self.bpm(v, "TempoAutomation point") else {
                        continue;
                    };
                    let ramp = how == Some(DpInterpolation::Linear);
                    let Some(last) = points.last().copied() else {
                        continue;
                    };
                    let tick = match unit {
                        DpTimeUnit::Beats => {
                            crate::dawproject::units::ticks(pt.time).map(|(t, _)| t.0)
                        }
                        DpTimeUnit::Seconds => {
                            let dt = pt.time - secs_at_last;
                            if dt < 0.0 {
                                self.add(ItemKind::LeftOut, MAP_ORDER, None);
                                continue;
                            }
                            seconds_after(last, bpm, dt).map(|d| last.tick.0.saturating_add(d))
                        }
                    };
                    let Some(tick) = tick.filter(|t| (0..=MAX_TICKS).contains(t)) else {
                        self.add(ItemKind::LeftOut, PAST_END, None);
                        continue;
                    };
                    if unit == DpTimeUnit::Seconds && tick >= last.tick.0 {
                        secs_at_last = pt.time;
                    }
                    let new = TempoPoint {
                        tick: Tick(tick),
                        bpm,
                        ramp,
                    };
                    match points.last_mut() {
                        Some(l) if l.tick.0 == tick => *l = new,
                        Some(l) if l.tick.0 > tick => {
                            self.add(ItemKind::LeftOut, MAP_ORDER, None);
                        }
                        _ => points.push(new),
                    }
                }
            }
        }
        let tempo = TempoMap::new(points)
            .or_else(|_| TempoMap::constant(Bpm(120_000)))
            .unwrap_or_default();

        let key = KeySig::default();
        let m0 = p
            .time_signature
            .as_ref()
            .and_then(|t| self.meter(t.numerator, t.denominator))
            .unwrap_or(Meter { num: 4, den: 4 });
        let mut entries = vec![MeterEntry {
            tick: Tick::ZERO,
            meter: m0,
            key,
        }];
        if let Some(a) = arr.and_then(|a| a.time_signature_automation.as_ref()) {
            let unit = a.head.time_unit.unwrap_or(DpTimeUnit::Beats);
            for pt in &a.points {
                self.tick()?;
                let DpPointValue::TimeSig(num, den) = pt.value else {
                    self.add(
                        ItemKind::LeftOut,
                        UNREADABLE_VALUES,
                        Some("TimeSignatureAutomation point".to_owned()),
                    );
                    continue;
                };
                let Some(meter) = self.meter(num, den) else {
                    continue;
                };
                let Some(tick) = self.time(pt.time, unit, &tempo) else {
                    continue;
                };
                let Some(last) = entries.last().copied() else {
                    continue;
                };
                if tick < last.tick {
                    self.add(ItemKind::LeftOut, MAP_ORDER, None);
                    continue;
                }
                let bar = last.meter.bar_ticks().max(1);
                let rel = tick.0.saturating_sub(last.tick.0);
                let at = rel
                    .checked_add(bar - 1)
                    .map(|r| r / bar * bar)
                    .and_then(|r| r.checked_add(last.tick.0))
                    .filter(|t| *t <= MAX_TICKS);
                let Some(at) = at else {
                    self.add(ItemKind::LeftOut, PAST_END, None);
                    continue;
                };
                if at != tick.0 {
                    self.add(ItemKind::Changed, METER_MOVED, None);
                }
                if at == last.tick.0 {
                    if let Some(l) = entries.last_mut() {
                        l.meter = meter;
                    }
                } else if meter != last.meter {
                    entries.push(MeterEntry {
                        tick: Tick(at),
                        meter,
                        key,
                    });
                }
            }
        }
        let meter = MeterMap::new(entries).unwrap_or_default();
        Ok((tempo, meter))
    }

    pub(super) fn markers(&mut self, tempo: &TempoMap) -> Result<Vec<Marker>, IoError> {
        let Some(a) = self.pkg.project.arrangement.as_ref() else {
            return Ok(Vec::new());
        };
        let unit = a.markers_head.time_unit.unwrap_or(DpTimeUnit::Beats);
        let mut out = Vec::new();
        for (i, m) in a.markers.iter().enumerate() {
            self.tick()?;
            let name = self.name(m.named.name.as_deref(), || {
                format!("Marker {}", i.saturating_add(1))
            });
            if m.named.color.is_some() {
                self.add(ItemKind::LeftOut, COLOURS_AND_COMMENTS, Some(name.clone()));
            }
            self.comment(m.named.comment.as_deref(), || format!("marker '{name}'"));
            if let Some(tick) = self.time(m.time, unit, tempo) {
                out.push(Marker { tick, name });
            }
        }
        out.sort_by_key(|m| m.tick);
        Ok(out)
    }

    /// The song info and the sequence's name (the title, else the file's
    /// stem).
    pub(super) fn song_info(&mut self) -> (String, SongInfo) {
        let m = &self.pkg.metadata;
        let mut info = SongInfo::default();
        for (i, (out, field)) in info.fields_mut().into_iter().zip(&m.fields).enumerate() {
            let Some(text) = field else { continue };
            let mut text = text.as_str();
            let stripped;
            if i == METADATA_FIELDS.len().saturating_sub(1) {
                stripped = without_export_line(text);
                text = &stripped;
            }
            let (kept, cut) = cut_to(text.trim_end(), MAX_INFO_BYTES);
            if cut || m.cut.get(i).copied().unwrap_or(false) {
                let field = METADATA_FIELDS.get(i).copied().unwrap_or("");
                self.add(ItemKind::Changed, INFO_CUT, Some(field.to_owned()));
            }
            if !kept.trim().is_empty() {
                *out = Some(kept.to_owned());
            }
        }
        let stem = self.pkg.stem().to_owned();
        let name = self.name(info.title.as_deref(), || {
            let (s, _) = clean_name(&stem);
            if s.is_empty() {
                "Sequence 1".to_owned()
            } else {
                s
            }
        });
        (name, info)
    }
}

/// `comment` without its last line when that is the export line.
pub(super) fn without_export_line(comment: &str) -> String {
    let trimmed = comment.trim_end();
    let (head, last) = match trimmed.rfind('\n') {
        Some(i) => (
            trimmed.get(..i).unwrap_or(""),
            trimmed.get(i + 1..).unwrap_or(""),
        ),
        None => ("", trimmed),
    };
    if last.starts_with(EXPORT_LINE) {
        head.trim_end().to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// `s` cut to at most `max` bytes at a character boundary, and whether it
/// was cut.
pub(super) fn cut_to(s: &str, max: usize) -> (&str, bool) {
    if s.len() <= max {
        return (s, false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    (s.get(..end).unwrap_or(""), true)
}

/// `raw` as a track, bus, marker or sequence name: control and zero-width
/// characters dropped (as `core_model::lane::valid_name` wants), trimmed,
/// cut to [`MAX_INFO_BYTES`]; and whether it was cut.
pub(super) fn clean_name(raw: &str) -> (String, bool) {
    let bad =
        |c: char| c.is_control() || matches!(c, '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}');
    let mut out = String::new();
    let mut cut = false;
    for c in raw.trim().chars().filter(|c| !bad(*c)) {
        if out.len().saturating_add(c.len_utf8()) > MAX_INFO_BYTES {
            cut = true;
            break;
        }
        out.push(c);
    }
    (out.trim().to_owned(), cut)
}

/// The ticks after a tempo point `last` at which `dt` seconds have passed,
/// when the next point's tempo is `next` (a ramp from `last` reaches it
/// there). `None` for a time before `last` or one that is not a number.
pub(super) fn seconds_after(last: TempoPoint, next: Bpm, dt: f64) -> Option<i64> {
    if !dt.is_finite() || dt < 0.0 {
        return None;
    }
    let b0 = last.bpm.as_f64();
    let b1 = next.as_f64();
    let ppq = PPQ as f64;
    let ticks = if last.ramp && (b1 - b0).abs() > 1e-9 {
        // A linear ramp over L ticks lasts 60 L ln(b1/b0) / (PPQ (b1 − b0)).
        dt * ppq * (b1 - b0) / (60.0 * (b1 / b0).ln())
    } else {
        dt * ppq * b0 / 60.0
    };
    let t = ticks.round();
    (t.is_finite() && t <= MAX_TICKS as f64).then_some(t as i64)
}
