//! Strip DSP on [`StereoBuf`]s. Every function clamps `frames` to the
//! buffers' lengths and, except `strip_gains`, is `// RT`.

use crate::backend::StereoBuf;

/// |x| above this (or NaN/Inf) in an insert's output is a fault (spec §7).
pub const FAULT_ABS: f32 = 64.0;

fn len(frames: u32, b: &StereoBuf) -> usize {
    (frames as usize).min(b.left.len()).min(b.right.len())
}

/// `dst = src` on the first `frames` frames. // RT
pub fn copy(dst: &mut StereoBuf, src: &StereoBuf, frames: u32) {
    let n = len(frames, dst).min(len(frames, src));
    if let (Some(d), Some(s)) = (dst.left.get_mut(..n), src.left.get(..n)) {
        d.copy_from_slice(s);
    }
    if let (Some(d), Some(s)) = (dst.right.get_mut(..n), src.right.get(..n)) {
        d.copy_from_slice(s);
    }
}

/// `dst += gain × src`; a gain of exactly 0 leaves `dst` untouched. // RT
pub fn mix_into(dst: &mut StereoBuf, src: &StereoBuf, gain: f32, frames: u32) {
    if gain == 0.0 {
        return;
    }
    let n = len(frames, dst).min(len(frames, src));
    for (d, s) in [(&mut dst.left, &src.left), (&mut dst.right, &src.right)] {
        if let (Some(d), Some(s)) = (d.get_mut(..n), s.get(..n)) {
            for (a, b) in d.iter_mut().zip(s) {
                *a += gain * *b;
            }
        }
    }
}

/// `dst += g(i) × src` with `g` moving linearly from `from` at frame 0
/// toward `to` (reached just after the last frame); equal ends are
/// exactly [`mix_into`] (Plan 4d Ruling 11). // RT
pub fn mix_into_ramp(dst: &mut StereoBuf, src: &StereoBuf, from: f32, to: f32, frames: u32) {
    if from == to {
        return mix_into(dst, src, to, frames);
    }
    let n = len(frames, dst).min(len(frames, src));
    let step = (to - from) / n.max(1) as f32;
    for (d, s) in [(&mut dst.left, &src.left), (&mut dst.right, &src.right)] {
        if let (Some(d), Some(s)) = (d.get_mut(..n), s.get(..n)) {
            for (i, (a, b)) in d.iter_mut().zip(s).enumerate() {
                *a += (from + step * i as f32) * *b;
            }
        }
    }
}

/// The left channel times a gain moving from `from.0` toward `to.0`, the
/// right from `from.1` toward `to.1`; equal ends are exactly [`scale`].
/// // RT
pub fn scale_ramp(buf: &mut StereoBuf, from: (f32, f32), to: (f32, f32), frames: u32) {
    if from == to {
        return scale(buf, to.0, to.1, frames);
    }
    let n = len(frames, buf);
    for (c, (a, b)) in [
        (&mut buf.left, (from.0, to.0)),
        (&mut buf.right, (from.1, to.1)),
    ] {
        let step = (b - a) / n.max(1) as f32;
        if let Some(c) = c.get_mut(..n) {
            for (i, x) in c.iter_mut().enumerate() {
                *x *= a + step * i as f32;
            }
        }
    }
}

/// Multiplies the left channel by `gl` and the right by `gr`. // RT
pub fn scale(buf: &mut StereoBuf, gl: f32, gr: f32, frames: u32) {
    let n = len(frames, buf);
    for (c, g) in [(&mut buf.left, gl), (&mut buf.right, gr)] {
        if let Some(c) = c.get_mut(..n) {
            for x in c {
                *x *= g;
            }
        }
    }
}

/// Peak |x| of each channel; non-finite samples count as 0. // RT
pub fn peaks(buf: &StereoBuf, frames: u32) -> (f32, f32) {
    let n = len(frames, buf);
    let p = |c: &[f32]| {
        c.get(..n)
            .unwrap_or(&[])
            .iter()
            .filter(|x| x.is_finite())
            .fold(0.0f32, |m, x| m.max(x.abs()))
    };
    (p(&buf.left), p(&buf.right))
}

/// Peak over both channels. // RT
pub fn peak(buf: &StereoBuf, frames: u32) -> f32 {
    let (l, r) = peaks(buf, frames);
    l.max(r)
}

/// True when any sample is NaN, infinite or beyond ±[`FAULT_ABS`]. // RT
pub fn is_faulty(buf: &StereoBuf, frames: u32) -> bool {
    let n = len(frames, buf);
    [&buf.left, &buf.right].into_iter().any(|c| {
        c.get(..n)
            .unwrap_or(&[])
            .iter()
            .any(|x| !x.is_finite() || x.abs() > FAULT_ABS)
    })
}

/// Fader × pan as `(left, right)` gains; without pan (master, Plan
/// Ruling 4) both are the fader gain. Also used on the audio thread by
/// automation (it neither allocates nor panics). // RT
pub fn strip_gains(gain_db: f64, pan: f64, apply_pan: bool) -> (f32, f32) {
    let g = core_model::mixer::db_to_gain(gain_db);
    if apply_pan {
        let (l, r) = core_model::mixer::pan_gains(pan);
        ((g * l) as f32, (g * r) as f32)
    } else {
        (g as f32, g as f32)
    }
}
