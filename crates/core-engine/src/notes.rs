//! Active-note tracker: which notes sound per target, channel and pitch,
//! and which channels hold the sustain pedal. // RT

use crate::midi1::Midi1Msg;
use crate::schedule::{MAX_TARGETS, TargetIdx};

/// Fixed-size note and sustain state; no allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveNotes {
    on: [[u128; 16]; MAX_TARGETS],
    sustain: [u16; MAX_TARGETS],
}

impl Default for ActiveNotes {
    fn default() -> Self {
        ActiveNotes {
            on: [[0; 16]; MAX_TARGETS],
            sustain: [0; MAX_TARGETS],
        }
    }
}

impl ActiveNotes {
    /// Updates the state from a message sent to `target`. // RT
    pub fn observe(&mut self, target: TargetIdx, msg: &[u8]) {
        let t = usize::from(target.0);
        let (Some(&status), Some(notes), Some(sus)) =
            (msg.first(), self.on.get_mut(t), self.sustain.get_mut(t))
        else {
            return;
        };
        let ch = usize::from(status & 0x0F);
        let d1 = msg.get(1).copied().unwrap_or(0) & 0x7F;
        let d2 = msg.get(2).copied().unwrap_or(0);
        let Some(bits) = notes.get_mut(ch) else {
            return;
        };
        match status & 0xF0 {
            0x90 if d2 > 0 => *bits |= 1u128 << d1,
            0x90 | 0x80 => *bits &= !(1u128 << d1),
            0xB0 if d1 == 64 => {
                if d2 >= 64 {
                    *sus |= 1 << ch;
                } else {
                    *sus &= !(1 << ch);
                }
            }
            0xB0 if d1 == 123 => *bits = 0,
            _ => {}
        }
    }

    /// Calls `send` with a note-off for every sounding note of `target` and
    /// CC 64 = 0 for every held pedal, then forgets them. // RT
    pub fn release(&mut self, target: TargetIdx, mut send: impl FnMut(Midi1Msg)) {
        let t = usize::from(target.0);
        let (Some(notes), Some(sus)) = (self.on.get_mut(t), self.sustain.get_mut(t)) else {
            return;
        };
        for (ch, bits) in notes.iter_mut().enumerate() {
            let status = 0x80 | (ch as u8 & 0x0F);
            while *bits != 0 {
                let pitch = bits.trailing_zeros() as u8;
                send(Midi1Msg::three(status, pitch, 0));
                *bits &= !(1u128 << pitch);
            }
            if *sus & (1 << ch) != 0 {
                send(Midi1Msg::three(0xB0 | (ch as u8 & 0x0F), 64, 0));
            }
        }
        *sus = 0;
    }

    /// True when any note of any target sounds.
    pub fn any(&self) -> bool {
        self.on.iter().flatten().any(|b| *b != 0) || self.sustain.iter().any(|s| *s != 0)
    }
}
