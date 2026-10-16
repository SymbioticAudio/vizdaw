use core_engine::fixtures::{BAR, demo_document};
use core_model::document::{DestKind, Document};
use core_model::ids::SeqId;
use core_model::query::{drum_view, seq_length, unused_sequences, use_count};
use core_model::rules::validate;
use core_model::time::Tick;

fn find_sequence(doc: &Document, name: &str) -> Option<SeqId> {
    doc.sequences()
        .values()
        .find(|s| s.name == name)
        .map(|s| s.id)
}

#[test]
fn demo_matches_the_app_shell_spec() {
    let doc = demo_document().unwrap();
    assert!(validate(&doc).is_empty(), "{:?}", validate(&doc));
    let beat = find_sequence(&doc, "Beat").unwrap();
    let groove = find_sequence(&doc, "Groove").unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    assert_eq!(use_count(&doc, beat), 1);
    assert_eq!(use_count(&doc, groove), 2);
    assert_eq!(unused_sequences(&doc), vec![song]);
    assert_eq!(seq_length(&doc, beat).unwrap(), Tick(BAR));
    assert_eq!(seq_length(&doc, groove).unwrap(), Tick(4 * BAR));
    assert_eq!(seq_length(&doc, song).unwrap(), Tick(12 * BAR));

    let channels: Vec<(String, u8)> = doc
        .instruments()
        .values()
        .map(|d| match &d.kind {
            DestKind::MidiEndpoint {
                endpoint_ref,
                channel,
                ..
            } => {
                assert_eq!(endpoint_ref, "builtin:synth");
                (d.name.clone(), *channel)
            }
            DestKind::PluginInstrument { .. } => panic!("no plugins in the demo"),
        })
        .collect();
    assert_eq!(
        channels,
        vec![
            ("Built-in Piano".to_string(), 0),
            ("Built-in Bass".to_string(), 1),
            ("Built-in Drums".to_string(), 9)
        ]
    );

    let beat_seq = doc.sequence(beat).unwrap();
    let kit = beat_seq.lanes.front().unwrap();
    let rows: Vec<(String, u8, usize)> = drum_view(&doc, beat, kit.id)
        .unwrap()
        .into_iter()
        .map(|(r, ev)| (r.name, r.note, ev.len()))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("Kick".to_string(), 36, 2),
            ("Snare".to_string(), 38, 2),
            ("Closed Hat".to_string(), 42, 8)
        ]
    );

    let tempo = &doc.sequence(song).unwrap().tempo;
    let bpm: Vec<(i64, u32, bool)> = tempo
        .points()
        .iter()
        .map(|p| (p.tick.0, p.bpm.0, p.ramp))
        .collect();
    assert_eq!(
        bpm,
        vec![
            (0, 120_000, false),
            (10 * BAR, 120_000, true),
            (12 * BAR, 90_000, false)
        ]
    );
}

#[test]
fn demo_is_deterministic() {
    let a = core_model::file::to_json(&demo_document().unwrap()).unwrap();
    let b = core_model::file::to_json(&demo_document().unwrap()).unwrap();
    assert_eq!(a, b);
}
