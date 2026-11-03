//! Strip views shared by the surface tests (plan 5f-1). A module, not a
//! test file, so its helpers are not run as tests of their own.
#![allow(dead_code)]

use core_model::automation::AutomationMode;
use core_model::ids::StripId;
use core_surface::{MixerView, StripView, TrackKind};

pub fn strip(id: u64, name: &str) -> StripView {
    StripView {
        id: StripId(id),
        name: name.into(),
        fader: 0.5,
        pan: 0.5,
        send: Some(0.25),
        mute: false,
        solo: false,
        arm: false,
        select: false,
        mode: AutomationMode::Read,
        meter: (0.0, 0.0),
        kind: TrackKind::Audio,
    }
}

pub fn view(n: u64) -> MixerView {
    MixerView {
        strips: (1..=n).map(|i| strip(i, &format!("Strip {i}"))).collect(),
        master: Some(strip(100, "Master")),
        position: "1.1.1.0".into(),
        ..MixerView::default()
    }
}
