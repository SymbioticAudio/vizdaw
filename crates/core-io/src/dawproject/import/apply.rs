//! Applying a mapped import to a new project.

use super::timeline::{AutoAt, Body, CROSSFADES_PLAIN, MediaPlan, SlotAt, StripAt, UNPLACED};
use super::*;
use core_model::automation::{AutomationTarget, SendIndex, target_exists};
use core_model::command::{EventCmd, MediaCmd};
use core_model::document::{Document, MediaOrigin};
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::ids::MediaId;
use core_model::sequence::SeqLength;
use std::collections::BTreeSet;

/// Locks `seq`'s length when its derived length would leave an event out.
///
/// A derived length is the barline at or after the content's end, and a
/// point (a controller, automation or other one-tick event) ends at its own
/// tick. So a file whose content ends in a point on a barline would import
/// with that point at the sequence's end, outside its window, where the
/// file plays it. The length is locked to the barline after the last such
/// event (or one tick past it near the end of the timeline); a point that
/// no length can cover is left out with its line.
fn cover_last_event(h: &mut History, seq: SeqId, report: &mut Report) -> Result<(), ModelError> {
    let doc = h.doc();
    let s = doc.sequence(seq)?;
    if matches!(s.length, SeqLength::Locked(_)) {
        return Ok(());
    }
    let derived = core_model::query::seq_length(doc, seq)?;
    let last = s
        .lanes
        .iter()
        .filter(|l| l.length.is_none())
        .filter_map(|l| l.active())
        .flat_map(|t| t.events.values())
        .map(|e| e.tick)
        .max();
    let Some(last) = last.filter(|t| *t >= derived) else {
        return Ok(());
    };
    let after = Tick(last.0.saturating_add(1));
    let bar = s.meter.barline_at_or_after(after);
    let length = if bar.0 <= MAX_TICKS { bar } else { after };
    if length.0 > MAX_TICKS {
        report.add(ItemKind::LeftOut, PAST_END, None);
        return Ok(());
    }
    run(
        h,
        Command::Sequence(SequenceCmd::SetLength {
            seq,
            length: SeqLength::Locked(length),
        }),
    )?;
    Ok(())
}

fn run(h: &mut History, cmd: Command) -> Result<CmdReport, ModelError> {
    h.execute(&cmd, &ViewFilter::default())
}

fn created<T>(r: &CmdReport, f: impl Fn(&AnyId) -> Option<T>) -> Option<T> {
    r.created.iter().find_map(f)
}

fn created_plugin(r: &CmdReport) -> Result<PluginInstanceId, ModelError> {
    created(r, |id| match id {
        AnyId::Plugin(p) => Some(*p),
        _ => None,
    })
    .ok_or_else(|| ModelError::invalid("no plug-in was created"))
}

/// The plug-in slots `apply_mix` made on a strip.
#[derive(Debug, Clone, Default)]
pub(super) struct MixSlots {
    pub(super) eq: Option<PluginInstanceId>,
    pub(super) inserts: Vec<PluginInstanceId>,
    /// Each planned send's index among the strip's sends (`None`: not
    /// made).
    pub(super) sends: Vec<Option<usize>>,
}

/// What the arrangement needs of the applied structure: each plan track's
/// track, destination, strip and slots (`None` for a track not made), the
/// buses' and the master's.
pub(super) struct Applied {
    pub(super) lanes: Vec<Option<LaneId>>,
    pub(super) dests: Vec<Option<DestId>>,
    /// The strip automation of the track drives, and its slots.
    pub(super) strips: Vec<Option<(StripId, MixSlots)>>,
    pub(super) instruments: Vec<Option<PluginInstanceId>>,
    /// By plan bus.
    pub(super) buses: BTreeMap<usize, (StripId, MixSlots)>,
    pub(super) master: (StripId, MixSlots),
}

impl Applied {
    /// Nothing placed yet, for `n` plan tracks.
    fn new(n: usize, master: StripId) -> Applied {
        Applied {
            lanes: vec![None; n],
            dests: vec![None; n],
            strips: vec![None; n],
            instruments: vec![None; n],
            buses: BTreeMap::new(),
            master: (master, MixSlots::default()),
        }
    }
}

/// The slots `strip` already has, as `apply_mix` would have made them (no
/// planned sends).
fn existing_slots(h: &History, strip: StripId) -> MixSlots {
    h.doc()
        .mixer()
        .strip(strip)
        .map(|s| MixSlots {
            eq: s.eq.as_ref().map(|e| e.id),
            inserts: s.inserts.iter().map(|i| i.id).collect(),
            sends: Vec::new(),
        })
        .unwrap_or_default()
}

/// What [`apply_tracks`] made: the new slots' states, the origins and the
/// placement the arrangement needs ([`arrange_tracks`]).
pub(crate) struct Added {
    /// Each new plug-in slot with a state, and its state.
    pub(crate) states: SlotStates,
    /// What each new object was where the file came from.
    pub(crate) origins: Origins,
    placed: Applied,
    lanes: u64,
    plugins: u64,
}

fn strip_of(h: &History, kind: StripKind) -> Result<StripId, ModelError> {
    h.doc()
        .mixer()
        .find_kind(kind)
        .ok_or_else(|| ModelError::invalid("the strip was not created"))
}

/// Applies `mix` to `strip`; with `flags`, its mute and solo too; on the
/// master, no output and no sends. New slots with a state join `states`.
fn apply_mix(
    h: &mut History,
    strip: StripId,
    mix: &Mix,
    buses: &BTreeMap<usize, StripId>,
    flags: bool,
    master: bool,
    states: &mut SlotStates,
) -> Result<MixSlots, ModelError> {
    let mut made = MixSlots::default();
    let mut sent = 0usize;
    let mixer = |cmd| Command::Mixer(cmd);
    if mix.gain_db != 0.0 {
        run(
            h,
            mixer(MixerCmd::SetGain {
                strip,
                gain_db: mix.gain_db,
            }),
        )?;
    }
    if mix.pan != 0.0 && !master {
        run(
            h,
            mixer(MixerCmd::SetPan {
                strip,
                pan: mix.pan,
            }),
        )?;
    }
    if flags && mix.mute {
        run(h, mixer(MixerCmd::SetMute { strip, mute: true }))?;
    }
    if flags && mix.solo && !master {
        run(h, mixer(MixerCmd::SetSolo { strip, solo: true }))?;
    }
    if mix.colour.is_some() {
        run(
            h,
            mixer(MixerCmd::SetColour {
                strip,
                colour: mix.colour,
            }),
        )?;
    }
    if !master {
        if let Out::Bus(b) = mix.output
            && let Some(bus) = buses.get(&b).copied()
        {
            run(
                h,
                mixer(MixerCmd::SetOutput {
                    strip,
                    output: Output::Bus(bus),
                }),
            )?;
        }
        for s in &mix.sends {
            let Some(target) = buses.get(&s.bus).copied() else {
                made.sends.push(None);
                continue;
            };
            made.sends.push(Some(sent));
            sent += 1;
            run(
                h,
                mixer(MixerCmd::AddSend {
                    strip,
                    send: AuxSend {
                        target,
                        level_db: s.level_db,
                        pre_fader: s.pre,
                        enabled: s.enabled,
                    },
                }),
            )?;
        }
    }
    let mut slot_done = |h: &mut History, id: PluginInstanceId, plan: &SlotPlan| {
        if plan.bypass {
            run(
                h,
                mixer(MixerCmd::SetBypass {
                    slot: id,
                    bypass: true,
                }),
            )?;
        }
        if let Some(s) = &plan.state {
            states.push((id, Arc::clone(s)));
        }
        Ok::<(), ModelError>(())
    };
    if let Some(eq) = &mix.eq {
        let r = run(
            h,
            mixer(MixerCmd::SetEq {
                strip,
                plugin: Some(eq.plugin.clone()),
            }),
        )?;
        let id = created_plugin(&r)?;
        slot_done(h, id, eq)?;
        made.eq = Some(id);
    }
    for ins in &mix.inserts {
        let r = run(
            h,
            mixer(MixerCmd::AddInsert {
                strip,
                index: None,
                plugin: ins.plugin.clone(),
            }),
        )?;
        let id = created_plugin(&r)?;
        slot_done(h, id, ins)?;
        made.inserts.push(id);
    }
    Ok(made)
}

/// Records the effects of `mix` that VizDAW cannot host, against `strip`.
fn unhosted(strip: StripId, mix: &Mix, o: &mut Origins) {
    if !mix.unhosted.is_empty() {
        o.unhosted.insert(strip, mix.unhosted.clone());
    }
}

/// Records the origins of the slots `apply_mix` made from `mix`.
fn slot_origins(mix: &Mix, made: &MixSlots, o: &mut Origins) {
    if let (Some(id), Some(tag)) = (made.eq, mix.eq.as_ref().and_then(|s| s.tag)) {
        o.slots.insert(id, tag);
    }
    for (id, plan) in made.inserts.iter().zip(&mix.inserts) {
        if let Some(tag) = plan.tag {
            o.slots.insert(*id, tag);
        }
    }
}

/// The plug-in slots `mix` makes.
fn slots(mix: &Mix) -> u64 {
    u64::try_from(mix.inserts.len())
        .unwrap_or(u64::MAX)
        .saturating_add(u64::from(mix.eq.is_some()))
}

/// Inside the caller's open transaction on a document with no tracks;
/// returns each new plugin slot's state to capture after the commit.
///
/// Makes, in order: the buses, the shared built-in-synth destination when a
/// track needs it, each track (its destination or plug-in instrument, its
/// output path, its strips' settings and, on the shared synth, its mute
/// and solo), the synth's, the buses' and the master's settings, then the
/// tempo and meter maps, the markers, the sequence's name and the song
/// info, then the arrangement ([`apply_arranged`]). Merges the
/// import's report, without [`TAGS_IGNORED`](super::TAGS_IGNORED) (a
/// merge's line), into `report` and fills its summary.
pub fn apply(
    h: &mut History,
    imp: &DpImport,
    seq: SeqId,
    report: &mut Report,
) -> Result<SlotStates, ModelError> {
    let states = apply_tagged(h, imp, seq, report).map(|(states, _)| states)?;
    // Tags matter only to a merge (Task 2 review Minor 5): a plain import
    // does not list the ones it ignored.
    report.items.retain(|i| i.what != super::TAGS_IGNORED);
    Ok(states)
}

/// [`apply`], with what each new object was in the project the file came
/// from, as its trusted XML IDs, else its tags, named it (plan 6e Task 2).
/// A track's first own strip and that strip's slots take its channel's and
/// devices' origins; a strip made twice from one channel (a track with
/// notes and audio) takes them once.
pub fn apply_tagged(
    h: &mut History,
    imp: &DpImport,
    seq: SeqId,
    report: &mut Report,
) -> Result<(SlotStates, Origins), ModelError> {
    let doc = h.doc();
    doc.sequence(seq)?;
    if doc.sequences().values().any(|s| !s.lanes.is_empty()) {
        return Err(ModelError::invalid(NOT_NEW));
    }
    report.merge(imp.report.clone());
    let plan = &imp.plan;
    let mut buses = BTreeMap::new();
    add_buses(h, imp, 0..plan.buses.len(), &mut buses)?;
    let all: Vec<usize> = (0..plan.tracks.len()).collect();
    // Every bus is made: `apply_tracks` adds none and sets no bus's mix.
    let Added {
        mut states,
        mut origins,
        mut placed,
        lanes,
        mut plugins,
    } = apply_tracks(h, imp, seq, &all, &mut buses, &BTreeSet::new())?;
    for (b, bp) in plan.buses.iter().enumerate() {
        let Some(id) = buses.get(&b).copied() else {
            continue;
        };
        let made = apply_mix(h, id, &bp.mix, &buses, true, false, &mut states)?;
        if let Some(tag) = bp.tag {
            origins.strips.insert(id, tag);
        }
        unhosted(id, &bp.mix, &mut origins);
        slot_origins(&bp.mix, &made, &mut origins);
        placed.buses.insert(b, (id, made));
        plugins = plugins.saturating_add(slots(&bp.mix));
    }
    let master = h.doc().mixer().master();
    let made = apply_mix(h, master, &plan.master, &buses, true, true, &mut states)?;
    unhosted(master, &plan.master, &mut origins);
    slot_origins(&plan.master, &made, &mut origins);
    placed.master = (master, made);
    plugins = plugins.saturating_add(slots(&plan.master));

    run(
        h,
        Command::Sequence(SequenceCmd::SetTempo {
            seq,
            tempo: plan.tempo.clone(),
        }),
    )?;
    run(
        h,
        Command::Sequence(SequenceCmd::SetMeter {
            seq,
            meter: plan.meter.clone(),
        }),
    )?;
    if !plan.markers.is_empty() {
        run(
            h,
            Command::Sequence(SequenceCmd::SetMarkers {
                seq,
                markers: plan.markers.iter().cloned().collect(),
            }),
        )?;
    }
    run(
        h,
        Command::Sequence(SequenceCmd::Rename {
            seq,
            name: plan.name.clone(),
        }),
    )?;
    if !plan.info.is_empty() {
        run(
            h,
            Command::Settings(SettingsCmd::SetInfo {
                info: plan.info.clone(),
            }),
        )?;
    }
    origins.markers = plan.marker_tags.clone();
    apply_arranged(h, plan, seq, &placed, report, &mut origins, &Arrange::all())?;
    cover_last_event(h, seq, report)?;
    let s = &mut report.summary;
    s.tracks = s.tracks.saturating_add(lanes);
    s.markers = s
        .markers
        .saturating_add(u64::try_from(plan.markers.len()).unwrap_or(u64::MAX));
    s.plugins = s.plugins.saturating_add(plugins);
    s.tempo_map = true;
    Ok((states, origins))
}

/// Adds plan buses `which` (in order) that `buses` does not hold yet, with
/// no settings, into `buses`.
fn add_buses(
    h: &mut History,
    imp: &DpImport,
    which: impl IntoIterator<Item = usize>,
    buses: &mut BTreeMap<usize, StripId>,
) -> Result<Vec<usize>, ModelError> {
    let mut made = Vec::new();
    for b in which {
        let Some(bp) = imp.plan.buses.get(b) else {
            continue;
        };
        if buses.contains_key(&b) {
            continue;
        }
        let r = run(
            h,
            Command::Mixer(MixerCmd::AddBus {
                name: bp.name.clone(),
                index: None,
            }),
        )?;
        let id = created(&r, |id| match id {
            AnyId::Strip(s) => Some(*s),
            _ => None,
        })
        .ok_or_else(|| ModelError::invalid("no bus was created"))?;
        buses.insert(b, id);
        made.push(b);
    }
    Ok(made)
}

/// The plan buses a strip with `mix` plays into or sends to, directly or
/// through other buses.
fn buses_needed<'m>(imp: &DpImport, mixes: impl IntoIterator<Item = &'m Mix>) -> BTreeSet<usize> {
    fn direct(mix: &Mix) -> impl Iterator<Item = usize> + '_ {
        let out = match mix.output {
            Out::Bus(b) => Some(b),
            Out::Master => None,
        };
        out.into_iter().chain(mix.sends.iter().map(|s| s.bus))
    }
    let mut seen = BTreeSet::new();
    let mut todo: Vec<usize> = mixes.into_iter().flat_map(direct).collect();
    while let Some(b) = todo.pop() {
        if b < imp.plan.buses.len() && seen.insert(b) {
            if let Some(bp) = imp.plan.buses.get(b) {
                todo.extend(direct(&bp.mix));
            }
        }
    }
    seen
}

/// Adds the buses `imp` describes and then sets their settings, for the
/// plan buses `which` that `buses` does not hold (a merge's new buses,
/// plan 6e Ruling 9). New slots with a state join `states`.
pub(crate) fn add_buses_with_mix(
    h: &mut History,
    imp: &DpImport,
    which: &[usize],
    buses: &mut BTreeMap<usize, StripId>,
    states: &mut SlotStates,
) -> Result<(), ModelError> {
    let made = add_buses(h, imp, which.iter().copied(), buses)?;
    for b in made {
        let (Some(bp), Some(id)) = (imp.plan.buses.get(b), buses.get(&b).copied()) else {
            continue;
        };
        apply_mix(h, id, &bp.mix, buses, true, false, states)?;
    }
    Ok(())
}

/// The plan bus each of `imp`'s buses became in the project `apply` made
/// from it: its buses in the order they were made.
pub(crate) fn bus_of_strip(doc: &Document) -> BTreeMap<StripId, usize> {
    let mut ids: Vec<StripId> = doc
        .mixer()
        .strips()
        .values()
        .filter(|s| s.kind == StripKind::Bus)
        .map(|s| s.id)
        .collect();
    ids.sort_by_key(|s| s.0);
    ids.into_iter().enumerate().map(|(i, s)| (s, i)).collect()
}

/// Inside the caller's open transaction: adds plan tracks `which` (with
/// their destinations or plug-in instruments, output paths, strips'
/// settings and devices) and the buses they need to a project that may
/// already have tracks (plan 6e Ruling 9); [`apply`] calls it for all.
///
/// `buses` maps plan buses to the project's strips; a bus a track needs
/// that it does not hold is added (with its settings) and joins it,
/// unless it is `absent` (a bus the project deleted, plan 6e Task 6 fix
/// round 1): a track then plays into the master and sends nothing to it.
/// The structure adds no report line: the mapping's lines are the
/// caller's to merge, and the events' are [`arrange_tracks`]'. A
/// track on the built-in synth uses the project's built-in synth when it
/// has one; only a synth made here gets the plan's synth settings. The
/// tracks' events are [`arrange_tracks`]'.
pub(crate) fn apply_tracks(
    h: &mut History,
    imp: &DpImport,
    seq: SeqId,
    which: &[usize],
    buses: &mut BTreeMap<usize, StripId>,
    absent: &BTreeSet<usize>,
) -> Result<Added, ModelError> {
    let plan = &imp.plan;
    h.doc().sequence(seq)?;
    let chosen: Vec<(usize, &TrackPlan)> = which
        .iter()
        .filter_map(|&i| plan.tracks.get(i).map(|t| (i, t)))
        .collect();
    let mut states = Vec::new();
    let mut origins = Origins::default();
    let mut plugins = 0u64;
    let mut needed = buses_needed(
        imp,
        chosen.iter().map(|(_, t)| &t.mix).chain(
            chosen
                .iter()
                .any(|(_, t)| t.midi == Some(Midi::Builtin))
                .then_some(plan.builtin.as_ref())
                .flatten(),
        ),
    );
    needed.retain(|b| !absent.contains(b));
    let new_buses = add_buses(h, imp, needed, buses)?;

    let mut builtin: Option<DestId> = None;
    let mut builtin_new = false;
    if chosen.iter().any(|(_, t)| t.midi == Some(Midi::Builtin)) {
        builtin = h
            .doc()
            .instruments()
            .values()
            .find(|d| d.kind.is_builtin_synth())
            .map(|d| d.id);
        if builtin.is_none() {
            let r = run(
                h,
                Command::Routing(RoutingCmd::AddDestination {
                    name: BUILTIN_NAME.to_owned(),
                    kind: DestKind::MidiEndpoint {
                        endpoint_ref: BUILTIN_SYNTH_REF.to_owned(),
                        group: 0,
                        channel: 0,
                    },
                    default_patch: None,
                }),
            )?;
            builtin = created(&r, |id| match id {
                AnyId::Dest(d) => Some(*d),
                _ => None,
            });
            builtin_new = builtin.is_some();
        }
    }

    let mut lanes = 0u64;
    let master = h.doc().mixer().master();
    let mut placed = Applied::new(plan.tracks.len(), master);
    placed.master.1 = existing_slots(h, master);
    for (&b, &id) in buses.iter() {
        placed.buses.insert(b, (id, existing_slots(h, id)));
    }
    // Tracks on the built-in synth, whose strip comes later.
    let mut on_builtin: Vec<usize> = Vec::new();
    for &(ti, t) in &chosen {
        let r = run(
            h,
            Command::Lane(LaneCmd::Add {
                seq,
                name: t.name.clone(),
                index: None,
            }),
        )?;
        let lane = created(&r, |id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        })
        .ok_or_else(|| ModelError::invalid("no track was created"))?;
        lanes = lanes.saturating_add(1);
        if let Some(slot) = placed.lanes.get_mut(ti) {
            *slot = Some(lane);
        }
        if let Some(tag) = t.tag {
            origins.lanes.insert(lane, tag);
        }
        let mut owned: Vec<StripId> = Vec::new();
        let mut dest_of = None;
        let mut instrument = None;
        match &t.midi {
            Some(Midi::Plugin(slot)) => {
                let mut s = PluginSlot::new(PluginInstanceId(0), slot.plugin.clone());
                s.bypass = slot.bypass;
                let r = run(
                    h,
                    Command::Routing(RoutingCmd::AddDestination {
                        name: t.dest_name.clone(),
                        kind: DestKind::PluginInstrument { slot: s },
                        default_patch: None,
                    }),
                )?;
                let dest = created(&r, |id| match id {
                    AnyId::Dest(d) => Some(*d),
                    _ => None,
                })
                .ok_or_else(|| ModelError::invalid("no destination was created"))?;
                let id = created_plugin(&r)?;
                instrument = Some(id);
                dest_of = Some(dest);
                if let Some(tag) = slot.tag {
                    origins.slots.insert(id, tag);
                }
                if let Some(st) = &slot.state {
                    states.push((id, Arc::clone(st)));
                }
                plugins = plugins.saturating_add(1);
                run(
                    h,
                    Command::Lane(LaneCmd::SetDest {
                        seq,
                        lane,
                        dest: Some(dest),
                    }),
                )?;
                owned.push(strip_of(h, StripKind::Instrument(dest))?);
            }
            Some(Midi::Builtin) => {
                dest_of = builtin;
                run(
                    h,
                    Command::Lane(LaneCmd::SetDest {
                        seq,
                        lane,
                        dest: builtin,
                    }),
                )?;
            }
            None => {}
        }
        if t.audio {
            let r = run(
                h,
                Command::Routing(RoutingCmd::AddOutput {
                    name: t.name.clone(),
                    reference: AUDIO_REFERENCE.to_owned(),
                }),
            )?;
            let out = created(&r, |id| match id {
                AnyId::Output(o) => Some(*o),
                _ => None,
            })
            .ok_or_else(|| ModelError::invalid("no output path was created"))?;
            run(
                h,
                Command::Lane(LaneCmd::SetOutput {
                    seq,
                    lane,
                    output: Some(out),
                }),
            )?;
            let path = strip_of(h, StripKind::AudioPath(out))?;
            if let Some(n) = &t.channel_name {
                origins.channel_names.insert(path, n.clone());
            }
            owned.push(path);
        }
        let mut first: Option<(StripId, MixSlots)> = None;
        for strip in owned {
            let made = apply_mix(
                h,
                strip,
                &t.mix,
                buses,
                t.flags.is_none(),
                false,
                &mut states,
            )?;
            unhosted(strip, &t.mix, &mut origins);
            if first.is_none() {
                if let Some(tag) = t.strip_tag {
                    origins.strips.insert(strip, tag);
                }
                slot_origins(&t.mix, &made, &mut origins);
            }
            first.get_or_insert((strip, made));
            plugins = plugins.saturating_add(slots(&t.mix));
        }
        if first.is_none() && t.midi == Some(Midi::Builtin) {
            on_builtin.push(ti);
        }
        if let Some(s) = placed.strips.get_mut(ti) {
            *s = first;
        }
        if let Some(d) = placed.dests.get_mut(ti) {
            *d = dest_of;
        }
        if let Some(i) = placed.instruments.get_mut(ti) {
            *i = instrument;
        }
        if let Some((mute, solo)) = t.flags
            && (mute || solo)
        {
            run(
                h,
                Command::Lane(LaneCmd::SetFlags {
                    seq,
                    lane,
                    mute: mute.then_some(true),
                    solo: solo.then_some(true),
                    arm: None,
                }),
            )?;
        }
    }
    if builtin.is_some() {
        let strip = strip_of(h, StripKind::BuiltinSynth)?;
        let made = match (&plan.builtin, builtin_new) {
            (Some(mix), true) => {
                let made = apply_mix(h, strip, mix, buses, false, false, &mut states)?;
                unhosted(strip, mix, &mut origins);
                slot_origins(mix, &made, &mut origins);
                plugins = plugins.saturating_add(slots(mix));
                made
            }
            _ => existing_slots(h, strip),
        };
        for ti in on_builtin {
            if let Some(s) = placed.strips.get_mut(ti) {
                *s = Some((strip, made.clone()));
            }
        }
    }
    for b in new_buses {
        let (Some(bp), Some(id)) = (plan.buses.get(b), buses.get(&b).copied()) else {
            continue;
        };
        let made = apply_mix(h, id, &bp.mix, buses, true, false, &mut states)?;
        placed.buses.insert(b, (id, made));
    }
    Ok(Added {
        states,
        origins,
        placed,
        lanes,
        plugins,
    })
}

/// Inside the caller's open transaction, after [`apply_tracks`] made
/// `added`'s tracks: their events, `shift` ticks later than the file
/// places them (a merge's pickup, plan 6e Ruling 1), with the media and
/// sequences they use. A media item reuses one of the project's with the
/// same content hash (Ruling 9). Answers each crossfaded region placed,
/// with its spec, for the merge to set again once the maps are in (the
/// model may have settled it under the project's tempo map).
pub(crate) fn arrange_tracks(
    h: &mut History,
    imp: &DpImport,
    seq: SeqId,
    which: &[usize],
    added: &Added,
    shift: i64,
    report: &mut Report,
) -> Result<Vec<(EventId, EventSpec)>, ModelError> {
    let mut origins = Origins::default();
    let arrange = Arrange {
        which: Some(which.iter().copied().collect()),
        shift,
        flagged: Some(std::cell::RefCell::new(Vec::new())),
    };
    apply_arranged(
        h,
        &imp.plan,
        seq,
        &added.placed,
        report,
        &mut origins,
        &arrange,
    )?;
    Ok(arrange.flagged.map(|f| f.into_inner()).unwrap_or_default())
}

// ---------------------------------------------------------------- the arrangement

/// The model target of `at` on the applied structure; `None` when that
/// part was not made.
fn target_of(h: &History, at: AutoAt, placed: &Applied) -> Option<AutomationTarget> {
    let strip = |s: StripAt| -> Option<(StripId, &MixSlots)> {
        match s {
            StripAt::Track(i) => placed
                .strips
                .get(i)
                .and_then(Option::as_ref)
                .map(|(id, m)| (*id, m)),
            StripAt::Bus(b) => placed.buses.get(&b).map(|(id, m)| (*id, m)),
            StripAt::Master => Some((placed.master.0, &placed.master.1)),
        }
    };
    let target = match at {
        AutoAt::Fader(s) => AutomationTarget::StripFader(strip(s)?.0),
        AutoAt::Pan(s) => AutomationTarget::StripPan(strip(s)?.0),
        AutoAt::Mute(s) => AutomationTarget::StripMute(strip(s)?.0),
        AutoAt::Send(s, i) => {
            let (id, made) = strip(s)?;
            let k = made.sends.get(i).copied().flatten()?;
            AutomationTarget::SendLevel(id, SendIndex(u16::try_from(k).ok()?))
        }
        AutoAt::Param(s, slot, q) => {
            let id = match (slot, s) {
                (SlotAt::Instrument, StripAt::Track(i)) => {
                    placed.instruments.get(i).copied().flatten()?
                }
                (SlotAt::Instrument, _) => return None,
                (SlotAt::Eq, s) => strip(s)?.1.eq?,
                (SlotAt::Insert(k), s) => strip(s)?.1.inserts.get(k).copied()?,
            };
            AutomationTarget::PluginParam(id, q)
        }
    };
    target_exists(h.doc(), target).then_some(target)
}

fn created_id<T>(r: &CmdReport, f: impl Fn(&AnyId) -> Option<T>) -> Result<T, ModelError> {
    r.created
        .iter()
        .find_map(f)
        .ok_or_else(|| ModelError::invalid("the part was not created"))
}

/// Which tracks' events [`apply_arranged`] applies, and where.
struct Arrange {
    /// The plan tracks (`None`: all, with every media item and sequence
    /// made in order, as an import makes them).
    which: Option<BTreeSet<usize>>,
    /// Ticks added to each track item's tick.
    shift: i64,
    /// A merge's: each crossfaded region placed, with its spec, for the
    /// merge to set again once every map is in (plan 6e Task 7 fix round
    /// 3). The model may settle such a crossfade under the merged tempo
    /// map; the merge lists that from its own settle (`decide::held`), so
    /// none is listed here and none is a surprise.
    flagged: Option<std::cell::RefCell<Vec<(EventId, EventSpec)>>>,
}

impl Arrange {
    fn all() -> Arrange {
        Arrange {
            which: None,
            shift: 0,
            flagged: None,
        }
    }

    fn has(&self, track: usize) -> bool {
        self.which.as_ref().is_none_or(|w| w.contains(&track))
    }
}

/// The media item for plan media `m`: with `reuse`, the project's item of
/// the same content hash when it has one; else a new one.
fn media_item(h: &mut History, m: &MediaPlan, reuse: bool) -> Result<MediaId, ModelError> {
    if reuse
        && let Some(hash) = &m.hash
        && let Some(found) = h
            .doc()
            .media()
            .values()
            .find(|x| x.content_hash.as_ref() == Some(hash))
    {
        return Ok(found.id);
    }
    let r = h.execute(
        &Command::Media(MediaCmd::Add {
            path: m.path.clone(),
            sample_rate: m.rate,
            channels: m.channels,
            length_frames: m.frames,
            content_hash: m.hash.clone(),
            name: Some(m.name.clone()),
            origin: Some(MediaOrigin::Imported {
                original_name: m.original.clone(),
            }),
        }),
        &ViewFilter::default(),
    )?;
    created_id(&r, |id| match id {
        AnyId::Media(m) => Some(*m),
        _ => None,
    })
}

/// Applies the arrangement once the structure and the maps are in place
/// (so regions and crossfades see the tempo map): the media items, the new
/// sequences, then each track's events in one insert (each new event's
/// origin from its item's tag). With `arrange.which`, only those tracks',
/// with only the media and sequences they use.
fn apply_arranged(
    h: &mut History,
    plan: &Plan,
    seq: SeqId,
    placed: &Applied,
    report: &mut Report,
    origins: &mut Origins,
    arrange: &Arrange,
) -> Result<(), ModelError> {
    let arr = &plan.arranged;
    let run = |h: &mut History, cmd: Command| h.execute(&cmd, &ViewFilter::default());
    let (mut notes, mut regions, mut points) = (0u64, 0u64, 0u64);
    let subset = arrange.which.is_some();
    // What the chosen tracks use.
    let mut used_media = BTreeSet::new();
    let mut used_seqs = BTreeSet::new();
    for (ti, items) in arr.items.iter().enumerate() {
        if !arrange.has(ti) {
            continue;
        }
        for it in items {
            match &it.body {
                Body::Region(r) => {
                    used_media.insert(r.media);
                }
                Body::Use { seq: k, .. } => {
                    used_seqs.insert(*k);
                }
                _ => {}
            }
        }
    }
    let mut media: Vec<Option<MediaId>> = Vec::with_capacity(arr.media.len());
    let mut made_media = 0u64;
    for (k, m) in arr.media.iter().enumerate() {
        if subset && !used_media.contains(&k) {
            media.push(None);
            continue;
        }
        media.push(Some(media_item(h, m, subset)?));
        made_media = made_media.saturating_add(1);
    }
    let mut seqs: Vec<Option<SeqId>> = Vec::with_capacity(arr.seqs.len());
    for (k, s) in arr.seqs.iter().enumerate() {
        if subset && !used_seqs.contains(&k) {
            seqs.push(None);
            continue;
        }
        let r = run(
            h,
            Command::Sequence(SequenceCmd::New {
                name: s.name.clone(),
            }),
        )?;
        let id = created_id(&r, |id| match id {
            AnyId::Seq(s) => Some(*s),
            _ => None,
        })?;
        let lane = created_id(&r, |id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        })?;
        if let Some(t) = plan.tracks.get(s.track) {
            run(
                h,
                Command::Lane(LaneCmd::Rename {
                    seq: id,
                    lane,
                    name: t.name.clone(),
                }),
            )?;
        }
        if let Some(dest) = placed.dests.get(s.track).copied().flatten() {
            run(
                h,
                Command::Lane(LaneCmd::SetDest {
                    seq: id,
                    lane,
                    dest: Some(dest),
                }),
            )?;
        }
        for cmd in [
            SequenceCmd::SetTempo {
                seq: id,
                tempo: plan.tempo.clone(),
            },
            SequenceCmd::SetMeter {
                seq: id,
                meter: plan.meter.clone(),
            },
            SequenceCmd::SetLength {
                seq: id,
                length: SeqLength::Locked(s.length),
            },
        ] {
            run(h, Command::Sequence(cmd))?;
        }
        if !s.events.is_empty() {
            notes = notes.saturating_add(count_notes(s.events.iter().map(|e| &e.body)));
            let r = run(
                h,
                Command::Event(EventCmd::Insert {
                    seq: id,
                    lane,
                    events: s.events.clone(),
                }),
            )?;
            settled(&r, &s.events, arrange, report)?;
        }
        seqs.push(Some(id));
    }
    for (ti, items) in arr.items.iter().enumerate() {
        if !arrange.has(ti) {
            continue;
        }
        let Some(lane) = placed.lanes.get(ti).copied().flatten() else {
            continue;
        };
        if items.is_empty() {
            continue;
        }
        let mut specs = Vec::with_capacity(items.len());
        let mut tags: Vec<Option<u64>> = Vec::with_capacity(items.len());
        for it in items {
            let (body, group_channel) = match &it.body {
                Body::Midi(b, gc) => (b.clone(), *gc),
                Body::Use {
                    seq: k,
                    length,
                    loop_count,
                } => {
                    let Some(target) = seqs.get(*k).copied().flatten() else {
                        continue;
                    };
                    (
                        EventBody::Reference {
                            seq: target,
                            length: *length,
                            loop_count: *loop_count,
                            transpose: 0,
                        },
                        None,
                    )
                }
                Body::Region(r) => {
                    let Some(m) = media.get(r.media).copied().flatten() else {
                        continue;
                    };
                    regions = regions.saturating_add(1);
                    let fade = |n: u64| Fade {
                        length_frames: n,
                        shape: FadeShape::Linear,
                    };
                    (
                        EventBody::AudioRegion {
                            media: m,
                            source_offset: r.source_offset,
                            length: r.length,
                            gain_db: 0.0,
                            fade_in: fade(r.fade_in),
                            fade_out: fade(r.fade_out),
                            name: r.name.clone(),
                            crossfade: r.crossfade,
                        },
                        None,
                    )
                }
                Body::Auto { at, value } => {
                    let Some(target) = target_of(h, *at, placed) else {
                        report.add(ItemKind::LeftOut, UNPLACED, None);
                        continue;
                    };
                    points = points.saturating_add(1);
                    (
                        EventBody::Automation {
                            target,
                            value: *value,
                        },
                        None,
                    )
                }
            };
            specs.push(EventSpec {
                tick: Tick(it.tick.0.saturating_add(arrange.shift)),
                dest: None,
                group_channel,
                mute: it.mute,
                body,
            });
            tags.push(it.tag);
        }
        notes = notes.saturating_add(count_notes(specs.iter().map(|e| &e.body)));
        let r = run(
            h,
            Command::Event(EventCmd::Insert {
                seq,
                lane,
                events: specs.clone(),
            }),
        )?;
        settled(&r, &specs, arrange, report)?;
        let made = r.created.iter().filter_map(|id| match id {
            AnyId::Event(e) => Some(*e),
            _ => None,
        });
        for (e, tag) in made.zip(tags) {
            if let Some(tag) = tag {
                origins.events.insert(e, tag);
            }
        }
    }
    let s = &mut report.summary;
    s.notes = s.notes.saturating_add(notes);
    s.regions = s.regions.saturating_add(regions);
    s.automation_points = s.automation_points.saturating_add(points);
    s.audio_files = s.audio_files.saturating_add(made_media);
    Ok(())
}

/// Lists what the model's crossfade rules changed in an insert. The
/// timeline mapping places crossfades by the model's own rule and tempo
/// lookup, so in an import this should never happen; if it does, the
/// change is reported, never dropped. A merge's insert (`arrange.flagged`)
/// is placed under the project's tempo map, not the file's: its crossfaded
/// regions are kept, with their specs, for the merge to set again.
fn settled(
    r: &CmdReport,
    specs: &[EventSpec],
    arrange: &Arrange,
    report: &mut Report,
) -> Result<(), ModelError> {
    if let Some(flagged) = &arrange.flagged {
        let made: Vec<EventId> = r
            .created
            .iter()
            .filter_map(|id| match id {
                AnyId::Event(e) => Some(*e),
                _ => None,
            })
            .collect();
        if made.len() != specs.len() {
            return Err(ModelError::invalid(
                "the insert made a different number of events",
            ));
        }
        let mut f = flagged.borrow_mut();
        for (e, s) in made.into_iter().zip(specs) {
            if matches!(
                s.body,
                EventBody::AudioRegion {
                    crossfade: true,
                    ..
                }
            ) {
                f.push((e, s.clone()));
            }
        }
        return Ok(());
    }
    debug_assert!(
        r.settled.is_empty(),
        "the model changed crossfades the import made: {:?}",
        r.settled
    );
    for (_, line) in &r.settled {
        report.add(ItemKind::Changed, CROSSFADES_PLAIN, Some(line.clone()));
    }
    Ok(())
}

fn count_notes<'e>(bodies: impl Iterator<Item = &'e EventBody>) -> u64 {
    let n = bodies
        .filter(|b| matches!(b, EventBody::Note { .. }))
        .count();
    u64::try_from(n).unwrap_or(u64::MAX)
}
