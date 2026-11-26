//! Pointer gestures (Swift `ToolMachine`): a press becomes a drag once the
//! pointer moved more than [`DEAD_ZONE`] points; release ends it; Escape
//! cancels it. A press released inside the dead zone is a click. `Hit` is
//! what the press landed on (editor-specific).

use super::{DEAD_ZONE, Point};

/// Keyboard modifiers of a pointer event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub option: bool,
    pub command: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        control: false,
        option: false,
        command: false,
    };
    pub const SHIFT: Modifiers = Modifiers {
        shift: true,
        ..Self::NONE
    };
    pub const CONTROL: Modifiers = Modifiers {
        control: true,
        ..Self::NONE
    };
    pub const OPTION: Modifiers = Modifiers {
        option: true,
        ..Self::NONE
    };
    pub const COMMAND: Modifiers = Modifiers {
        command: true,
        ..Self::NONE
    };
}

/// Where the machine is.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase<Hit> {
    Idle,
    Pressed {
        hit: Hit,
        at: Point,
        clicks: u32,
        modifiers: Modifiers,
    },
    Dragging {
        hit: Hit,
        from: Point,
    },
}

/// What a pointer event means.
#[derive(Debug, Clone, PartialEq)]
pub enum MachineEvent<Hit> {
    /// Released inside the dead zone: at the press point, with the
    /// press's modifiers.
    Click {
        hit: Hit,
        at: Point,
        clicks: u32,
        modifiers: Modifiers,
    },
    /// The pointer left the dead zone (its first `Move` follows at once);
    /// the modifiers are those of that move.
    Begin {
        hit: Hit,
        from: Point,
        modifiers: Modifiers,
    },
    Move {
        hit: Hit,
        from: Point,
        to: Point,
    },
    /// Released after a drag, with the modifiers at release.
    End {
        hit: Hit,
        from: Point,
        to: Point,
        modifiers: Modifiers,
    },
    /// Escape, or a press while a drag was open (its release was lost).
    Cancel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolMachine<Hit> {
    phase: Phase<Hit>,
}

impl<Hit> Default for ToolMachine<Hit> {
    fn default() -> Self {
        ToolMachine { phase: Phase::Idle }
    }
}

impl<Hit: Clone> ToolMachine<Hit> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn phase(&self) -> &Phase<Hit> {
        &self.phase
    }

    /// A press. A drag still open (its release was lost) is cancelled.
    pub fn down(
        &mut self,
        hit: Hit,
        at: Point,
        clicks: u32,
        modifiers: Modifiers,
    ) -> Vec<MachineEvent<Hit>> {
        let mut out = vec![];
        if matches!(self.phase, Phase::Dragging { .. }) {
            out.push(MachineEvent::Cancel);
        }
        self.phase = Phase::Pressed {
            hit,
            at,
            clicks,
            modifiers,
        };
        out
    }

    pub fn drag(&mut self, to: Point, modifiers: Modifiers) -> Vec<MachineEvent<Hit>> {
        match &self.phase {
            Phase::Idle => vec![],
            Phase::Pressed { hit, at, .. } => {
                let (hit, from) = (hit.clone(), *at);
                if (to.x - from.x).hypot(to.y - from.y) <= DEAD_ZONE {
                    return vec![];
                }
                self.phase = Phase::Dragging {
                    hit: hit.clone(),
                    from,
                };
                vec![
                    MachineEvent::Begin {
                        hit: hit.clone(),
                        from,
                        modifiers,
                    },
                    MachineEvent::Move { hit, from, to },
                ]
            }
            Phase::Dragging { hit, from } => vec![MachineEvent::Move {
                hit: hit.clone(),
                from: *from,
                to,
            }],
        }
    }

    pub fn up(&mut self, at: Point, modifiers: Modifiers) -> Vec<MachineEvent<Hit>> {
        match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Idle => vec![],
            Phase::Pressed {
                hit,
                at: start,
                clicks,
                modifiers: down,
            } => vec![MachineEvent::Click {
                hit,
                at: start,
                clicks,
                modifiers: down,
            }],
            Phase::Dragging { hit, from } => vec![MachineEvent::End {
                hit,
                from,
                to: at,
                modifiers,
            }],
        }
    }

    /// Escape: cancels a drag; a press inside the dead zone is forgotten.
    pub fn cancel(&mut self) -> Vec<MachineEvent<Hit>> {
        match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Dragging { .. } => vec![MachineEvent::Cancel],
            _ => vec![],
        }
    }
}
