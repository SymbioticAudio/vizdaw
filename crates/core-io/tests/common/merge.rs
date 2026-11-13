//! The merge tests' project (plan 6e): one document that exercises every
//! identity the snapshot keeps. Task 4 adds the merge harness.

use core_engine::fixtures::{BAR, Builder, Q};
use core_model::command::{
    AudioCmd, Command, CrossfadeLength, LaneCmd, MediaCmd, MixerCmd, SequenceCmd,
};
use core_model::document::{Document, MediaOrigin};
use core_model::event::{Fade, FadeShape};
use core_model::ids::{AnyId, MediaId, SeqId};
use core_model::mixer::{AuxSend, PluginFormat, PluginRef, StripKind};
use core_model::sequence::{Marker, SeqLength};
use core_model::time::Tick;

/// A CLAP plug-in `id`.
pub fn clap(id: &str) -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: id.into(),
        name: id.into(),
        vendor: "Example".into(),
        version: "1.0".into(),
    }
}

/// A media item `Media/<name>.caf` with the content hash `hash-<name>`.
pub fn media(b: &mut Builder, name: &str, frames: u64, channels: u16) -> MediaId {
    let r = b
        .run(Command::Media(MediaCmd::Add {
            path: format!("Media/{name}.caf"),
            sample_rate: 48_000,
            channels,
            length_frames: frames,
            content_hash: Some(format!("hash-{name}")),
            name: Some(name.into()),
            origin: Some(MediaOrigin::Imported {
                original_name: format!("{name}.wav"),
            }),
        }))
        .unwrap();
    r.created
        .iter()
        .find_map(|i| match i {
            AnyId::Media(m) => Some(*m),
            _ => None,
        })
        .unwrap()
}

fn no_fade() -> Fade {
    Fade {
        length_frames: 0,
        shape: FadeShape::Linear,
    }
}

/// Sequence "Song" with two tracks:
///
/// - "Bass" on CLAP instrument "Bass Synth" (`org.example.Bass`): a note at
///   tick 0, then sequence "Riff" (two notes, one bar) used at bar 1 once
///   and at bar 3 looped twice. Its strip is at −6 dB, sends to bus
///   "Reverb" at −10 dB and has the insert `org.example.Comp`;
/// - "Keys" on CLAP instrument "Keys Synth" (`org.example.Keys`): a note at
///   tick 0 and, so it is split, two regions of media "loop" (at 0 and one
///   beat, the second crossfaded into the first by 4 800 frames).
///
/// Markers "Verse" at 0 and "Chorus" at bar 2.
pub fn fixture() -> (Document, SeqId) {
    let mut b = Builder::new();
    let (bass_dest, _) = b
        .plugin_dest("Bass Synth", clap("org.example.Bass"))
        .unwrap();
    let (seq, bass) = b.sequence("Song", "Bass", Some(bass_dest)).unwrap();
    let (keys_dest, _) = b
        .plugin_dest("Keys Synth", clap("org.example.Keys"))
        .unwrap();
    let keys = b.lane(seq, "Keys", Some(keys_dest)).unwrap();

    b.notes(seq, bass, &[(0, 36, 0x8000, Q)]).unwrap();
    let (riff, rl) = b.sequence("Riff", "Riff", Some(bass_dest)).unwrap();
    b.notes(riff, rl, &[(0, 40, 0x8000, Q), (Q, 43, 0x9000, Q)])
        .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq: riff,
        length: SeqLength::Locked(Tick(BAR)),
    }))
    .unwrap();
    b.reference(seq, bass, BAR, riff, 1, 0).unwrap();
    b.reference(seq, bass, 3 * BAR, riff, 2, 0).unwrap();

    b.notes(seq, keys, &[(0, 60, 0x8000, 2 * Q)]).unwrap();
    let m = media(&mut b, "loop", 96_000, 2);
    b.region(seq, keys, 0, m, 0, 96_000, 0.0, no_fade(), no_fade())
        .unwrap();
    let second = b
        .region(seq, keys, Q, m, 0, 96_000, 0.0, no_fade(), no_fade())
        .unwrap();
    b.run(Command::Audio(AudioCmd::Crossfade {
        event: second,
        length: CrossfadeLength::Frames(4_800),
        shape: FadeShape::EqualPower,
    }))
    .unwrap();

    let strip = b.strip_of(StripKind::Instrument(bass_dest)).unwrap();
    b.mixer(MixerCmd::SetGain {
        strip,
        gain_db: -6.0,
    })
    .unwrap();
    let reverb = b.bus("Reverb").unwrap();
    b.mixer(MixerCmd::AddSend {
        strip,
        send: AuxSend {
            target: reverb,
            level_db: -10.0,
            pre_fader: false,
            enabled: true,
        },
    })
    .unwrap();
    b.insert(strip, clap("org.example.Comp")).unwrap();

    b.run(Command::Sequence(SequenceCmd::SetMarkers {
        seq,
        markers: vec![
            Marker {
                tick: Tick(0),
                name: "Verse".into(),
            },
            Marker {
                tick: Tick(2 * BAR),
                name: "Chorus".into(),
            },
        ]
        .into_iter()
        .collect(),
    }))
    .unwrap();
    (b.finish(), seq)
}

/// Sequence "Song" (eight bars) with the two shapes in which the export
/// repeats a tag:
///
/// - "Pad A" and "Pad B" both play CLAP instrument "Pad Synth"
///   (`org.example.Pad`), whose strip has the insert `org.example.Chorus`,
///   so its channel is copied into each track; each has a note at tick 0;
/// - "Loop" on CLAP instrument "Arp Synth" (`org.example.Arp`) loops its
///   first two bars, and uses sequence "Arp" (two notes, one bar) at bar 1
///   twice, across the loop's end, so the use is one clip per pass.
pub fn shared_fixture() -> (Document, SeqId) {
    let mut b = Builder::new();
    let (pad, _) = b.plugin_dest("Pad Synth", clap("org.example.Pad")).unwrap();
    let (seq, pad_a) = b.sequence("Song", "Pad A", Some(pad)).unwrap();
    let pad_b = b.lane(seq, "Pad B", Some(pad)).unwrap();
    b.notes(seq, pad_a, &[(0, 60, 0x8000, Q)]).unwrap();
    b.notes(seq, pad_b, &[(0, 64, 0x8000, Q)]).unwrap();
    let strip = b.strip_of(StripKind::Instrument(pad)).unwrap();
    b.insert(strip, clap("org.example.Chorus")).unwrap();

    let (arp, _) = b.plugin_dest("Arp Synth", clap("org.example.Arp")).unwrap();
    let lp = b.lane(seq, "Loop", Some(arp)).unwrap();
    let (riff, rl) = b.sequence("Arp", "Arp", Some(arp)).unwrap();
    b.notes(riff, rl, &[(0, 48, 0x8000, Q), (2 * Q, 55, 0x9000, Q)])
        .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq: riff,
        length: SeqLength::Locked(Tick(BAR)),
    }))
    .unwrap();
    b.reference(seq, lp, BAR, riff, 2, 0).unwrap();
    b.run(Command::Lane(LaneCmd::SetLength {
        seq,
        lane: lp,
        length: Some(Tick(2 * BAR)),
        looped: true,
        locked: false,
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(8 * BAR)),
    }))
    .unwrap();
    (b.finish(), seq)
}
