//! The surfaces' port transport (plan 5f-1 Ruling 10): the fake one in
//! test builds and offline, CoreMIDI on macOS live builds, none elsewhere.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// The most sends [`FakePorts`] remembers per port (an offline app runs
/// for hours).
const FAKE_SENT_LIMIT: usize = 4096;

/// How the surfaces reach their MIDI ports.
pub(crate) trait Ports: Send {
    /// Sends one message to destination `port`; false when it could not
    /// (the port is gone or its queue is full).
    fn send(&mut self, port: &str, bytes: &[u8]) -> bool;
    /// The source names present (as of the last [`Ports::refresh`] for
    /// CoreMIDI).
    fn sources(&self) -> Vec<String>;
    /// The destination names present (as of the last refresh for CoreMIDI).
    fn destinations(&self) -> Vec<String>;
    /// Looks at the system again: connects new sources, forgets gone ones,
    /// reopens outputs.
    fn refresh(&mut self);
    /// The port names the surfaces use now. [`FakePorts`] treats them as
    /// present (unless missing); real transports ignore it.
    fn want(&mut self, _names: &BTreeSet<String>) {}
    /// The fake itself (test hooks: test builds only ever use the fake).
    #[cfg(feature = "test-hooks")]
    fn as_fake(&mut self) -> &mut FakePorts;
}

/// The fake transport (Ruling 10): every port the surfaces use is present
/// unless [`FakePorts::set_missing`] says otherwise; sends are recorded.
/// With stale lists on, `sources()` and `destinations()` answer what the
/// last `refresh()` saw, as CoreMIDI's listener does.
#[derive(Debug, Default)]
pub(crate) struct FakePorts {
    /// The names the surfaces use.
    wanted: BTreeSet<String>,
    /// Names that are not present.
    missing: BTreeSet<String>,
    /// Answer the last refresh's lists.
    stale: bool,
    /// What the last refresh saw.
    seen: Vec<String>,
    /// What was sent, per port (the last [`FAKE_SENT_LIMIT`]).
    sent: BTreeMap<String, VecDeque<Vec<u8>>>,
}

// The setters and `take_sent` are test hooks; offline live builds use the
// fake without them.
#[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
impl FakePorts {
    /// Makes `ports` missing (replacing the earlier list).
    pub(crate) fn set_missing(&mut self, ports: Vec<String>) {
        self.missing = ports.into_iter().collect();
    }

    /// Turns stale lists on or off.
    pub(crate) fn set_stale(&mut self, on: bool) {
        self.stale = on;
        self.seen = self.present();
    }

    /// Drains what was sent to `port`.
    pub(crate) fn take_sent(&mut self, port: &str) -> Vec<Vec<u8>> {
        self.sent.remove(port).map(Vec::from).unwrap_or_default()
    }

    /// The names present now.
    fn present(&self) -> Vec<String> {
        self.wanted.difference(&self.missing).cloned().collect()
    }

    /// The list `sources` and `destinations` answer.
    fn list(&self) -> Vec<String> {
        if self.stale {
            self.seen.clone()
        } else {
            self.present()
        }
    }
}

impl Ports for FakePorts {
    fn send(&mut self, port: &str, bytes: &[u8]) -> bool {
        if !self.list().iter().any(|p| p == port) {
            return false;
        }
        let q = self.sent.entry(port.to_string()).or_default();
        if q.len() >= FAKE_SENT_LIMIT {
            q.pop_front();
        }
        q.push_back(bytes.to_vec());
        true
    }

    fn sources(&self) -> Vec<String> {
        self.list()
    }

    fn destinations(&self) -> Vec<String> {
        self.list()
    }

    fn refresh(&mut self) {
        self.seen = self.present();
    }

    fn want(&mut self, names: &BTreeSet<String>) {
        self.wanted = names.clone();
        if !self.stale {
            self.seen = self.present();
        }
    }

    #[cfg(feature = "test-hooks")]
    fn as_fake(&mut self) -> &mut FakePorts {
        self
    }
}

/// No MIDI on this platform: no sources, no destinations, so every
/// surface reads Disconnected.
#[cfg(all(not(target_os = "macos"), not(feature = "test-hooks")))]
#[derive(Debug, Default)]
pub(crate) struct NoPorts;

#[cfg(all(not(target_os = "macos"), not(feature = "test-hooks")))]
impl Ports for NoPorts {
    fn send(&mut self, _port: &str, _bytes: &[u8]) -> bool {
        false
    }

    fn sources(&self) -> Vec<String> {
        Vec::new()
    }

    fn destinations(&self) -> Vec<String> {
        Vec::new()
    }

    fn refresh(&mut self) {}
}

#[cfg(all(target_os = "macos", not(feature = "test-hooks")))]
pub(crate) use live::CoreMidiPorts;

#[cfg(all(target_os = "macos", not(feature = "test-hooks")))]
mod live {
    use super::Ports;
    use core_engine::backend::{MidiBackend, MidiOut, MidiPortInfo};
    use platform_macos::midi::{MidiSystem, SourceListener, list_destinations};
    use std::collections::BTreeMap;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::{Arc, RwLock};

    /// The client name of the surfaces' input and output.
    const CLIENT: &str = "VizDAW Surfaces";

    /// CoreMIDI (Ruling 10): a [`SourceListener`] for input, whose sink
    /// queues each message under its source's name, and a [`MidiSystem`]
    /// whose destinations are opened on first send. Either half that could
    /// not be created is retried at the next refresh.
    pub(crate) struct CoreMidiPorts {
        listener: Option<SourceListener>,
        system: Option<MidiSystem>,
        /// Source key → name, for the sink (`try_read`: it never waits).
        names: Arc<RwLock<BTreeMap<u32, String>>>,
        /// The destinations the last refresh saw.
        dests: Vec<MidiPortInfo>,
        /// The outputs opened, by name, with the destination they opened.
        outs: BTreeMap<String, (MidiPortInfo, Box<dyn MidiOut>)>,
    }

    impl CoreMidiPorts {
        /// Creates the client and connects every source present now.
        pub(crate) fn new() -> CoreMidiPorts {
            let mut p = CoreMidiPorts {
                listener: None,
                system: None,
                names: Arc::new(RwLock::new(BTreeMap::new())),
                dests: Vec::new(),
                outs: BTreeMap::new(),
            };
            p.refresh();
            p
        }

        /// The sink: CoreMIDI's thread; looks up and pushes, never waits,
        /// never unwinds into CoreMIDI.
        fn sink(names: Arc<RwLock<BTreeMap<u32, String>>>) -> platform_macos::midi::SourceSink {
            Box::new(move |key, host_ns, bytes| {
                let _ = catch_unwind(AssertUnwindSafe(|| {
                    if let Ok(n) = names.try_read()
                        && let Some(name) = n.get(&key)
                    {
                        super::super::input(name, bytes, host_ns);
                    }
                }));
            })
        }
    }

    impl Ports for CoreMidiPorts {
        fn send(&mut self, port: &str, bytes: &[u8]) -> bool {
            if !self.outs.contains_key(port) {
                let Some(info) = self.dests.iter().find(|d| d.name == port).cloned() else {
                    return false;
                };
                let Some(system) = self.system.as_mut() else {
                    return false;
                };
                let Ok(out) = system.open_destination(&info) else {
                    return false;
                };
                self.outs.insert(port.to_string(), (info, out));
            }
            self.outs
                .get_mut(port)
                .is_some_and(|(_, out)| out.send(0, bytes))
        }

        fn sources(&self) -> Vec<String> {
            self.listener
                .as_ref()
                .map(|l| l.sources().to_vec())
                .unwrap_or_default()
        }

        fn destinations(&self) -> Vec<String> {
            self.dests.iter().map(|d| d.name.clone()).collect()
        }

        fn refresh(&mut self) {
            match self.listener.as_mut() {
                Some(l) => {
                    l.reconnect();
                }
                None => {
                    self.listener =
                        SourceListener::connect_all(CLIENT, Self::sink(Arc::clone(&self.names)))
                            .ok();
                }
            }
            let names: BTreeMap<u32, String> = self
                .sources()
                .into_iter()
                .map(|n| (core_engine::input::port_key(&n), n))
                .collect();
            if let Ok(mut w) = self.names.write() {
                *w = names;
            }
            if self.system.is_none() {
                self.system = MidiSystem::new(CLIENT).ok();
            }
            self.dests = list_destinations();
            // Reopen what vanished or changed: an output is kept only while
            // the same destination is present.
            let dests = &self.dests;
            self.outs.retain(|_, (info, _)| dests.contains(info));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> BTreeSet<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_fake_has_what_is_wanted_but_missing_and_records_sends() {
        let mut p = FakePorts::default();
        p.want(&names(&["A", "B"]));
        assert_eq!(p.sources(), vec!["A".to_string(), "B".to_string()]);
        p.set_missing(vec!["B".into()]);
        assert_eq!(p.destinations(), vec!["A".to_string()]);
        assert!(p.send("A", &[1, 2]));
        assert!(!p.send("B", &[3]), "a missing port refuses");
        assert_eq!(p.take_sent("A"), vec![vec![1, 2]]);
        assert!(p.take_sent("A").is_empty(), "drained");
        p.set_stale(true);
        p.set_missing(Vec::new());
        assert_eq!(p.sources(), vec!["A".to_string()], "stale until a refresh");
        p.refresh();
        assert_eq!(p.sources().len(), 2);
        for _ in 0..FAKE_SENT_LIMIT + 5 {
            p.send("A", &[0]);
        }
        assert_eq!(p.take_sent("A").len(), FAKE_SENT_LIMIT, "bounded");
    }
}
