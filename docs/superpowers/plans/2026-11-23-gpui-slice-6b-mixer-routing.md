# 27.1.0 Slice 6b: Mixer Routing, Groups, VCAs, Slots and the Channel Strip Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete the mixer on top of slice 6a: sends (knob, level box, pre-fader, on, remove, New Bus from a strip), output routing rules and the `mixer.*` strip commands, strip colours and the strip menu, fader groups (tag, link mutes/solos, followers, ⌥ alone), VCAs (columns, members, offsets, marks, modes), insert/EQ/instrument slots shown with their non-editor actions, the left-pane Channel Strip in the Arrange view, external MIDI destination strips, and the Mixer menu — with the Swift strings, labels and undo steps, driven over the control channel.

**Architecture:** As 6a: behaviour in `vizdaw-app` (`mixer/` grows sends, groups, VCAs, slots and MIDI strips; most changes go through `run_mixer`), views in `vizdaw-gpui` (`mixer_view.rs` rows that 6a left blank; a VCA column; the left pane's `StripFocus` strip; the Mixer menu). Reuse 6a's controls (knob, number box, toggle, menu face, meter).

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin; `core-ffi`, `core-control`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md` (§4, §8, §10). Parity checklist: Mixer rows. Slice 6a plan and Outcome (rulings, deferred minors, M1 numbers): `docs/superpowers/plans/2026-11-23-gpui-slice-6a-mixer-core.md`. Swift reference and exact strings: the slice 6 inventory (§1.3 labels, §1.4 gestures, §1.6 model rules, §3 commands, §5 tests, §6 e2e, §7.3 traps, §7.4 the 6b list).

## Global Constraints

- All of slice 6a's Global Constraints apply (song format, `vizdaw-app` without GPUI, registry-sourced commands, `result_json` JSON, core-owned gestures and undo labels, exact Swift strings/labels, touch release rules, palette tokens, registry roles, no GUI for subagents, naming rules, CI, trailers).
- Send indexes: 1-based in the registry, `state.get` and `send:<strip>:<n>` targets; 0-based in the FFI (`edit_send`, `SendLevel{send}`).
- Keys fill `strips` from the selection for `mixer.new_group`/`mixer.new_vca`; the control channel never does.
- Every new drag control registers its window-wide move/up listeners on every paint and checks at event time (slice 6a Task 4 Critical).
- Out of this slice: plug-in editor windows and the Built-in Synth sound window (host track H2: the slot's "Open" item and double-click are disabled with no new strings), the plug-in browser/catalogue for adding inserts, EQ and instruments (a later Plug-in Manager slice: "+"/"EQ ＋"/Instruments show disabled), MIDI Learn and the Maps sheet, surfaces, Co-Producer badges and Make Mine, themes, the MIDI FX section, recording inputs beyond 6a's arm/monitor.

## Review Focus

1. Group drags: a fader drag in a group moves its followers in one core step ("Move Group <name>"), ⌥ at press moves the fader alone, and a follower that vanishes mid-drag releases nothing twice.
2. Send knobs and boxes follow the 6a drag/typed rules (no touch left open; strict grammar) with 1-based indexes everywhere the user or channel sees them.
3. VCA offsets: a member's shown level includes the VCA offset; a VCA at −∞ mutes its members' output in the display as Swift does; removing a VCA or a member restores the plain level.
4. Slots without editors: bypass, remove, reorder and sidechain work and are undoable; "Open"/double-click are disabled; a faulted or placeholder slot shows its state and refuses nothing silently.
5. The left-pane Channel Strip follows the selection (selected tracks of the shown sequence, else armed tracks), shows the right strip kind (audio path, instrument, Built-in Synth, MIDI) and the Swift note when nothing is focused; its drags share the mixer model's placements rules.

---

### Task 1: core-app — sends, output rules and the strip commands

**Files:** `crates/vizdaw-app/src/mixer/{model.rs,commands.rs,…}`, `session.rs`/`testing.rs` as needed; tests `crates/vizdaw-app/tests/{mixer_sends.rs,mixer_commands.rs}`.
**Produces:** send model (levels, pre-fader, on, add/remove, New Bus fed by a strip), output routing rules, strip colour, and the commands `mixer.set_strip`, `mixer.set_output`, `mixer.add_send`, `mixer.set_send`, `mixer.remove_send`, `mixer.remove_bus`, `mixer.set_colour`, `automation.mode.*`, `automation.suspend` with Swift validation and strings (Make Mine is Co-Producer: out).
- [ ] Tests ported from Swift `MixerModelTests`/`MixerCommandsTests` (sends, outputs, colours) plus 1-based/0-based index tests; implement; commit `core-app: mixer sends, outputs and strip commands`.

### Task 2: core-app — fader groups and VCAs

**Files:** `crates/vizdaw-app/src/mixer/{groups.rs,vca.rs,model.rs,commands.rs}`; tests `crates/vizdaw-app/tests/{mixer_groups.rs,mixer_vca.rs}`.
**Produces:** groups (new from selection, link mutes/solos, remove member, ungroup, followers via core values, ⌥ alone), VCAs (new from selection, assign/remove members, rename, delete, offsets and their text, marks, four modes), and their `mixer.*` ids; VCA columns in `columns()` order.
- [ ] Tests ported from Swift group/VCA tests; implement; commit `core-app: fader groups and VCAs`.

### Task 3: core-app — slots and MIDI destination strips

**Files:** `crates/vizdaw-app/src/mixer/{slots.rs,midi.rs,model.rs}`; tests `crates/vizdaw-app/tests/{mixer_slots.rs,midi_strips.rs}`.
**Produces:** slot rows for instrument, Built-in Synth, EQ and inserts (states: faulted, live, bypassed, loading, placeholder), bypass, remove, reorder, sidechain, Load / Load All (safe mode), EQ remove; external MIDI destination strips (volume 0–127, pan, mute, activity, `destination.set_midi_mix`, drag previews every 20 ms per Swift `MidiStripMath`).
- [ ] Tests; implement; commit `core-app: mixer slots and MIDI strips`.

### Task 4: GPUI — sends, slots and the strip menu in the strips

**Files:** `crates/vizdaw-gpui/src/{mixer_view.rs,controls/*,menus.rs}`; tests `crates/vizdaw-gpui/tests/{mixer.rs,mixer_slots.rs}`.
**Produces:** the rows 6a left blank: EQ slot, inserts (dots, bypass, remove, drag reorder, sidechain menu; "+" disabled), sends (name line, PRE chip, knob, level box, send menu, New Bus / new-bus plus), the strip header menu (Rename…, colours, Group, Assign to VCA ▸, New VCA, Remove from VCA ▸, Remove Bus, Set All Strips to); exact labels per inventory §1.3; the Mixer menu (Swift `CommandMenu("Mixer")`).
- [ ] Tests (labels, gestures, menus, undo names); implement; visual check by the controller; commit `gpui: sends, slots and strip menus`.

### Task 5: GPUI — groups, VCA columns and MIDI strips

**Files:** `crates/vizdaw-gpui/src/{mixer_view.rs,vca_strip.rs,midi_strip.rs}`; tests `crates/vizdaw-gpui/tests/{mixer_groups.rs,mixer_vca.rs}`.
**Produces:** group tags and their menu, group drags with followers and ⌥ alone; VCA columns (name, members line, fader, level, M S, mode menu, offset row and mark, rename popover); external MIDI destination strips (6-wide activity meter).
- [ ] Tests; implement; commit `gpui: groups, VCAs and MIDI strips`.

### Task 6: GPUI — the left-pane Channel Strip

**Files:** `crates/vizdaw-gpui/src/{panes.rs,channel_strip_pane.rs}`, `crates/vizdaw-app/src/view.rs` (StripFocus is there); tests `crates/vizdaw-gpui/tests/channel_strip.rs`.
**Produces:** the left pane's "strip" view: `StripFocus` from the selection (else armed tracks), one strip sized to the pane height − 16 with vertical scrolling, the Swift note "Select a track to see its channel strip." when nothing is focused, inspector placeholder for the other kinds; a third mixer placement with 6a's rules.
- [ ] Tests; implement; commit `gpui: the left-pane channel strip`.

### Task 7: e2e and checklist

**Files:** `crates/vizctl/tests/e2e_gpui.rs`.
**Produces:** GPUI ports of `groups_vcas_and_transport_bar_over_the_channel` (groups and VCA steps; the `command:` bar item stays a noted gap), `midi_mix_reaches_state_get`, `default_eq_and_new_bus_over_the_channel` (the New Bus part; the EQ install part needs the catalogue — noted), the left-pane steps of `window_views_and_panes_over_the_channel`.
- [ ] Write the tests (`#[ignore]`); controller runs them on the Mac and Linux, does the visual checks and ticks the checklist; commit `e2e: mixer routing, groups and VCAs`.
