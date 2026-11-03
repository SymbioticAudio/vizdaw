//! Kontrol S-Series MK3 feedback, byte by byte (plan 5f-4 Task 3; spec
//! §8.4).

mod common;

use common::view;
use core_model::automation::AutomationTarget as T;
use core_model::ids::PluginInstanceId;
use core_model::mixer::ParamId;
use core_surface::kontrol::{self, Kontrol, sysex};
use core_surface::{MixerView, ParamView, PluginPageView, Surface};
use std::time::{Duration, Instant};

const V4: [u8; 3] = [0xBF, 0x01, 0x04];

/// A keyboard that answered, and what its first feedback of `v` sent.
fn fed(v: &MixerView, first: usize) -> (Kontrol, Instant, Vec<Vec<u8>>) {
    let now = Instant::now();
    let mut k = Kontrol::new();
    let _ = k.hello();
    let _ = k.input(&V4, now);
    let sent = k.feedback(v, first, now);
    (k, now, sent)
}

#[track_caller]
fn has(sent: &[Vec<u8>], m: &[u8]) {
    assert!(sent.iter().any(|x| x == m), "{m:02x?} not in {sent:02x?}");
}

#[track_caller]
fn lacks(sent: &[Vec<u8>], m: &[u8]) {
    assert!(!sent.iter().any(|x| x == m), "{m:02x?} in {sent:02x?}");
}

fn param(i: u64, name: &str) -> ParamView {
    ParamView {
        target: T::PluginParam(PluginInstanceId(5), ParamId(i)),
        name: name.into(),
        value: 0.2,
        text: "0.20".into(),
    }
}

#[test]
fn the_first_feedback_names_vizdaw_then_shows_the_eight_slots() {
    let mut v = view(3);
    v.tempo = 120.0;
    let (_, _, sent) = fed(&v, 0);
    assert_eq!(sent.first(), Some(&sysex(0x07, 1, 0, b"Generic")));
    for m in [
        sysex(0x40, 3, 0, &[]),
        sysex(0x48, 0, 0, b"Strip 1"),
        sysex(0x48, 0, 2, b"Strip 3"),
        sysex(0x40, 0, 3, &[]),
        sysex(0x48, 0, 3, b""),
        sysex(0x46, 0, 0, b"-6.1 dB"),
        sysex(0x47, 0, 0, b"C"),
        sysex(0x4B, 0, 0, b"#46B45A"),
        sysex(0x43, 0, 0, &[]),
        sysex(0x44, 0, 0, &[]),
        sysex(0x45, 0, 0, &[]),
        sysex(0x42, 0, 0, &[]),
        sysex(0x4A, 0, 0, &[]),
        sysex(0x41, 0, 0, b""),
        sysex(0x19, 0, 0, &[0x00, 0x61, 0x6B, 0x17, 0x00]),
        sysex(0x49, 2, 0, &[0; 16]),
        vec![0xBF, 0x50, 0x40],
        vec![0xBF, 0x58, 0x40],
        vec![0xBF, 0x10, 0x00],
        vec![0xBF, 0x14, 0x01],
        vec![0xBF, 0x12, 0x00],
        vec![0xBF, 0x16, 0x00],
        vec![0xBF, 0x13, 0x00],
        vec![0xBF, 0x18, 0x01],
        vec![0xBF, 0x23, 0x00],
        vec![0xBF, 0x68, 0x00],
        vec![0xBF, 0x05, 0x00],
    ] {
        has(&sent, &m);
    }
}

#[test]
fn only_what_changed_is_sent_again() {
    let (mut k, now, _) = fed(&view(3), 0);
    assert!(k.feedback(&view(3), 0, now).is_empty(), "nothing changed");
    let mut v = view(3);
    v.strips[1].mute = true;
    assert_eq!(k.feedback(&v, 0, now), vec![sysex(0x43, 1, 1, &[])]);
    v.strips[0].fader = 0.8;
    let mut sent = k.feedback(&v, 0, now);
    sent.sort();
    let mut want = vec![vec![0xBF, 0x50, 0x66], sysex(0x46, 0, 0, b"6.2 dB")];
    want.sort();
    assert_eq!(sent, want);
}

#[test]
fn refresh_sends_the_state_again_but_not_the_name() {
    let (mut k, now, _) = fed(&view(3), 0);
    k.refresh();
    let again = k.feedback(&view(3), 0, now);
    has(&again, &sysex(0x48, 0, 0, b"Strip 1"));
    lacks(&again, &sysex(0x07, 1, 0, b"Generic"));
}

#[test]
fn mute_solo_arm_select_and_muted_by_solo_light() {
    let mut v = view(3);
    v.strips[0].solo = true;
    v.soloing = true;
    v.strips[1].arm = true;
    v.strips[2].select = true;
    let (_, _, sent) = fed(&v, 0);
    for m in [
        sysex(0x44, 1, 0, &[]),
        sysex(0x4A, 0, 0, &[]),
        sysex(0x4A, 1, 1, &[]),
        sysex(0x45, 1, 1, &[]),
        sysex(0x42, 1, 2, &[]),
        vec![0xBF, 0x68, 0x03],
        vec![0xBF, 0x66, 0x00],
        vec![0xBF, 0x67, 0x00],
        vec![0xBF, 0x69, 0x01],
    ] {
        has(&sent, &m);
    }
}

#[test]
fn volume_and_pan_read_as_text() {
    let mut v = view(4);
    v.strips[0].fader = 0.0;
    v.strips[1].fader = 0.631;
    v.strips[0].pan = 0.0;
    v.strips[1].pan = 1.0;
    v.strips[2].pan = 0.25;
    v.strips[3].name = "Ünïcode name that is much longer than thirty-two bytes".into();
    // Pre-flight M9: below −99.9 dB the text is "-inf dB" (12 + 60·log10
    // 0.01 = −108); 0.02 is −89.9 dB. M10: a pan that rounds to 0 % is "C".
    v.strips[2].fader = 0.01;
    v.strips[3].fader = 0.02;
    v.strips[3].pan = 0.502;
    let (_, _, sent) = fed(&v, 0);
    for m in [
        sysex(0x46, 0, 0, b"-inf dB"),
        sysex(0x46, 0, 1, b"0.0 dB"),
        sysex(0x46, 0, 2, b"-inf dB"),
        sysex(0x46, 0, 3, b"-89.9 dB"),
        sysex(0x47, 0, 0, b"L100"),
        sysex(0x47, 0, 1, b"R100"),
        sysex(0x47, 0, 2, b"L50"),
        sysex(0x47, 0, 3, b"C"),
        sysex(0x48, 0, 3, b"?n?code name that is much longer"),
        vec![0xBF, 0x50, 0x00],
    ] {
        has(&sent, &m);
    }
    for m in sent.iter().filter(|m| m.get(10) == Some(&0x46)) {
        assert!(
            m.len() - 14 <= 8,
            "a volume text is at most 8 characters: {m:02x?}"
        );
    }
}

#[test]
fn meters_are_one_message_of_sixteen_levels() {
    let mut v = view(2);
    v.strips[0].meter = (1.0, 0.5);
    v.strips[1].meter = (0.25, 0.0);
    let (mut k, now, sent) = fed(&v, 0);
    let mut levels = [0u8; 16];
    levels[..4].copy_from_slice(&[127, 114, 102, 0]);
    let vu = sysex(0x49, 2, 0, &levels);
    has(&sent, &vu);
    assert!(kontrol::is_meter(&vu));
    assert!(k.feedback(&v, 0, now).is_empty(), "the same levels go once");
}

#[test]
fn the_navigation_lights_say_where_one_can_go() {
    let mut v = view(12);
    v.strips[0].select = true;
    let (_, _, sent) = fed(&v, 0);
    has(&sent, &[0xBF, 0x30, 0x02]);
    has(&sent, &[0xBF, 0x31, 0x02]);
    let mut v = view(12);
    v.strips[11].select = true;
    let (_, _, sent) = fed(&v, 4);
    has(&sent, &[0xBF, 0x30, 0x01]);
    has(&sent, &[0xBF, 0x31, 0x01]);
    let mut v = view(12);
    v.strips[5].select = true;
    let (_, _, sent) = fed(&v, 2);
    has(&sent, &[0xBF, 0x30, 0x03]);
    has(&sent, &[0xBF, 0x31, 0x03]);
    let (_, _, sent) = fed(&MixerView::default(), 0);
    has(&sent, &[0xBF, 0x30, 0x00]);
    has(&sent, &[0xBF, 0x31, 0x00]);
    has(&sent, &[0xBF, 0x32, 0x00]);
    has(&sent, &[0xBF, 0x36, 0x00]);
}

#[test]
fn the_tempo_follows_the_view() {
    let mut v = view(1);
    v.tempo = 90.0;
    let (mut k, now, sent) = fed(&v, 0);
    has(&sent, &sysex(0x19, 0, 0, &[0x2B, 0x01, 0x65, 0x1F, 0x00]));
    v.tempo = 0.0;
    assert!(
        k.feedback(&v, 0, now).is_empty(),
        "no tempo: nothing is sent"
    );
    v.tempo = 120.0;
    assert_eq!(
        k.feedback(&v, 0, now),
        vec![sysex(0x19, 0, 0, &[0x00, 0x61, 0x6B, 0x17, 0x00])]
    );
}

/// Ruling 8 (owner, 2026-11-03): Count-in and Automation light with
/// VizDAW's state; Tap is dark while recording; Metro and Quantize dark.
#[test]
fn transport_lights_follow_play_record_loop_count_in_and_suspend() {
    let mut v = view(1);
    v.playing = true;
    v.recording = true;
    v.looping = true;
    v.count_in = true;
    v.automation_suspended = true;
    let (mut k, now, sent) = fed(&v, 0);
    for m in [
        [0xBF, 0x10, 0x01],
        [0xBF, 0x14, 0x00],
        [0xBF, 0x12, 0x01],
        [0xBF, 0x16, 0x01],
        [0xBF, 0x11, 0x01],
        [0xBF, 0x20, 0x01],
        [0xBF, 0x21, 0x01],
        [0xBF, 0x13, 0x01],
        [0xBF, 0x23, 0x01],
        [0xBF, 0x18, 0x00],
        [0xBF, 0x17, 0x00],
        [0xBF, 0x22, 0x00],
    ] {
        has(&sent, &m);
    }
    v.recording = false;
    v.count_in = false;
    v.automation_suspended = false;
    let mut sent = k.feedback(&v, 0, now);
    sent.sort();
    let mut want = vec![
        vec![0xBF, 0x12, 0x00],
        vec![0xBF, 0x13, 0x00],
        vec![0xBF, 0x18, 0x01],
        vec![0xBF, 0x23, 0x00],
    ];
    want.sort();
    assert_eq!(sent, want, "they follow VizDAW, whoever changed it");
}

#[test]
fn plugin_mode_shows_the_chain_names_values_page_and_preset() {
    let mut v = view(1);
    v.page = Some(PluginPageView {
        chain: vec!["Synth".into(), "Delay".into()],
        slot: 1,
        params: vec![param(0, "Time"), param(1, "Mix"), param(2, "Fbk")],
        page: 0,
        pages: 2,
        page_name: "Page 1".into(),
        preset: Some(1),
        presets: 3,
        preset_name: "Warm".into(),
    });
    let (mut k, now, mixer) = fed(&v, 0);
    lacks(&mixer, &sysex(0x71, 0, 0, b"Synth\0Delay\0\0"));
    let _ = k.input(&[0xBF, 0x05, 0x01], now);
    let sent = k.feedback(&v, 0, now);
    for m in [
        sysex(0x71, 0, 0, b"Synth\0Delay\0\0"),
        sysex(0x70, 0, 1, &[]),
        sysex(0x72, 0, 0, b"Time"),
        sysex(0x73, 0, 0, b"0.20"),
        sysex(0x72, 0, 2, b"Fbk"),
        sysex(0x72, 0, 3, b""),
        sysex(0x73, 0, 7, b""),
        sysex(0x74, 2, 0, &[]),
        sysex(0x75, 0, 0, b"Page 1"),
        sysex(0x76, 0, 0, b"Warm"),
        vec![0xBF, 0x70, 0x19],
        vec![0xBF, 0x73, 0x00],
        vec![0xBF, 0x36, 0x03],
        vec![0xBF, 0x31, 0x02],
        vec![0xBF, 0x05, 0x01],
    ] {
        has(&sent, &m);
    }
}

#[test]
fn plugin_mode_without_a_plugin_clears_the_view() {
    let (mut k, now, _) = fed(&view(1), 0);
    let _ = k.input(&[0xBF, 0x05, 0x01], now);
    let sent = k.feedback(&view(1), 0, now);
    for m in [
        sysex(0x71, 0, 0, b"\0"),
        sysex(0x74, 0, 0, &[]),
        sysex(0x72, 0, 0, b""),
        sysex(0x76, 0, 0, b""),
        vec![0xBF, 0x36, 0x00],
    ] {
        has(&sent, &m);
    }
}

#[test]
fn goodbye_empties_the_slots_then_says_goodbye() {
    let (mut k, _, _) = fed(&view(3), 0);
    let mut want: Vec<Vec<u8>> = (0..8).map(|i| sysex(0x40, 0, i, &[])).collect();
    want.push(vec![0xBF, 0x02, 0x00]);
    assert_eq!(k.goodbye(), want);
    assert!(!k.online());
}

#[test]
fn a_blank_view_empties_the_slots_it_showed() {
    let (mut k, now, _) = fed(&view(3), 0);
    let sent = k.feedback(&MixerView::default(), 0, now);
    for i in 0..3 {
        has(&sent, &sysex(0x40, 0, i, &[]));
        has(&sent, &sysex(0x48, 0, i, b""));
    }
    lacks(&sent, &sysex(0x40, 0, 5, &[]));
}

#[test]
fn a_bank_shows_the_next_eight_strips() {
    let (mut k, now, _) = fed(&view(12), 0);
    let sent = k.feedback(&view(12), 8, now);
    has(&sent, &sysex(0x48, 0, 0, b"Strip 9"));
    has(&sent, &sysex(0x40, 0, 4, &[]));
}

/// Ruling 4 (owner, 2026-11-03): a keyboard that said goodbye is asked
/// again 2 s later, and its answer gets the identity and everything the
/// first feedback sent, not only what changed.
#[test]
fn a_keyboard_pulled_back_into_daw_mode_gets_everything_again() {
    let mut v = view(3);
    v.tempo = 120.0;
    let (mut k, now, first) = fed(&v, 0);
    let _ = k.input(&[0xBF, 0x02, 0x00], now);
    assert!(!k.online());
    let at = now + kontrol::HELLO_EVERY;
    assert!(k.feedback(&v, 0, at - Duration::from_millis(1)).is_empty());
    assert_eq!(
        k.feedback(&v, 0, at),
        vec![vec![0xBF, 0x01, 0x04]],
        "VizDAW asks again"
    );
    let _ = k.input(&V4, at);
    let again = k.feedback(&v, 0, at);
    assert_eq!(again.first(), Some(&sysex(0x07, 1, 0, b"Generic")));
    for m in &first {
        has(&again, m);
    }
}

/// Ruling 5: after the keyboard's own tempo VizDAW's goes back to it; the
/// keyboard's echo of VizDAW's tempo is not answered.
#[test]
fn the_tempo_is_sent_again_after_the_keyboards_own() {
    let mut v = view(1);
    v.tempo = 120.0;
    let (mut k, now, _) = fed(&v, 0);
    let ours = sysex(0x19, 0, 0, &[0x00, 0x61, 0x6B, 0x17, 0x00]);
    assert!(k.input(&ours, now).is_empty(), "the echo acts on nothing");
    assert!(k.feedback(&v, 0, now).is_empty(), "and is not answered");
    let theirs = sysex(0x19, 0, 0, &kontrol::tempo_bytes(90.0).unwrap());
    assert_eq!(k.input(&theirs, now).len(), 1, "a tempo of its own");
    assert_eq!(
        k.feedback(&v, 0, now),
        vec![ours],
        "VizDAW's tempo goes back (the session refused or has not applied it)"
    );
    assert_eq!(k.ignored(), 0);
}

/// Pre-flight I3: more than 127 pages, a page past 127 and a preset past
/// 127 are sent as 127; every SysEx data byte stays 7-bit.
#[test]
fn more_than_127_pages_stay_seven_bit() {
    let mut v = view(1);
    v.page = Some(PluginPageView {
        chain: vec!["Big".into()],
        slot: 0,
        params: vec![param(0, "A")],
        page: 150,
        pages: 300,
        page_name: "Page 151".into(),
        preset: Some(200),
        presets: 300,
        preset_name: "P".into(),
    });
    let (mut k, now, _) = fed(&v, 0);
    let _ = k.input(&[0xBF, 0x05, 0x01], now);
    let sent = k.feedback(&v, 0, now);
    has(&sent, &sysex(0x74, 0x7F, 0x7F, &[]));
    for m in sent.iter().filter(|m| m.first() == Some(&0xF0)) {
        assert!(m[1..m.len() - 1].iter().all(|b| *b <= 0x7F), "{m:02x?}");
    }
    for m in sent.iter().filter(|m| m.first() == Some(&0xBF)) {
        assert!(m[1..].iter().all(|b| *b <= 0x7F), "{m:02x?}");
    }
}
