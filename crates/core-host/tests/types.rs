use core_engine::rt_check::{self, CountingAlloc};
use core_host::queue::param_queues;
use core_host::{
    AudioPortInfo, MAX_PARAM_EVENTS, MidiEvent, ParamChange, ParamEvent, ParamInfo, ParamOut,
    PluginInfo, PluginKind, PortRole, Tail,
};
use core_model::mixer::{ParamId, PluginFormat, PluginRef};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn ducker() -> PluginInfo {
    let port = |name: &str, role| AudioPortInfo {
        name: name.into(),
        channels: 2,
        role,
    };
    PluginInfo {
        plugin: PluginRef {
            format: PluginFormat::Clap,
            id: "vizdaw.test.ducker".into(),
            name: "Test Ducker".into(),
            vendor: "VizDAW".into(),
            version: "1.0.0".into(),
        },
        kind: PluginKind::Effect,
        inputs: vec![
            port("In", PortRole::Main),
            port("Sidechain", PortRole::Sidechain),
        ],
        outputs: vec![port("Out", PortRole::Main)],
        note_input: false,
        params: vec![ParamInfo {
            id: ParamId(0),
            name: "Depth".into(),
            module: String::new(),
            min: 0.0,
            max: 1.0,
            default: 1.0,
            stepped: false,
            automatable: true,
            hidden: false,
        }],
        latency: 0,
        tail: Tail::Frames(480),
    }
}

#[test]
fn plugin_info_round_trips_as_json_and_finds_ports() {
    let info = ducker();
    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("\"role\":\"sidechain\""), "{json}");
    let back: PluginInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(back, info);
    assert_eq!(info.main_input(), Some(0));
    assert_eq!(info.sidechain_input(), Some(1));
    assert_eq!(info.output_channels(), 2);
    assert_eq!(info.param(ParamId(0)).unwrap().name, "Depth");
    assert!(info.param(ParamId(9)).is_none());
}

#[test]
fn midi_events_keep_up_to_three_bytes() {
    let e = MidiEvent::new(7, &[0x90, 60, 100]);
    assert_eq!((e.frame, e.as_slice()), (7, &[0x90u8, 60, 100][..]));
    assert_eq!(MidiEvent::new(0, &[0xC0, 5]).as_slice(), &[0xC0, 5]);
    assert_eq!(MidiEvent::new(0, &[0xF0, 1, 2, 3, 0xF7]).len, 3);
}

#[test]
fn param_out_is_bounded_and_never_allocates() {
    let mut out = ParamOut::default();
    rt_check::arm();
    let before = rt_check::count();
    for i in 0..MAX_PARAM_EVENTS as u64 {
        assert!(out.push(ParamEvent::Value(ParamId(i), 0.5)));
    }
    assert!(!out.push(ParamEvent::End(ParamId(0))), "full");
    let n = out.as_slice().len();
    out.clear();
    let allocs = rt_check::count() - before;
    rt_check::disarm();
    assert_eq!(n, MAX_PARAM_EVENTS);
    assert!(out.as_slice().is_empty());
    assert_eq!(allocs, 0);
}

#[test]
fn queues_carry_values_in_and_gestures_out() {
    let ((mut tx, mut gestures), (mut rx, mut gtx)) = param_queues();
    assert!(tx.send(ParamId(1), 0.25));
    assert!(tx.send(ParamId(2), 0.75));
    let mut buf = [ParamChange::default(); 8];
    rt_check::arm();
    let before = rt_check::count();
    let n = rx.drain_into(&mut buf);
    let lost = gtx.send_all(&[
        ParamEvent::Begin(ParamId(1)),
        ParamEvent::Value(ParamId(1), 0.3),
        ParamEvent::End(ParamId(1)),
    ]);
    let allocs = rt_check::count() - before;
    rt_check::disarm();
    assert_eq!(allocs, 0, "the audio-side ends never allocate");
    assert_eq!((n, lost), (2, 0));
    assert_eq!(
        (buf[0].id, buf[0].value, buf[1].id, buf[1].value),
        (ParamId(1), 0.25, ParamId(2), 0.75)
    );
    assert_eq!(
        gestures.drain(),
        vec![
            ParamEvent::Begin(ParamId(1)),
            ParamEvent::Value(ParamId(1), 0.3),
            ParamEvent::End(ParamId(1)),
        ]
    );
}
