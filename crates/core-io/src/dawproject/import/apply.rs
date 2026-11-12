//! Applying a mapped import to a new project.

use super::timeline::{AutoAt, Body, CROSSFADES_PLAIN, SlotAt, StripAt, UNPLACED};
use super::*;
use core_model::automation::{AutomationTarget, SendIndex, target_exists};
use core_model::command::{EventCmd, MediaCmd};
use core_model::document::MediaOrigin;
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::ids::MediaId;
use core_model::sequence::SeqLength;

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
/// track, destination, strip and slots, the buses' and the master's.
pub(super) struct Applied {
    pub(super) lanes: Vec<LaneId>,
    pub(super) dests: Vec<Option<DestId>>,
    /// The strip automation of the track drives, and its slots.
    pub(super) strips: Vec<Option<(StripId, MixSlots)>>,
    pub(super) instruments: Vec<Option<PluginInstanceId>>,
    pub(super) buses: Vec<(StripId, MixSlots)>,
    pub(super) master: (StripId, MixSlots),
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
    buses: &[StripId],
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
            && let Some(bus) = buses.get(b).copied()
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
            let Some(target) = buses.get(s.bus).copied() else {
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
/// import's report into `report` and fills its summary.
pub fn apply(
    h: &mut History,
    imp: &DpImport,
    seq: SeqId,
    report: &mut Report,
) -> Result<SlotStates, ModelError> {
    let doc = h.doc();
    doc.sequence(seq)?;
    if doc.sequences().values().any(|s| !s.lanes.is_empty()) {
        return Err(ModelError::invalid(NOT_NEW));
    }
    report.merge(imp.report.clone());
    let plan = &imp.plan;
    let mut states = Vec::new();
    let mut plugins = 0u64;

    let mut buses = Vec::with_capacity(plan.buses.len());
    for b in &plan.buses {
        let r = run(
            h,
            Command::Mixer(MixerCmd::AddBus {
                name: b.name.clone(),
                index: None,
            }),
        )?;
        let id = created(&r, |id| match id {
            AnyId::Strip(s) => Some(*s),
            _ => None,
        })
        .ok_or_else(|| ModelError::invalid("no bus was created"))?;
        buses.push(id);
    }

    let mut builtin: Option<DestId> = None;
    if plan.tracks.iter().any(|t| t.midi == Some(Midi::Builtin)) {
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
    }

    let mut lanes = 0u64;
    let mut placed = Applied {
        lanes: Vec::with_capacity(plan.tracks.len()),
        dests: Vec::with_capacity(plan.tracks.len()),
        strips: Vec::with_capacity(plan.tracks.len()),
        instruments: Vec::with_capacity(plan.tracks.len()),
        buses: Vec::new(),
        master: (h.doc().mixer().master(), MixSlots::default()),
    };
    // Tracks on the built-in synth, whose strip comes later.
    let mut on_builtin: Vec<usize> = Vec::new();
    for (ti, t) in plan.tracks.iter().enumerate() {
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
        placed.lanes.push(lane);
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
                        name: t.name.clone(),
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
            owned.push(strip_of(h, StripKind::AudioPath(out))?);
        }
        let mut first: Option<(StripId, MixSlots)> = None;
        for strip in owned {
            let made = apply_mix(
                h,
                strip,
                &t.mix,
                &buses,
                t.flags.is_none(),
                false,
                &mut states,
            )?;
            first.get_or_insert((strip, made));
            plugins = plugins.saturating_add(slots(&t.mix));
        }
        if first.is_none() && t.midi == Some(Midi::Builtin) {
            on_builtin.push(ti);
        }
        placed.strips.push(first);
        placed.dests.push(dest_of);
        placed.instruments.push(instrument);
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
    if let (Some(mix), Some(_)) = (&plan.builtin, builtin) {
        let strip = strip_of(h, StripKind::BuiltinSynth)?;
        let made = apply_mix(h, strip, mix, &buses, false, false, &mut states)?;
        plugins = plugins.saturating_add(slots(mix));
        for ti in on_builtin {
            if let Some(s) = placed.strips.get_mut(ti) {
                *s = Some((strip, made.clone()));
            }
        }
    }
    for (b, id) in plan.buses.iter().zip(buses.clone()) {
        let made = apply_mix(h, id, &b.mix, &buses, true, false, &mut states)?;
        placed.buses.push((id, made));
        plugins = plugins.saturating_add(slots(&b.mix));
    }
    let master = h.doc().mixer().master();
    let made = apply_mix(h, master, &plan.master, &buses, true, true, &mut states)?;
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
    apply_arranged(h, plan, seq, &placed, report)?;
    let s = &mut report.summary;
    s.tracks = s.tracks.saturating_add(lanes);
    s.markers = s
        .markers
        .saturating_add(u64::try_from(plan.markers.len()).unwrap_or(u64::MAX));
    s.plugins = s.plugins.saturating_add(plugins);
    s.tempo_map = true;
    Ok(states)
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
            StripAt::Bus(b) => placed.buses.get(b).map(|(id, m)| (*id, m)),
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

/// Applies the arrangement once the structure and the maps are in place
/// (so regions and crossfades see the tempo map): the media items, the new
/// sequences, then each track's events in one insert.
fn apply_arranged(
    h: &mut History,
    plan: &Plan,
    seq: SeqId,
    placed: &Applied,
    report: &mut Report,
) -> Result<(), ModelError> {
    let arr = &plan.arranged;
    let run = |h: &mut History, cmd: Command| h.execute(&cmd, &ViewFilter::default());
    let (mut notes, mut regions, mut points) = (0u64, 0u64, 0u64);
    let mut media: Vec<MediaId> = Vec::with_capacity(arr.media.len());
    for m in &arr.media {
        let r = run(
            h,
            Command::Media(MediaCmd::Add {
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
        )?;
        media.push(created_id(&r, |id| match id {
            AnyId::Media(m) => Some(*m),
            _ => None,
        })?);
    }
    let mut seqs: Vec<SeqId> = Vec::with_capacity(arr.seqs.len());
    for s in &arr.seqs {
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
            settled(&r, report);
        }
        seqs.push(id);
    }
    for (ti, items) in arr.items.iter().enumerate() {
        let Some(lane) = placed.lanes.get(ti).copied() else {
            continue;
        };
        if items.is_empty() {
            continue;
        }
        let mut specs = Vec::with_capacity(items.len());
        for it in items {
            let (body, group_channel) = match &it.body {
                Body::Midi(b, gc) => (b.clone(), *gc),
                Body::Use {
                    seq: k,
                    length,
                    loop_count,
                } => {
                    let Some(&target) = seqs.get(*k) else {
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
                    let Some(&m) = media.get(r.media) else {
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
                tick: it.tick,
                dest: None,
                group_channel,
                mute: it.mute,
                body,
            });
        }
        notes = notes.saturating_add(count_notes(specs.iter().map(|e| &e.body)));
        let r = run(
            h,
            Command::Event(EventCmd::Insert {
                seq,
                lane,
                events: specs,
            }),
        )?;
        settled(&r, report);
    }
    let s = &mut report.summary;
    s.notes = s.notes.saturating_add(notes);
    s.regions = s.regions.saturating_add(regions);
    s.automation_points = s.automation_points.saturating_add(points);
    s.audio_files = s
        .audio_files
        .saturating_add(u64::try_from(media.len()).unwrap_or(u64::MAX));
    Ok(())
}

/// Lists what the model's crossfade rules changed in an insert. The
/// timeline mapping places crossfades by the model's own rule and tempo
/// lookup, so this should never happen; if it does, the change is reported,
/// never dropped.
fn settled(r: &CmdReport, report: &mut Report) {
    debug_assert!(
        r.settled.is_empty(),
        "the model changed crossfades the import made: {:?}",
        r.settled
    );
    for (_, line) in &r.settled {
        report.add(ItemKind::Changed, CROSSFADES_PLAIN, Some(line.clone()));
    }
}

fn count_notes<'e>(bodies: impl Iterator<Item = &'e EventBody>) -> u64 {
    let n = bodies
        .filter(|b| matches!(b, EventBody::Note { .. }))
        .count();
    u64::try_from(n).unwrap_or(u64::MAX)
}
