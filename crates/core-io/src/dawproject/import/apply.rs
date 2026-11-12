//! Applying a mapped import to a new project.

use super::*;

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
) -> Result<(), ModelError> {
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
                continue;
            };
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
    }
    Ok(())
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
/// info. Merges the import's report into `report` and fills its summary.
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
    for t in &plan.tracks {
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
        let mut owned: Vec<StripId> = Vec::new();
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
        for strip in owned {
            apply_mix(
                h,
                strip,
                &t.mix,
                &buses,
                t.flags.is_none(),
                false,
                &mut states,
            )?;
            plugins = plugins.saturating_add(slots(&t.mix));
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
    if let (Some(mix), Some(_)) = (&plan.builtin, builtin) {
        let strip = strip_of(h, StripKind::BuiltinSynth)?;
        apply_mix(h, strip, mix, &buses, false, false, &mut states)?;
        plugins = plugins.saturating_add(slots(mix));
    }
    for (b, id) in plan.buses.iter().zip(buses.clone()) {
        apply_mix(h, id, &b.mix, &buses, true, false, &mut states)?;
        plugins = plugins.saturating_add(slots(&b.mix));
    }
    let master = h.doc().mixer().master();
    apply_mix(h, master, &plan.master, &buses, true, true, &mut states)?;
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
    let s = &mut report.summary;
    s.tracks = s.tracks.saturating_add(lanes);
    s.markers = s
        .markers
        .saturating_add(u64::try_from(plan.markers.len()).unwrap_or(u64::MAX));
    s.plugins = s.plugins.saturating_add(plugins);
    s.tempo_map = true;
    Ok(states)
}
