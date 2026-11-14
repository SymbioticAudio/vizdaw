//! The clip launcher: scenes' slots as unplaced sequences.

use super::*;

impl<'p> Tl<'_, '_, 'p> {
    /// The scenes' clip slots, each as an unplaced sequence.
    pub(super) fn scenes(&mut self) -> Result<(), IoError> {
        let scenes = &self.cx.pkg.project.scenes;
        for (si, s) in scenes.iter().enumerate() {
            let Some(content) = &s.content else { continue };
            let scene = s
                .named
                .name
                .as_deref()
                .map(|n| self.made_name(n))
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("Scene {}", si.saturating_add(1)));
            let mut stack: Vec<(&'p DpTimeline, Frame, Option<usize>)> =
                vec![(content, Frame::top(DpTimeUnit::Beats), None)];
            while let Some((t, f, track)) = stack.pop() {
                self.tick()?;
                if self.budget == 0 {
                    return Ok(());
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
                    DpTimeline::ClipSlot(_, None) => {}
                    DpTimeline::ClipSlot(_, Some(clip)) => match track {
                        Some(i) => self.slot(clip, &f, i, &scene)?,
                        None => self.left_out(NO_TRACK),
                    },
                    _ => self.left_out(SLOT_OTHER),
                }
            }
        }
        Ok(())
    }

    /// One clip launcher slot's clip on track `track`.
    pub(super) fn slot(
        &mut self,
        c: &'p DpClip,
        f: &Frame,
        track: usize,
        scene: &str,
    ) -> Result<(), IoError> {
        let mut pf = *f;
        pf.muted = false;
        let Some(mut fr) = self.one_pass(c, &pf) else {
            return Ok(());
        };
        // The use carries the clip's mute; its sequence plays.
        fr.muted = false;
        self.pass.clear();
        self.pass_other = false;
        if let Some(t) = self.content_of(c) {
            self.flatten(Work::T(t, fr, 1), To::Pass)?;
        }
        if self.pass_other {
            self.left_out(SLOT_OTHER);
        }
        let mut evs = std::mem::take(&mut self.pass);
        let start = fr.lo;
        for e in &mut evs {
            e.tick = e.tick.saturating_sub(start);
        }
        let len = if fr.hi < END {
            fr.hi.saturating_sub(start)
        } else {
            evs.iter().map(ev_end).max().unwrap_or(0)
        };
        let name = format!("{scene} \u{2013} {}", self.name_of(track));
        let name = self.made_name(&name);
        self.slots.push(RawSeq {
            name,
            track,
            len: len.max(1),
            evs,
        });
        Ok(())
    }
}
