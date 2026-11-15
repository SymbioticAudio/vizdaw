//! Many sequences (and other id-keyed items) on small thread stacks: a
//! stack overflow aborts the process, so these run on a thread whose stack
//! is the size of a Swift cooperative-pool thread's (512 KiB).

use core_model::command::{
    Command, EventCmd, MediaCmd, MixerCmd, RoutingCmd, SequenceCmd, SettingsCmd, apply,
};
use core_model::document::{DestKind, Document};
use core_model::event::{EventBody, EventSpec};
use core_model::file;
use core_model::history::History;
use core_model::ids::{EventId, LaneId, SeqId};
use core_model::modify::ModifyTemplate;
use core_model::rule::SelectMode;
use core_model::selection::ViewFilter;
use core_model::time::Tick;

const SMALL_STACK: usize = 512 * 1024;
const MANY: usize = 20_000;

fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

fn new_seq(i: usize) -> Command {
    Command::Sequence(SequenceCmd::New {
        name: format!("S{i}"),
    })
}

fn many_sequences() -> Document {
    let mut d = Document::new();
    for i in 0..MANY {
        d = apply(&d, &new_seq(i)).unwrap().doc;
    }
    d
}

#[test]
fn many_new_sequences_and_undo_fit_a_small_stack() {
    let (n, after_undo) = on_small_stack(|| {
        let mut h = History::new(Document::new());
        let f = ViewFilter::default();
        for i in 0..MANY {
            h.execute(&new_seq(i), &f).unwrap();
        }
        let n = h.doc().sequences().len();
        for _ in 0..3 {
            h.undo().unwrap();
        }
        let after = h.doc().sequences().len();
        h.redo().unwrap();
        (n, after)
    });
    assert_eq!(n, MANY);
    assert_eq!(after_undo, MANY - 3);
}

#[test]
fn project_with_many_sequences_loads_on_a_small_stack() {
    // Built on a roomy stack; only the load runs on the small one.
    let text = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(|| file::to_json(&many_sequences()).unwrap())
        .unwrap()
        .join()
        .unwrap();
    let (n, last) = on_small_stack(move || {
        let d = file::from_json(&text).unwrap();
        let n = d.sequences().len();
        let last = d.sequence(SeqId(MANY as u64 - 1)).unwrap().name.clone();
        (n, last)
    });
    assert_eq!(n, MANY);
    assert_eq!(last, format!("S{}", MANY - 1));
}

/// Runs `cmds(i)` for `i` in `0..n` through a history on a small stack,
/// undoes and redoes a few, then saves and reloads the result on a small
/// stack; returns the reloaded document's count of `count`.
fn many_then_reload(
    n: usize,
    cmd: fn(usize) -> Command,
    count: fn(&Document) -> usize,
) -> (usize, usize) {
    let text = on_small_stack(move || {
        let mut h = History::new(Document::new());
        let f = ViewFilter::default();
        for i in 0..n {
            h.execute(&cmd(i), &f).unwrap();
        }
        for _ in 0..3 {
            h.undo().unwrap();
        }
        for _ in 0..3 {
            h.redo().unwrap();
        }
        file::to_json(h.doc()).unwrap()
    });
    on_small_stack(move || {
        let d = file::from_json(&text).unwrap();
        let c = count(&d);
        let again = file::from_json(&file::to_json(&d).unwrap()).unwrap();
        (c, count(&again))
    })
}

#[test]
fn many_destinations_and_their_strips_fit_a_small_stack() {
    let (n, again) = many_then_reload(
        MANY,
        |i| {
            Command::Routing(RoutingCmd::AddDestination {
                name: format!("D{i}"),
                kind: DestKind::MidiEndpoint {
                    endpoint_ref: format!("e{i}"),
                    group: 0,
                    channel: 0,
                },
                default_patch: None,
            })
        },
        |d| d.instruments().len(),
    );
    assert_eq!((n, again), (MANY, MANY));
}

#[test]
fn many_buses_fit_a_small_stack() {
    // Each Add Bus costs O(buses); 5 000 already gives a three-level tree.
    const BUSES: usize = 5_000;
    let (n, again) = many_then_reload(
        BUSES,
        |i| {
            Command::Mixer(MixerCmd::AddBus {
                name: format!("B{i}"),
                index: None,
            })
        },
        |d| d.mixer().strips().len(),
    );
    // Plus the master.
    assert_eq!((n, again), (BUSES + 1, BUSES + 1));
}

#[test]
fn many_outputs_fit_a_small_stack() {
    let (n, again) = many_then_reload(
        MANY,
        |i| {
            Command::Routing(RoutingCmd::AddOutput {
                name: format!("O{i}"),
                reference: format!("audio:{i}"),
            })
        },
        |d| d.outputs().len(),
    );
    assert_eq!((n, again), (MANY, MANY));
}

#[test]
fn many_media_items_fit_a_small_stack() {
    let (n, again) = many_then_reload(
        MANY,
        |i| {
            Command::Media(MediaCmd::Add {
                path: format!("Media/m{i}.caf"),
                sample_rate: 48_000,
                channels: 2,
                length_frames: 48_000,
                content_hash: None,
                name: None,
                origin: None,
            })
        },
        |d| d.media().len(),
    );
    assert_eq!((n, again), (MANY, MANY));
}

#[test]
fn many_templates_fit_a_small_stack() {
    let (n, again) = many_then_reload(
        MANY,
        |i| {
            Command::Settings(SettingsCmd::SetTemplate {
                name: format!("T{i}"),
                template: Some(ModifyTemplate {
                    rule: None,
                    mode: SelectMode::Select,
                    modify: None,
                    to_copy: false,
                }),
            })
        },
        |d| d.settings().templates.len(),
    );
    assert_eq!((n, again), (MANY, MANY));
}

/// The events of one take: one command inserts many, then single inserts,
/// a delete and undo run, and the take reloads, all on a small stack.
#[test]
fn many_events_in_one_take_fit_a_small_stack() {
    let (n, again) = many_then_reload(
        MANY,
        |i| match i {
            0 => new_seq(0),
            1 => Command::Event(EventCmd::Insert {
                seq: SeqId(0),
                lane: LaneId(0),
                events: (0..MANY as i64)
                    .map(|t| EventSpec::new(Tick(t * 10), EventBody::note(60, 100, Tick(5))))
                    .collect(),
            }),
            // Ids 0..MANY went to the bulk insert.
            2 => Command::Event(EventCmd::Delete {
                events: vec![EventId(7)],
            }),
            _ => Command::Event(EventCmd::Insert {
                seq: SeqId(0),
                lane: LaneId(0),
                events: vec![EventSpec::new(
                    Tick(i as i64 * 10 + 3),
                    EventBody::note(61, 100, Tick(5)),
                )],
            }),
        },
        |d| d.all_events().count(),
    );
    // MANY bulk, one deleted, MANY - 3 singles.
    assert_eq!(n, 2 * MANY - 4);
    assert_eq!(again, n);
}
