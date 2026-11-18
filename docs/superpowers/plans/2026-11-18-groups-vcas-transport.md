# Fader Groups, VCAs and a Customizable Transport Bar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Linked fader groups, VCA faders, and a transport bar the user can rearrange, trim and extend with command buttons. The bar always fits on one row, and anything that doesn't fit goes into a » menu.

**Architecture:**
- **`core-model`.** Groups and VCAs live in the `Mixer`, with their own IDs. A new `groups` module holds the group-move and heard-level arithmetic. A new `command/groups.rs` holds `GroupCmd`. The format goes to VERSION 11.
- **`core-engine`.** The graph adds each strip's VCA offsets to its fader. This happens in the static gains and in `Controls::commit`, so the live engine, the offline bounce and render all use the same sum. A VCA fader is a new `AutoDest`. Every buffer is built off the audio thread.
- **`core-control` and `core-ffi`.** There are 14 new registry commands, run through `run_mixer`. The group rule lives in the core:
  - `mixer.set_strip` fans a move out to the group;
  - the gesture path does the same, through a new child module `session/groups.rs`.

  So the mouse, surfaces, the channel, automation writing and Co-Producer all follow it.
- **Swift.** The mixer gets group tags, a VCA section, VCA offset readouts and a Mixer menu. The transport bar becomes a list of items, laid out on one row by an AppKit container driven by a pure fit function. A customize sheet edits that list, and the list is saved in preferences.

**Tech Stack:** Rust 2024 (workspace, UniFFI 0.32.2), Swift 6.1 / SwiftUI + AppKit (macOS 14+), Swift Testing. No new third-party dependency.

**Spec:** `docs/superpowers/specs/2026-11-18-groups-vcas-transport-design.md`, approved by the owner, is binding. That includes its "Settled after the builder check" section (§2a). The owner added on 2026-11-18 that the transport bar is always one row, with a » overflow (spec §5 "Room"). The Co-Producer gate follows the rules of plan 8d (`docs/superpowers/plans/2026-11-16-co-producer-8d.md`): Ruling 1's command shape and Ruling 2's "the strip decides". Executors read both documents.

## Global Constraints

- Version: `26.0.0` (a beta the owner shares with friends; 26.1.0 is the official release). This plan changes no version string.
- Format: `core_model::file::VERSION` goes from 10 to 11, once, in Task 1. An older build then says "made by a newer VizDAW", not "Corrupt".
- Co-Producer:
  - In 26.0.0 Co-Producer never writes MIDI notes, on any track, its own included (owner, 2026-11-16).
  - Changing or deleting the composer's work, routing or sound needs an approval-sheet yes.
  - A Co-Producer bus's own send going to −∞, or being retargeted, needs no yes (owner, 2026-11-18).
- Programmatic and control-channel calls never open dialogs. The only exceptions are the import's new-project window, Co-Producer's approval sheet (only for a pending request) and the starter project's one new song window. A transport command button is a user click (spec §5), not a programmatic call.
- New commands get hostile-input tests. New FFI goes inside `ffi_guard`. Every new UniFFI record field gets `#[uniffi(default)]`, so Swift fakes keep building.
- Audio thread: no allocation and no lock in anything marked `// RT`. Two kinds of test guard this:
  - the counting-allocator tests (`crates/core-engine/tests/rt_alloc_*.rs`, armed around `OfflineBackend::block`), extended in Tasks 4 and 6;
  - Task 4's source scan `controls_take_no_lock`.
- Swift: build and test only through `scripts/swift.sh`, `scripts/build-ffi.sh` and `scripts/make-app.sh`. Swift 6.1 rules:
  - no `weak self` or non-Sendable values inside `MainActor.assumeIsolated`;
  - no `@State` or `@FocusState`;
  - AppKit main-actor globals (`NSApp`, `NSEvent.modifierFlags` and friends) only from `@MainActor` code; CI's Xcode 16.4 refuses other uses.
- UI: accessibility labels and theme tokens on every control. The UI says "track", never "lane".
- Tests:
  - never open real audio or MIDI devices, and never trigger mic/TCC prompts;
  - never touch the real `~/Library` (Application Support/VizDAW, prefs, Trash, Audio/Plug-Ins, Keychain) or the real UserDefaults (use `MemoryDefaults`);
  - Swift tests that make a real Session set `TestSupportDir.shared`;
  - the e2e uses vizctl's throwaway home and `VIZDAW_TEST_HOOKS=1`;
  - on Linux CI, AU/CF/importer tests are macOS-gated.
- Never run `auval`. No GUI or computer-use tools. Never use dangerouslyDisableSandbox.
- Names:
  - banned vendor and DAW names appear only in the name scans (`app/Tests/VizDAWAppModelTests/NameScanTests.swift`, `crates/core-control/tests/names.rs`) and in licence notices;
  - the reference DAW's repo name stays out of repo text.
- Commits:
  - by explicit paths;
  - never commit `.DS_Store`, `.superpowers/` or generated FFI files;
  - brand assets are never committed;
  - each commit ends with the two trailer lines `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P`.
- Env: `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`.
- Linux clippy: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`.
- **End-of-task checks** (every task): `cargo fmt --check`, `cargo test --workspace`, Linux clippy, and the name scans. When FFI or Swift changed, also `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. Every task leaves the app building and every test green.
- Pace (owner): fix crashes, data loss, wrong overwrites, security holes and rule violations. Narrow edge cases go to the backlog.

## Review Focus

1. **A group drag past the top and back must keep the balance.** A member at +10 stops a +6 move at +2. Dragging back down must land every member where it started, not where the stop left it, because the move is measured from the bases taken at touch. (Task 2: `a_group_drag_past_the_top_and_back_keeps_the_balance`.)
2. **A member another hand holds stays with that hand.** Take a group member whose fader is held on a control surface. Dragging another member with the mouse must not move it or double-move it. When the surface lets go, the member must not jump. (Task 2: `a_member_held_by_another_hand_is_not_moved`.)
3. **Deleting a track whose strip is a member is one undo step, and Undo brings the membership back.** That covers the last member of a VCA too: the VCA stays, empty, and Undo puts the strip back in it. (Task 5: `deleting_a_member_strip_is_one_step_and_undo_restores_membership`.)
4. **A damaged or newer transport preference must not cost the user their bar.** Garbage JSON, or a layout version this build does not know, shows the default bar. It must not crash, and it must not overwrite the stored preference until the user changes the bar. (Task 9: `aDamagedPreferenceGivesTheDefaultAndIsKept`.)
5. **The mixer window at 300 pt shows the play cluster and », and every other item is in ».** Widening the window brings them back in order, and none is lost on the way. (Task 9: `theMixerWindowRowAt300KeepsEverything`.)

## Plan Rulings

These are this plan's decisions where the spec leaves a choice. Each one names its cost if wrong.

1. **Storage.** `Mixer` gains `groups: OrdMap<GroupId, Group>` and `vcas: OrdMap<VcaId, Vca>`. Each holds its `members: Vector<StripId>` in the order they were added.
   - `MixerRepr` writes them as `groups` and `vcas` lists, skipped when empty. `Counters` gains `group` and `vca`, with `#[serde(default)]`.
   - The first ID allocated is 1, as the registry's IDs start at 1.
   - Membership lives in the group or VCA, not on `Strip`, so `Strip` and its file shape do not change.
   - The VERSION 11 migration is the identity. Its doc says groups and VCAs are new optional lists.
   - **Cost if wrong:** a later move to per-strip fields is one migration.
2. **Group moves.**
   - The move is measured in dB from each member's base, taken at the touch (a gesture) or read from the document (`mixer.set_strip`).
   - `FADER_FLOOR_DB = -60.0` is "the fader's lowest finite step", matching `MixerMath.faderBottomDb`. A downward move stops when any finite member would go below it. A member already below it blocks downward moves.
   - A group move never sends a member to −∞. To silence one member, the user moves it alone (⌥).
   - A leader at −∞ moves alone, because it has no dB to measure from.
   - **Cost if wrong:** one fader moves alone where the user expected the group to follow.
3. **One group per strip.** Adding a strip that is in another group is refused: "Strip ‹name› is in group ‹name›; remove it from that group first." It is never moved silently.
4. **Limits.** A group or VCA holds at most 128 members (`MAX_MEMBERS`), which bounds the gesture fan-out against `MAX_LIVE` (1024).
   - Names are 1–64 characters, trimmed, with no control characters.
   - The defaults are "Group N" and "VCA N", where N is one more than the count.
   - Colours are one of the eight `Swatch`es, required. The default is `Swatch` order by count.
5. **Writing modes.** §2a's "Touch or Latch" is read as every writing mode. Each member's gesture takes the path its own strip's mode gives, as a single touch would.
   - Stopped, a group move is one undo step "Move Group ‹name›".
   - While a pass writes, the static members' moves are that one step, and the writing members' lanes land with the pass, as every write does.
6. **VCA automation modes** are Off, Read, Touch and Latch. Write and Trim are refused: "A VCA's automation mode is Off, Read, Touch or Latch." A backlog row covers them.
7. **VCA lane timing.** A VCA lane plays at the latest arrival (PDC) among its members' strips. A backlog row covers per-member alignment.
8. **DAWproject has no place for VCA membership.** The schema's `Channel` has only `destination`, which is audio routing.
   - Export writes each VCA as a `Channel role="vca"`, with name, volume, mute, solo and its fader automation. It adds a report line that says which assignments the file cannot hold. Members keep their own faders.
   - Import makes each VCA channel a VCA with no members, and says so. The "VCA channels" left-out line goes.
   - Merge keeps today's behaviour.
   - **Cost if wrong:** the composer reassigns VCA members after an import.
9. **AAF** writes each track's own fader. It lists VCAs and groups as left out.
10. **Co-Producer.**
    - Groups and VCAs carry no Co-Producer mark: their members decide.
    - A group or VCA item asks when any member, before or after, is the composer's strip. That includes rename and colour, which is stricter than §4's list (cost: one extra question).
    - `mixer.new_group` and `mixer.new_vca` are Add without `strips` and Change with it, as `mixer.new_bus` is with `strip`.
    - `vizdaw-mcp` leaves `strips` out of both tools' schemas.
11. **An absent `strips` is always empty.** The Mixer menu and the key path pass the selection explicitly (`KeyDispatch` fills `strips` for `mixer.new_group` and `mixer.new_vca`). So no channel or Co-Producer call ever acts on the composer's selection.
12. **The Mixer menu.** A new `CommandMenu("Mixer")` holds **Group**, **Ungroup**, **New VCA**, **Assign to VCA ›** (each VCA, then "New VCA") and **Remove from VCA ›**. None has a default shortcut.
13. **Transport items.**
    - **Click** is not an item, because VizDAW has no metronome outside count-in. A backlog row covers it.
    - **Count-in** is a toggle item (`RecordModel.toggleCountIn`). It is not in the default.
    - **The fixed cluster** is Return to Start, Rewind, Fast Forward, Play/Stop (one button, as today) and Record, with Record's counting-in, pass and time-left readouts.
    - **The default** is today's bar, with Loop just after the cluster (cost: Loop sits after Record instead of before it).
    - **DSP load** is an item, not in the default. It polls `session.levels()` through the document's `MixerModel` at most 4 times a second while shown.
    - **The faults button, the main-view tabs and the pane toggles** are document-window items. The mixer window skips them: they are neither shown nor listed in ».
14. **The one-row container** is AppKit: `TransportRowView` in `VizDAWEditors`, driven by `TransportRowFit`.
    - Flexible spaces and separators are never listed in ». A separator next to an edge, the » or another separator is hidden.
    - What a folded item becomes in » (`TransportMenuEntry`):
      - a button becomes an action, with a check for a toggle;
      - a readout becomes its current text, disabled;
      - a field, Record Options or the main-view tabs become a popover of the item's own view, anchored to ».
15. **Co-Producer recipes** are vizdaw-mcp prompts, not registry commands. A command button cannot pick one in 26.0.0. A backlog row covers it.
16. **Command buttons run as a key does** (`KeyDispatch.command`). On `.pass`, they perform the command's main-menu item, as an unused key equivalent would reach it. So Bounce… opens its sheet.
17. **Layout preference.** JSON is stored under `VizDAW.transportBar`, `{"version": 1, "items": [...]}`. A damaged or newer value gives the default and is left untouched until the user changes the bar.
18. **Transport on the channel.**
    - `ui.customize_transport` answers `dialog_required`.
    - `ui.set_transport_bar {items}` and `ui.reset_transport_bar` are the channel's way to change the bar.
    - All three are Refused to Co-Producer (app-wide preferences).

## Commands added (registry 217 → 234)

All are `key_only` unless marked (menu), `document: true` except the `ui.*` three, menu "Mixer" or "View". IDs are `mixer_id`s (1 to `MAX_EXACT_ID`). `strips` is Text through `co_producer::id_list` (1–64 IDs). Levels are Text through `registry::decimal` (−144..12). Colours are `GROUP_COLOURS`, the eight swatch names without `none`.

| id | params | class | title |
|---|---|---|---|
| `mixer.new_group` (menu) | `strips?`, `name?`, `colour?` | add (change with `strips`) | "Group" |
| `mixer.rename_group` | `group`, `name` | change | "Rename Group" |
| `mixer.set_group_colour` | `group`, `colour` | change | "Group Colour" |
| `mixer.add_to_group` | `group`, `strips` | change | "Add to Group" |
| `mixer.remove_from_group` | `group`, `strips` | change | "Remove from Group" |
| `mixer.set_group_links` | `group`, `mute?`, `solo?` (Flag; at least one) | change | "Group Links" |
| `mixer.delete_group` (menu) | `group` | remove | "Ungroup" |
| `mixer.new_vca` (menu) | `strips?`, `name?`, `colour?` | add (change with `strips`) | "New VCA" |
| `mixer.rename_vca` | `vca`, `name` | change | "Rename VCA" |
| `mixer.set_vca_colour` | `vca`, `colour` | change | "VCA Colour" |
| `mixer.assign_vca` | `vca`, `strips` | change | "Assign to VCA" |
| `mixer.unassign_vca` | `vca`, `strips` | change | "Remove from VCA" |
| `mixer.set_vca` | `vca`; `gain_db?`, `mute?`, `solo?`, `mode?` (Choice `off read touch latch`); at least one | change | "Set VCA" |
| `mixer.delete_vca` | `vca` | remove | "Delete VCA" |
| `ui.set_transport_bar` | `items` (Text, Task 9's grammar) | refused | "Set Transport Bar" |
| `ui.reset_transport_bar` | none | refused | "Reset Transport Bar" |
| `ui.customize_transport` (menu "View") | none; `DialogSpec` "Customize Transport Bar", always | refused | "Customize Transport Bar…" |

Existing commands gain:
- `mixer.set_strip`: `alone?` (Flag; true moves this strip only);
- `automation.gesture`: `alone?`, and its `target` text takes `vca:<id>`.

`registry::GROUP_IDS` lists the 14 `mixer.*` ids in table order.

## Not in this plan

These are backlog rows, added in Task 12:
- nested VCAs, spill, and group modes beyond the one (spec non-goals);
- per-project transport layouts;
- VCAs on control surfaces;
- VCA Write and Trim modes;
- per-member VCA lane alignment;
- a Click item once VizDAW has a metronome;
- command buttons for Co-Producer recipes;
- the Record ⌄ popover rethink (backlog row 93's second half).

---

### Task 1: Groups in the model (`core-model`)

**Files:**
- Create: `crates/core-model/src/groups.rs` (types and arithmetic), `crates/core-model/src/command/groups.rs` (`GroupCmd`)
- Modify:
  - `crates/core-model/src/ids.rs` (`GroupId` "group", `VcaId` "vca", `AnyId::Group`, `AnyId::Vca`, `Counters::{group, vca}`, `alloc_group`, `alloc_vca`);
  - `crates/core-model/src/mixer.rs` (fields, `MixerRepr`, accessors; `remove` ~424 drops membership);
  - `crates/core-model/src/command/mod.rs` (`Command::Groups(GroupCmd)` in `name` and `apply_in`);
  - `crates/core-model/src/mixer_rules.rs` (`mixer_problems` checks groups);
  - `crates/core-model/src/file.rs` (VERSION 11, `migrate_v10_to_v11`);
  - `crates/core-model/src/lib.rs`;
  - the `assert_eq!(VERSION, 10)` lines in `crates/core-model/tests/{file,input_fx,mpe,learn}.rs`, which become 11.
- Test: `crates/core-model/tests/groups.rs` (new)

**Interfaces:**
- Produces:
  ```rust
  // groups.rs
  pub const MAX_MEMBERS: usize = 128;
  pub const NAME_MAX: usize = 64;
  pub const FADER_FLOOR_DB: f64 = -60.0;
  pub struct Group { pub id: GroupId, pub name: String, pub colour: Swatch, pub members: Vector<StripId>,
                     pub link_mute: bool, pub link_solo: bool }           // serde, deny_unknown_fields
  pub enum Link { Mute, Solo }
  /// The clamped delta (Ruling 2): ≤ GAIN_MAX_DB − max finite base, ≥ min(0, FADER_FLOOR_DB − min finite base);
  /// 0 for no finite base or a non-finite `wanted`.
  pub fn group_delta(bases_db: &[f64], wanted: f64) -> f64;
  /// `strip`'s fader to `gain_db` with its group following, leader first; just `[(strip, gain_db)]` when
  /// ungrouped, alone, or the leader is at −∞. Members at −∞ are left out.
  pub fn group_gains(m: &Mixer, strip: StripId, gain_db: f64) -> Vec<(StripId, f64)>;
  /// `strip` and, when its group links `link`, the other members.
  pub fn linked(m: &Mixer, strip: StripId, link: Link) -> Vec<StripId>;
  pub fn group_of(m: &Mixer, strip: StripId) -> Option<&Group>;
  /// Trimmed, 1–NAME_MAX chars, no control chars; else ModelError::invalid naming the rule.
  pub fn check_name(what: &str, name: &str) -> Result<String>;
  // mixer.rs
  impl Mixer { pub fn groups(&self) -> &OrdMap<GroupId, Group>; pub fn group(&self, id: GroupId) -> Result<&Group>;
               pub(crate) fn group_mut(&mut self, id: GroupId) -> Result<&mut Group>; }
  // command/groups.rs
  pub enum GroupCmd {
      NewGroup { name: String, colour: Swatch, members: Vec<StripId> },
      RenameGroup { group: GroupId, name: String },
      SetGroupColour { group: GroupId, colour: Swatch },
      AddToGroup { group: GroupId, strips: Vec<StripId> },
      RemoveFromGroup { group: GroupId, strips: Vec<StripId> },
      SetGroupLinks { group: GroupId, mute: Option<bool>, solo: Option<bool> },
      DeleteGroup { group: GroupId },
  }
  impl GroupCmd { pub fn name(&self) -> &'static str }  // "New Group", "Rename Group", "Group Colour",
                                                         // "Add to Group", "Remove from Group", "Group Links", "Delete Group"
  ```
  `NewGroup` sets both links on and pushes `AnyId::Group(id)` to `Report::created`.

- [ ] **Step 1: Write the failing tests** in `tests/groups.rs`, on a document with strips A, B and C, a bus and the master:
  - `a_group_move_keeps_the_balance`: members at −6, −3 and 0. `group_gains(m, B, 0.0)` is `[(B, 0.0), (A, -3.0), (C, 3.0)]`.
  - `a_group_move_stops_at_the_top`: members at 0 and +10. Moving the first to +6 gives `[(A, 2.0), (B, 12.0)]`.
  - `a_group_move_stops_at_the_lowest_finite_step`: members at −50 and −20. Moving the second to −40 gives `[(B, -30.0), (A, -60.0)]`.
  - `a_member_at_minus_infinity_stays_there`: members at −144 and −6. Moving the second to −3 gives `[(B, -3.0)]` only.
  - `a_leader_at_minus_infinity_moves_alone`.
  - `a_member_below_the_floor_blocks_downward_moves`: with a member at −80, `group_delta(&[-80.0, -6.0], -5.0) == 0.0`, and `group_delta(&[-80.0, -6.0], 3.0) == 3.0`.
  - `group_delta_is_total`: a proptest over bases in `[-1e9, 1e9] ∪ {NaN, ±inf, -144}` and any `wanted`. The result is finite, 0 for `NaN` or `±inf`, and inside Ruling 2's bounds.
  - `linked_follows_the_switches`: `linked(m, A, Link::Mute)` is all members with `link_mute`, and `[A]` after `SetGroupLinks { mute: Some(false), .. }`.
  - `a_strip_joins_one_group_only`: `AddToGroup` of a strip in another group is `Err` with Ruling 3's message, and the document is unchanged.
  - `master_cannot_join`: "Master cannot join a group."
  - `removing_a_strip_removes_its_membership`: removing a member bus through `MixerCmd::RemoveBus` leaves the group without it. One `History::undo` restores both the bus and the membership.
  - `empty_groups_stay`: removing every member leaves the group.
  - `hostile_group_commands_change_nothing`: each of these is `Err`, with no panic and `doc` unchanged:
    - `GroupId(u64::MAX)`;
    - a 70 000-character name, a name with NUL, and a blank name;
    - 129 members;
    - a duplicate strip in `strips`;
    - an unknown strip.
  - `groups_save_and_load`: `to_json` writes `"version": 11` and a `groups` list, and `from_json` gives an equal document. A document without groups writes no `groups` key. `migrate_v10_to_v11(v)` is `v`. A version-12 file gives `ModelError::NewerFormat { found: 12 }`.
  - `validate_finds_broken_groups`: each of these JSON documents is `Corrupt` with a problem naming the group:
    - one with a member strip that does not exist;
    - one with a strip in two groups;
    - one with the master as a member.
- [ ] **Step 2: Run** `cargo test -p core-model --test groups`. Expected: FAIL (no `groups` module).
- [ ] **Step 3: Implement** the Interfaces block.
  - `GroupCmd` refuses before changing anything.
  - `Mixer::remove` also removes the strip from every group, and from every VCA once Task 3 adds them.
  - `mixer_problems` checks every member, and that no strip is in two groups.
- [ ] **Step 4: Run** `cargo test -p core-model`. Expected: PASS, including the old file tests at VERSION 11.
- [ ] **Step 5: End-of-task checks. Commit** `feat(model): fader groups in the mixer, format 11`.

### Task 2: Groups in commands and gestures (`core-control`, `core-ffi`)

**Files:**
- Modify:
  - `crates/core-control/src/registry.rs`: the seven group entries after `mixer.remove_bus`; `GROUP_IDS`; `GROUP_COLOURS`; `alone` on `MIXER_SET_STRIP_PARAMS` and `AUTOMATION_GESTURE_PARAMS`;
  - `crates/core-control/tests/commands.rs:565`: 224, with "groups-vcas-transport adds seven group commands";
  - `crates/core-control/tests/co_producer.rs:196`: 224, and the new ids in the class lists;
  - `crates/core-ffi/src/mixer_chores.rs`: `builds` takes `GROUP_IDS`; `commands_for` builds `Command::Groups`; `mixer.set_strip` fans out;
  - `crates/core-ffi/src/session/co_producer_mixer.rs` (`run_mixer`'s step label);
  - `crates/core-ffi/src/session.rs`: `gesture_as`; `gesture_held` calls into `session/groups.rs`; `open_static_share` takes a label;
  - `crates/core-ffi/src/automation.rs` (`AutomationStateRow.touched_values`);
  - `crates/core-ffi/src/mixer.rs` (`GroupRow`, `MixerSnapshot.groups`, `StripRow.group`);
  - `crates/core-ffi/src/control.rs`: `state.get mixer`'s `groups`, and `strip_json`'s `group`;
  - `app/Sources/VizDAWAppModel/MixerCommands.swift` (the seven ids run through `runMixer`);
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift` (the `gestureAs` stub).
- Create: `crates/core-ffi/src/session/groups.rs` (the gesture fan-out)
- Test: `crates/core-ffi/tests/groups.rs` (new), `crates/core-control/tests/commands.rs`

**Interfaces:**
- Consumes: Task 1's `group_gains`, `group_delta`, `linked`, `GroupCmd`.
- Produces:
  ```rust
  // mixer.rs
  pub struct GroupRow { pub id: u64, pub name: String, pub colour: SwatchRow, pub members: Vec<u64>,
                        pub link_mute: bool, pub link_solo: bool }
  // MixerSnapshot: #[uniffi(default)] pub groups: Vec<GroupRow>;  StripRow: #[uniffi(default)] pub group: Option<u64>
  // automation.rs — AutomationStateRow: #[uniffi(default)] pub touched_values: Vec<f64>  // parallel to `touched`, normalised
  // session.rs
  impl Session { pub fn gesture_as(&self, target: AutomationTargetRow, phase: GesturePhaseRow,
                                   holder: GestureHolderRow, alone: bool) -> Result<(), VizError>; }
  // session/groups.rs
  pub(crate) struct GroupDrag { group: GroupId, leader_base_db: f64, followers: Vec<(AutomationTarget, f64)> }
  // Inner gains `group_drags: BTreeMap<AutomationTarget, GroupDrag>` (keyed by the leader's target).
  ```
  `gesture_from` is `gesture_as(.., false)`. `automation.gesture` passes its `alone`.
- **The fan-out.**
  - **Fader touch.** A `StripFader(s)` touch that is not alone, of a grouped `s` whose base is finite, also touches each other member's `StripFader` with the same holder and `value: None`. Members at −∞ are skipped, and so are members already touched by any hand (Review Focus 2). The followers' bases are their heard values in dB.
  - **Fader value.** Each leader value `v` gives `delta = group_delta(all bases, fader_db(v) − leader_base_db)`. Every touched member then moves to `fader_norm(base + delta)`, the leader included, so the leader stops too.
  - **Release.** Releasing the leader releases its followers.
  - **The step.** The shared static edit opens with the label "Move Group ‹name›" when the leader drags a group.
  - **Mute and solo.** A `StripMute`/`StripSolo` touch, value or release that is not alone goes to every `linked(..)` member's same target, with the same value.

- [ ] **Step 1: Write the failing tests.**
  - `crates/core-control/tests/commands.rs`:
    - `group_commands_are_registered`: each of the seven has the table's params, class, menu and title;
    - `mixer.new_group` and `mixer.delete_group` have menu items;
    - `alone` is an optional Flag on `mixer.set_strip` and `automation.gesture`.
  - `crates/core-ffi/tests/groups.rs`, on a saved session with strips Kick, Snare and Hat, grouped as "Drums":
    - `new_group_from_strips_is_one_step`: `run_mixer("mixer.new_group", [strips: "<k>,<s>"])` is one undo step "New Group". The snapshot's `groups[0].members` is `[k, s]`, and each strip's `group` is set.
    - `set_strip_moves_the_group`: `mixer.set_strip {strip: Kick, gain_db: "-3"}` moves every member by −3 dB in one step "Move Group Drums". With `alone: true`, only Kick moves, as "Set Strip".
    - `linked_mute_and_solo_follow_the_switches`: `mixer.set_strip {strip: Kick, mute: true}` mutes all three. After `mixer.set_group_links {group, mute: false}`, only Kick.
    - `a_fader_gesture_moves_the_group_as_one_step`, with the transport stopped:
      - touch, three values and release on `StripFader(Kick)` move all three by the same dB;
      - `undo_label` is "Move Group Drums";
      - one undo restores all three.
    - `an_alone_gesture_moves_one_fader`: the same through `gesture_as(.., alone: true)` moves only Kick, as "Set Fader".
    - `a_group_drag_past_the_top_and_back_keeps_the_balance` (Review Focus 1): members at 0 and +10. Dragging the first to +6 leaves +2 and +12. Dragging it back to 0 in the same touch leaves 0 and +10.
    - `a_member_held_by_another_hand_is_not_moved` (Review Focus 2): Hat is touched with `GestureHolderRow::Surface`. A mouse drag on Kick moves Kick and Snare only. Releasing the surface leaves Hat where it was.
    - `writing_members_write_their_own_lanes`, in an offline pass (the existing pass fixture):
      - Kick is in Touch and Snare in Read;
      - a group drag on Snare writes a `StripFader(Kick)` lane at Stop;
      - Snare's fader moves as a single touch's would;
      - no lane is written for Snare.
    - `surface_gestures_move_the_group`: `gesture_as(.., Surface, false)` fans out as the mouse does.
    - `touched_values_follow_the_followers`: during a drag, `automation_state().touched_values` gives each follower's moved value.
    - `run_group_commands_refuse_hostile_input`: each of these is `Err`, with no panic and no change:
      - an unknown id, `group: u64::MAX`, and a group from another session;
      - `strips: "1,1"`, `strips: "0"`, and 65 IDs;
      - the master in `strips`;
      - a `name` of 70 000 characters, and a `name` with NUL;
      - `colour: "none"`;
      - `mixer.set_group_links` with neither switch.
- [ ] **Step 2: Run** `cargo test -p core-control --test commands` and `cargo test -p core-ffi --test groups`. Expected: FAIL.
- [ ] **Step 3: Implement** the Interfaces block. `commands_for`:
  - reads `strips` with `co_producer::id_list`;
  - defaults `name` to "Group N" and `colour` per Ruling 4;
  - expands `mixer.set_strip`'s `SetGain` through `group_gains`, and `SetMute`/`SetSolo` through `linked`, unless `alone`.

  `run_mixer` names a group or VCA step by `GroupCmd::name` (so `mixer.new_group` is "New Group", not its menu title "Group"), and a fanned-out gain "Move Group ‹name›".

  `MixerCommands.coProducerEdits` and `titles` gain the seven ids. They run as `runMixer`, and nothing opens a sheet.
- [ ] **Step 4: Run** `cargo test -p core-control -p core-ffi`. Expected: PASS, including 8d's `mixer_chores` and the gesture suites.
- [ ] **Step 5: End-of-task checks**, Swift included. **Commit** `feat(mixer): group commands, and group moves from every source`.

### Task 3: VCAs in the model (`core-model`)

**Files:**
- Modify:
  - `crates/core-model/src/groups.rs` (`Vca`, the heard-level arithmetic);
  - `crates/core-model/src/command/groups.rs` (VCA variants);
  - `crates/core-model/src/mixer.rs` (`vcas`, `MixerRepr.vcas`, accessors; `remove` drops VCA membership);
  - `crates/core-model/src/mixer_rules.rs`: `audible` counts VCA solos; `mixer_problems` checks VCAs and their levels through `level`;
  - `crates/core-model/src/file.rs` (the v11 migration doc names `vcas`).
- Test: `crates/core-model/tests/vcas.rs` (new)

**Interfaces:**
- Consumes: Task 1's `check_name`, `MAX_MEMBERS`, IDs and counters.
- Produces:
  ```rust
  pub struct Vca { pub id: VcaId, pub name: String, pub colour: Swatch, pub members: Vector<StripId>,
                   pub gain_db: f64, pub mute: bool, pub solo: bool,
                   #[serde(default, skip_serializing_if = "AutomationMode::is_read")] pub automation: AutomationMode }
  pub const VCA_MODES: [AutomationMode; 4];          // Off, Read, Touch, Latch
  pub fn vcas_of(m: &Mixer, strip: StripId) -> impl Iterator<Item = &Vca> + '_;
  /// Σ of `strip`'s VCAs' levels; GAIN_FLOOR_DB when any is at or below it; 0 for none.
  pub fn vca_offset_db(m: &Mixer, strip: StripId) -> f64;
  /// own + offset, clamped to GAIN_MAX_DB; GAIN_FLOOR_DB when either is at or below it or the sum is.
  pub fn heard_db(own_db: f64, offset_db: f64) -> f64;
  pub fn vca_muted(m: &Mixer, strip: StripId) -> bool;     // any of its VCAs muted
  pub fn vca_soloed(m: &Mixer, strip: StripId) -> bool;    // any of its VCAs soloed
  impl Mixer { pub fn vcas(&self) -> &OrdMap<VcaId, Vca>; pub fn vca(&self, id: VcaId) -> Result<&Vca>; }
  // GroupCmd gains:
  NewVca { name: String, colour: Swatch, members: Vec<StripId> }, RenameVca { vca: VcaId, name: String },
  SetVcaColour { vca: VcaId, colour: Swatch }, AssignVca { vca: VcaId, strips: Vec<StripId> },
  UnassignVca { vca: VcaId, strips: Vec<StripId> }, SetVcaLevel { vca: VcaId, gain_db: f64 },
  SetVcaMute { vca: VcaId, mute: bool }, SetVcaSolo { vca: VcaId, solo: bool },
  SetVcaMode { vca: VcaId, mode: AutomationMode }, DeleteVca { vca: VcaId },
  // names: "New VCA", "Rename VCA", "VCA Colour", "Assign to VCA", "Remove from VCA", "Set VCA", "Mute VCA",
  //        "Solo VCA", "Set VCA Automation Mode", "Delete VCA"
  ```
  `audible` treats a strip as soloed when `s.solo || vca_soloed(m, s.id)`.

- [ ] **Step 1: Write the failing tests** in `tests/vcas.rs`:
  - `heard_level_is_fader_plus_offsets`: a strip at −6 in VCAs at −3 and +2 has `vca_offset_db == -1.0` and `heard_db(-6.0, -1.0) == -7.0`.
  - `a_vca_at_minus_infinity_silences`: a VCA at −144 gives an offset of `GAIN_FLOOR_DB` and a heard level of `GAIN_FLOOR_DB`. `heard_db(-144.0, 6.0)` is the floor too.
  - `heard_total_clamps_at_the_top`: `heard_db(10.0, 6.0) == 12.0`.
  - `vca_mute_and_solo_are_flags`:
    - after `SetVcaMute` and `SetVcaSolo`, every member's `mute` and `solo` are unchanged;
    - `vca_muted` and `vca_soloed` are true;
    - `audible(m)` holds the members and what they reach, and not an unrelated strip.
  - `members_faders_are_never_rewritten` (anti-goal): after each VCA command in turn, every member's `gain_db`, `mute`, `solo` and `automation`, and every automation event, equal the start's.
  - `deleting_a_vca_leaves_members_as_their_faders_say`: after `DeleteVca`, `vca_offset_db` is 0 for every former member, and their strips are unchanged.
  - `a_strip_may_be_in_a_group_and_many_vcas`: one strip in a group and in three VCAs is valid.
  - `vca_modes_are_off_read_touch_latch`: `SetVcaMode` with Write or Trim is `Err` with Ruling 6's message.
  - `hostile_vca_commands_change_nothing`: each of these is `Err`, with the doc unchanged:
    - a level of NaN, +inf, 12.5 or −145;
    - an unknown `VcaId`;
    - the master as a member;
    - 129 members;
    - a duplicate in `strips`;
    - a 70 000-character name.
  - `vcas_save_and_load`: a round trip through `to_json` and `from_json` at version 11. A VCA in Read writes no `automation` key. `validate` finds a VCA with an unknown member and one with level 99.
- [ ] **Step 2: Run** `cargo test -p core-model --test vcas`. Expected: FAIL.
- [ ] **Step 3: Implement** the Interfaces block. VCA commands never touch a `Strip`.
- [ ] **Step 4: Run** `cargo test -p core-model`. Expected: PASS, including `mixer_rules` solo tests.
- [ ] **Step 5: End-of-task checks. Commit** `feat(model): VCAs, their offsets, mute and solo`.

### Task 4: VCA offsets in the engine (`core-engine`)

**Files:**
- Modify:
  - `crates/core-engine/src/graph/mixer.rs` (~174–247): static gains, `muted`, `MixerGraph.vcas`, `StripControlSpec.vcas`;
  - `crates/core-engine/src/graph/controls.rs`: `VcaControlSpec`; `CtlVca`; per-strip VCA links; `commit` sums the offsets; `adopt` and `OverrideSnapshot` carry VCA overrides;
  - wherever `Controls::build` is called with `MixerGraph::controls`, which now passes `MixerGraph::vcas` too (`graph/exec.rs`).
- Test:
  - `crates/core-engine/tests/graph_mixer.rs`, `graph_controls.rs` and `mixer_engine.rs`;
  - `crates/core-engine/tests/bounce.rs`;
  - `crates/core-engine/tests/rt_alloc_graph.rs`;
  - `crates/core-engine/tests/rt_source_scan.rs` (new).

**Interfaces:**
- Consumes: Task 3's `vca_offset_db`, `heard_db`, `vca_muted`, `vcas_of`.
- Produces:
  ```rust
  // controls.rs
  pub struct VcaControlSpec { pub vca: VcaId, pub gain_db: f64, pub mute: bool, pub solo: bool }
  // StripControlSpec: pub vcas: Vec<VcaId>;   MixerGraph: pub vcas: Vec<VcaControlSpec>
  pub(crate) fn build(specs: &[StripControlSpec], vcas: &[VcaControlSpec],
                      input: impl Fn(usize, usize) -> Option<usize>) -> Controls;
  ```
  - **`Controls` gains:**
    - `vcas: Box<[CtlVca]>`, where `CtlVca { vca, base: (db, mute, solo), over_db: Option<f64> }` (`over_db` is set by Task 6);
    - `links: Box<[u16]>`, with each `CtlStrip` owning a `vca_links: Range<usize>` into it.

    Both are sized at build, off the audio thread.
  - **`commit`** gives each strip:
    - `db = heard_db(own, Σ linked VCA dbs)`;
    - `on = kept && !own_mute && !any_vca_mute`;
    - solo is true when `own_solo || any_vca_solo`.

    It writes the result through the existing `strip` and `edge` callbacks.
  - **`mixer_spec`'s static `gain_l`/`gain_r`/`audible`** use the same three rules, so the static path and `commit` agree.

- [ ] **Step 1: Write the failing tests.**
  - `graph_mixer.rs`:
    - `vca_offset_scales_main_output_and_post_sends_not_pre`. A strip at 0 dB in a VCA at −6 has `spec.gain_l` equal to `strip_gains(-6.0, 0.0, true).0`. Its post-fader send's input sits after the strip gain (`Tap::Post`), and its pre-fader send's `Tap::Pre` gain is unchanged.
    - `vca_mute_and_solo_in_the_graph`. A muted VCA gives `audible: false` for its members. A soloed VCA keeps its members' reach audible, and silences the rest.
  - `graph_controls.rs`: `commit_keeps_the_vca_offset`. With a fader override of −3 on a member of a −6 VCA, the strip callback gets `strip_gains(-9.0, ..)`. Without the VCA it gets −3.
  - `mixer_engine.rs`: `a_vca_level_change_ramps_as_a_fader_change_does`. Render a tone through strip A, then apply either `SetGain { A, -6 }` or `SetVcaLevel { vca, -6 }` with A as the only member. The next 2 blocks are equal sample for sample.
  - `bounce.rs`: `live_bounce_and_render_agree_with_a_vca`. The offline bounce, the render and live blocks of the same document with a −6 VCA give equal samples.
  - `rt_alloc_graph.rs`: `vca_graphs_never_allocate`. With 3 VCAs of 40 members each, armed blocks across a graph rebuild with `adopt` and a commit give `rt_check::count()` unchanged.
  - `rt_source_scan.rs`: `controls_take_no_lock`. `include_str!("../src/graph/controls.rs")` contains none of `Mutex`, `RwLock`, `.lock(`, `Vec::new`, `vec!` or `Box::new`, outside `build`, `snapshot`, `adopt_snapshot` and `OverrideSnapshot::controls`, the functions marked "Not RT". The scan splits the file at `fn ` and checks only bodies whose doc comment ends `// RT`.
- [ ] **Step 2: Run** `cargo test -p core-engine --test graph_mixer --test graph_controls --test mixer_engine --test bounce --test rt_alloc_graph --test rt_source_scan`. Expected: FAIL.
- [ ] **Step 3: Implement** the Interfaces block. `heard_db` is the one formula, in `core-model`, and both paths call it.
- [ ] **Step 4: Run** `cargo test -p core-engine`, then `cargo test -p core-engine --features rt-check`. Expected: PASS.
- [ ] **Step 5: End-of-task checks. Commit** `feat(engine): VCA offsets in the mix, the same in live, bounce and render`.

### Task 5: VCA commands and rows (`core-control`, `core-ffi`)

**Files:**
- Modify:
  - `crates/core-control/src/registry.rs`: the seven VCA entries; `GROUP_IDS` reaches 14; `VCA_MODE_NAMES`;
  - the count tests in `crates/core-control/tests/{commands,co_producer}.rs`: 231;
  - `crates/core-ffi/src/mixer_chores.rs` (the VCA arms);
  - `crates/core-ffi/src/mixer.rs`: `VcaRow`; `MixerSnapshot.vcas`; `StripRow.{vcas, vca_offset_db, vca_mark}`; `solo_muted` from Task 3's `audible`;
  - `crates/core-ffi/src/control.rs` (`state.get mixer`'s `vcas`, and `strip_json`'s VCA fields);
  - `app/Sources/VizDAWAppModel/MixerCommands.swift` (the seven ids).
- Test: `crates/core-ffi/tests/vcas.rs` (new), `crates/core-ffi/tests/control.rs`

**Interfaces:**
- Consumes: Task 3's commands and arithmetic; Task 2's `run_mixer` arms and `GroupRow`.
- Produces:
  ```rust
  pub struct VcaRow { pub id: u64, pub name: String, pub colour: SwatchRow, pub members: Vec<u64>,
                      pub gain_db: f64, pub mute: bool, pub solo: bool, pub automation: AutomationModeRow }
  // MixerSnapshot: #[uniffi(default)] pub vcas: Vec<VcaRow>
  // StripRow: #[uniffi(default)] pub vcas: Vec<u64>, #[uniffi(default)] pub vca_offset_db: f64,
  //           #[uniffi(default)] pub vca_mark: bool   // a VCA mutes or solos it
  ```
  `mixer.set_vca` builds `SetVcaLevel`, `SetVcaMute`, `SetVcaSolo` and `SetVcaMode`, in that order, for those given, as one step "Set VCA".

- [ ] **Step 1: Write the failing tests** in `tests/vcas.rs`, on a saved session with Kick, Snare and a bus:
  - `new_vca_from_strips`: `mixer.new_vca {strips: "<k>,<s>"}` is one step "New VCA". `snapshot.vcas[0]` is "VCA 1", at 0 dB, in Read.
  - `set_vca_changes_heard_not_faders`: after `mixer.set_vca {vca, gain_db: "-3"}`, Kick's `gain_db` is unchanged, its `vca_offset_db` is −3.0, and the engine's heard level is fader −3.
  - `vca_mark_lights_when_muting_or_soloing`: `vca_mark` is false, then true after `mute: true`, then false, then true after `solo: true`. The members' own `mute` and `solo` stay false.
  - `deleting_a_vca_is_one_step`: "Delete VCA". Undo restores the VCA and its members.
  - `deleting_a_member_strip_is_one_step_and_undo_restores_membership` (Review Focus 3):
    - Kick is in group "Drums" and is the only member of VCA "Kit";
    - `track.delete` of its track is one undo step;
    - afterwards "Kit" exists with no members, and "Drums" lacks Kick;
    - one undo restores the track, the strip, the group membership and the VCA membership.
  - `run_vca_commands_refuse_hostile_input`: each of these is `Err`, with no change:
    - `vca: u64::MAX`;
    - `gain_db: "1e3"`, `"NaN"`, `"12.5"` and `""`;
    - `mode: "write"` (registry) and `mode: "trim"`;
    - `strips` with the master;
    - `mixer.set_vca` with nothing to set;
    - a 70 000-character name.
  - In `tests/control.rs`: `state_get_mixer_lists_groups_and_vcas`. The JSON has `groups` and `vcas` arrays with Task 2's and this task's fields, and each strip has `group`, `vcas`, `vca_offset_db` and `vca_mark`.
  - In `crates/core-control/tests/commands.rs`: `vca_commands_are_registered`, as Task 2's test is for groups.
- [ ] **Step 2: Run** `cargo test -p core-ffi --test vcas --test control` and `cargo test -p core-control`. Expected: FAIL.
- [ ] **Step 3: Implement** the Interfaces block.
- [ ] **Step 4: Run** `cargo test -p core-ffi -p core-control`. Expected: PASS.
- [ ] **Step 5: End-of-task checks**, Swift included. **Commit** `feat(mixer): VCA commands and the strip's VCA offset`.

### Task 6: VCA automation (model, engine, gestures, Swift arms)

**Files:**
- Modify:
  - `crates/core-model/src/automation.rs`: `AutomationTarget::VcaFader(VcaId)`; `describe` "vca<n> fader"; `target_exists`; `mode_of`;
  - `crates/core-model/src/diff.rs` and `learn.rs`, where their matches are exhaustive;
  - `crates/core-engine/src/schedule/automation.rs` (`AutoDest::Vca(VcaId)`, both directions; it ramps);
  - `crates/core-engine/src/schedule/compile.rs:180`, whose Off check uses `mode_of`;
  - `crates/core-engine/src/graph/controls.rs`: `set_vca`, which is RT; `base_of`, `heard_of` and `release` for `AutoDest::Vca`;
  - `crates/core-engine/src/render.rs` (~1533, ~2609, ~2686): `automate_vca`, and `vca_arrival` per Ruling 7;
  - `crates/core-ffi/src/automation.rs`:
    - `AutomationTargetRow::VcaFader { vca }`;
    - its text, `vca:<id>`, in `automation_target_text` and `parse_automation_target`;
    - `static_command`, which gives `SetVcaLevel`;
    - `static_label` "Set VCA";
    - `lane_name` "VCA ‹name› Fader";
    - `fixed_value`;
  - `crates/core-ffi/src/session.rs` (~8728): `write_mode_of` uses `mode_of`; `check_gesture`; `heard_value`;
  - `crates/core-io/src/dawproject/export_timeline.rs`, which skips `VcaFader` lanes until Task 8;
  - `crates/core-io/src/aaf/export.rs`, which lists them as `OTHER_AUTOMATION`;
  - Swift `switch`es over `AutomationTargetRow`: `app/Sources/VizDAWAppModel/{StripLane,StripChartModel,MixerModel,AutomationText}.swift` and `app/Sources/VizDAWEditors/StripChartView.swift`; plus `MixerModel.heardTargets`, which adds `.vcaFader`.
- Test:
  - `crates/core-engine/tests/automation.rs`;
  - `crates/core-engine/tests/rt_alloc_gestures.rs`;
  - `crates/core-ffi/tests/vcas.rs`;
  - `crates/core-ffi/src/automation.rs` (its unit tests);
  - `app/Tests/VizDAWAppModelTests/AutomationRowsTests.swift`.

**Interfaces:**
- Consumes: Task 4's `CtlVca.over_db`; Task 5's `VcaRow`.
- Produces:
  ```rust
  /// The automation mode governing `t`: its strip's (as `strip_of`), or its VCA's; None for velocity scale.
  pub fn mode_of(doc: &Document, t: AutomationTarget) -> Option<AutomationMode>;
  pub(crate) fn set_vca(&mut self, vca: VcaId, v: f64);     // controls.rs, // RT
  ```
  Swift sees `AutomationTargetRow.vcaFader(vca: UInt64)`.

- [ ] **Step 1: Write the failing tests.**
  - `crates/core-engine/tests/automation.rs`:
    - `vca_lanes_play`: a `VcaFader` lane going from `fader_norm(-144.0)` to `fader_norm(0.0)` takes a member's output from silence to its fader's level. The member's own `StripFader` override stays `None`.
    - `vca_off_mode_plays_static`: a VCA in Off ignores its lane.
  - `rt_alloc_gestures.rs`: `gestures_never_allocate` adds a `VcaFader` lane and every gesture phase on `AutoDest::Vca`. The allocation count is unchanged.
  - `crates/core-ffi/tests/vcas.rs`:
    - `a_vca_gesture_is_one_step`: stopped, touch, values and release on `vca:<id>` give one step "Set VCA", with members' faders unchanged.
    - `vca_touch_writes_its_own_lane`: the VCA is in Touch, and an offline pass writes a `VcaFader` lane. No member lane is written.
    - `vca_gestures_refuse_odd_targets`: `vca:<unknown>` is `Err` "vca<n> fader does not exist".
  - `automation.rs` unit test `vca_target_text_round_trips`: `vca:7` parses and prints back. `vca:`, `vca:-1`, `vca:0x1`, `vca:1:2` and 65 characters each give `None`.
  - Swift: `vcaFaderTargetsAreNamed`. The lane title is "VCA Drums Fader", and a `.vcaFader` heard value is polled while playing.
- [ ] **Step 2: Run** the Rust tests by name, and `scripts/swift.sh test --filter AutomationRows`. Expected: FAIL.
- [ ] **Step 3: Implement** the Interfaces block and the arms listed under Files. `commit`'s VCA db is `over_db.unwrap_or(base.db)`, and `adopt` keeps `over_db` when the VCA's base is unchanged.
- [ ] **Step 4: Run** `cargo test --workspace`, `cargo test -p core-engine --features rt-check` and `scripts/swift.sh test`. Expected: PASS.
- [ ] **Step 5: End-of-task checks. Commit** `feat(automation): VCA faders are an automation target, read and written like a fader`.

### Task 7: Co-Producer rules and previews

**Files:**
- Modify:
  - `crates/core-control/src/co_producer.rs`: `PREVIEWABLE` gains the 14 `GROUP_IDS`; `effective_class` gives Change for `mixer.new_group` and `mixer.new_vca` with `strips`;
  - `crates/core-control/tests/co_producer.rs` (the `PREVIEWABLE` length and class lists);
  - `crates/core-model/src/diff.rs` (group and VCA items; the strip's "heard" part);
  - `crates/core-ffi/src/co_producer.rs`, whose mixer arm already takes `mixer_chores::builds`; `mixer_rows` takes the new items' `co_producers`;
  - `crates/vizdaw-mcp/src/tools.rs`: `strips` is left out of `mixer_new_group` and `mixer_new_vca` (~137); the `change` tool's description gains one sentence;
  - `app/Sources/VizDAWAppModel/CoProducerCommands.swift`, if its mirror lists mixer ids.
- Test: `crates/core-control/tests/co_producer.rs`, `crates/core-model/tests/diff.rs`, `crates/core-ffi/tests/mixer_chores.rs`, `crates/vizdaw-mcp/tests/tools.rs`

**Interfaces:**
- Consumes: Tasks 2 and 5's ids, `commands_for` and rows; 8d's `preview_items`, `describe_with` and `ChangeItem.co_producers`.
- **The items.** `diff::describe` adds them after 8d's strip items:
  - **"Group ‹name›".** Its parts, in this order: `added`, `deleted`, `renamed ‹old› → ‹new›`, `colour ‹old› → ‹new›`, `‹strip› added`, `‹strip› removed`, `mute link on`/`off`, `solo link on`/`off`.
  - **"VCA ‹name›".** Its parts: `added`, `deleted`, `renamed …`, `colour …`, `level −2.0 dB → −5.0 dB` (8d's dB style), `muted`/`unmuted`, `soloed`/`unsoloed`, `mode Read → Touch`, `‹strip› assigned`, `‹strip› unassigned`.
  - **Strips.** A strip whose `heard_db(own, offset)` changed while its own fader did not gains the part `heard −2.0 dB → −5.0 dB`. A change in `vca_muted` gains `muted by a VCA` or `no longer muted by a VCA`.
  - **`co_producers`.** A group or VCA item has `co_producers` true only when every member, before and after, is a strip marked Co-Producer's (Ruling 10). An empty one counts as Co-Producer's.
  - **Solos.** A VCA solo change asks, as every solo change does (8d).

- [ ] **Step 1: Write the failing tests.**
  - `crates/core-control/tests/co_producer.rs`:
    - `empty_groups_and_vcas_are_adds`: `command.run mixer.new_vca {}` is admitted. With `strips` it is `not_allowed` through `command.run`, and a plan holding it is admitted;
    - `group_and_vca_changes_only_through_a_preview`, for each of the 12 change and remove ids.
  - `crates/core-model/tests/diff.rs`: `group_and_vca_items_read_plainly`, with the exact texts above for each part.
  - `crates/core-ffi/tests/mixer_chores.rs`:
    - `the_members_decide_who_says_yes`:
      - **Free:** a VCA of Co-Producer's own bus only. `mixer.set_vca {gain_db: "-3"}` previews `needs_yes: false`.
      - **Asks:** a VCA holding Kick. `mixer.set_vca {gain_db: "-3"}` previews `needs_yes: true`, with "VCA Kit" / "level 0.0 dB → −3.0 dB" and "Strip Kick" / "heard 0.0 dB → −3.0 dB", both `composers_work: true`.
      - **Asks:** `mixer.set_strip` on Co-Producer's bus in a group with Kick. Kick's "volume …" item asks.
      - **Asks:** `mixer.set_group_links` and `mixer.rename_group` on a group with Kick, and `mixer.assign_vca` of Kick.
      - **Asks:** `mixer.delete_vca` of "Kit", with "Strip Kick" / "heard −3.0 dB → 0.0 dB".
    - `group_previews_match_what_the_apply_does`, for each of the 14 ids, as 8d's `mixer_previews_match_what_the_apply_does`.
  - `crates/vizdaw-mcp/tests/tools.rs`: `new_group_and_vca_tools_have_no_strips`.
- [ ] **Step 2: Run** `cargo test -p core-control --test co_producer`, `-p core-model --test diff`, `-p core-ffi --test mixer_chores` and `-p vizdaw-mcp`. Expected: FAIL.
- [ ] **Step 3: Implement** the Interfaces block.
- [ ] **Step 4: Run** the same. Expected: PASS, including 8a–8f's suites.
- [ ] **Step 5: End-of-task checks**, Swift included. **Commit** `feat(co-producer): groups and VCAs, where the members decide who says yes`.

### Task 8: DAWproject and AAF

**Files:**
- Modify:
  - `crates/core-io/src/dawproject/export.rs`: VCA channels in the structure after the buses; `VCA_MEMBERS`, `GROUPS`;
  - `crates/core-io/src/dawproject/export_timeline.rs` (`VcaFader` lanes become the VCA channel's `Volume` automation);
  - `crates/core-io/src/dawproject/import.rs` (~1148, ~1221): a `TrackRole::Vca` becomes `GroupCmd::NewVca` with no members, plus its level, mute and solo; `VCA_NO_MEMBERS` replaces the main import's `VCA` line;
  - `crates/core-io/src/dawproject/import/timeline`: a VCA channel's volume points become a `VcaFader` lane;
  - `crates/core-io/src/aaf/export.rs`: `VCAS`, `GROUPS`;
  - `crates/core-io/tests/data/dawproject/import_6d_golden.json`: `hand_other`'s `ch6` VCA now imports.
- Test: `crates/core-io/tests/dawproject_export.rs`, `crates/core-io/tests/dawproject_import.rs`, `crates/core-io/tests/dawproject_schema.rs`, `crates/core-io/tests/aaf_export.rs` (or the existing AAF export test file)

**Interfaces:**
- **Report texts**, as `pub const` beside the others:
  - `VCA_MEMBERS = "VCA assignments (DAWproject cannot say which channels a VCA controls; members keep their own faders)"`, LeftOut, one per VCA with members, naming it;
  - `GROUPS = "Fader groups"`, LeftOut, one per group (both exports);
  - `VCA_NO_MEMBERS = "VCA channels came in without members (DAWproject does not say which channels a VCA controls)"`, Changed, one per VCA channel, naming it;
  - AAF `VCAS = "VCAs (each track keeps its own fader)"`, LeftOut, one per VCA.
- **The VCA channel's** `Volume` uses the strips' linear unit (`db_of_linear` the other way). The colour goes through `nearest_swatch` on import.

- [ ] **Step 1: Write the failing tests.**
  - `vca_channels_round_trip`: export a song with VCA "Kit" at −6, muted, with fader automation. The file validates against `Project.xsd` (the existing schema check). Its re-import has one VCA "Kit" at −6, muted, with the same points within the existing tolerance, with no members, and with one `VCA_NO_MEMBERS` line.
  - `vca_members_and_groups_are_listed`: the export report has `VCA_MEMBERS` for "Kit", `GROUPS` for "Drums", and the members' channel volumes equal their own faders.
  - `hand_other_imports_its_vca`: the golden file updated. Its "VCA channels" line is gone.
  - `merge_still_leaves_vcas_out`: the 6e merge's lines are unchanged.
  - `aaf_lists_vcas_and_groups`: `VCAS` and `GROUPS` lines. Track volumes are the members' own faders.
- [ ] **Step 2: Run** `cargo test -p core-io dawproject aaf`. Expected: FAIL.
- [ ] **Step 3: Implement** the Interfaces block.
- [ ] **Step 4: Run** `cargo test -p core-io -p core-ffi`. Expected: PASS.
- [ ] **Step 5: End-of-task checks. Commit** `feat(interchange): VCA channels in DAWproject; groups and VCAs reported where they cannot go`.

### Task 9: Swift mixer: groups and VCAs

**Files:**
- Modify:
  - `app/Sources/VizDAWAppModel/MixerModel.swift`: `groups`, `vcas`, `group(selected:)`, `touch(_:alone:)`, the followers' shown values, `vcaOffsetDb`, `vcaMark`, `columns` gains `.vca`;
  - `app/Sources/VizDAWAppModel/KeyBindings.swift` (`KeyDispatch.command` fills `strips`, Ruling 11);
  - `app/Sources/VizDAW/ChannelStrip.swift`:
    - the header's group tag;
    - the "VCA −3.0" readout under the level box;
    - the "VCA" mark in the button row;
    - the fader's `onBegin` reads ⌥;
    - the strip context menu's **Group**, **Assign to VCA ›** and **Remove from VCA ›**;
  - `app/Sources/VizDAW/MixerView.swift` (the VCA columns after the buses, before the master's divider);
  - `app/Sources/VizDAW/AppCommands.swift` (`CommandMenu("Mixer")`, Ruling 12).
- Create: `app/Sources/VizDAW/VcaStrip.swift` (`VcaStripCells`: colour bar and name, member count, fader, level box, M and S, the automation mode menu with `VCA_MODES`)
- Test: `app/Tests/VizDAWAppModelTests/MixerGroupsTests.swift` (new)

**Interfaces:**
- Consumes: Tasks 2, 5 and 6: `GroupRow`, `VcaRow`, `StripRow.{group, vcas, vcaOffsetDb, vcaMark}`, `AutomationStateRow.touchedValues`, `session.gestureAs(target:phase:holder:alone:)`, `.vcaFader(vca:)`, and `runMixer`.
- Produces (`MixerModel`):
  ```swift
  public var groups: [GroupRow] { get }; public var vcas: [VcaRow] { get }
  public func group(_ s: StripRow) -> GroupRow?
  public func groupSelection()                      // mixer.new_group with the selected strips
  public func newVca(fromSelection: Bool)           // mixer.new_vca, with the selection when true
  public func assign(_ strips: [UInt64], toVca vca: UInt64, _ on: Bool)
  public func touch(_ t: AutomationTargetRow, alone: Bool)       // `touch(_:)` is `alone: false`
  public func vcaOffsetDb(_ s: StripRow) -> Double  // Σ of its VCAs' shown values (live/heard/snapshot); floor if any is
  public static func vcaOffsetText(_ db: Double) -> String?      // nil at 0; "VCA −3.0", "VCA +2.0", "VCA −∞"
  public func vcaMark(_ s: StripRow) -> Bool        // snapshot's vcaMark
  ```
  - `MixerColumn` gains `.vca(VcaRow)`.
  - `gainDb(_:)` reads the core's touched value (`touchedValues`) for a target the core holds and this view does not, so followers move during the drag.
  - Every new control has an accessibility label: "Group ‹name›", "VCA offset ‹text›", "Fader of VCA ‹name›", "Mute VCA ‹name›", "Solo VCA ‹name›" and "VCA automation mode". Colours come from theme swatch tokens.

- [ ] **Step 1: Write the failing tests** in `MixerGroupsTests.swift`, on `FakeSession` with a recorded call log:
  - `groupingTheSelectionRunsNewGroup`: with strips 2 and 3 selected, `groupSelection()` runs `mixer.new_group` with `strips: "2,3"`.
  - `optionDragTouchesAlone`: `touch(.stripFader(strip: 2), alone: true)` sends `gestureAs(.., alone: true)`.
  - `followersShowTheCoresValues`: the fake's `touchedValues` for strip 3 moves `gainDb` of strip 3 while strip 2 is touched here.
  - `vcaOffsetReadoutSumsShownValues`: offsets of −3 and +1 read "VCA −2.0"; 0 reads nil; a VCA at −∞ reads "VCA −∞".
  - `vcaColumnsSitBeforeMaster`: `columns` ends with the buses, then the VCAs; the master follows after the divider.
  - `keysFillTheSelection`: `KeyDispatch.command("mixer.new_vca", …)` with strips 2 and 4 selected runs with `strips: "2,4"`. `mixer.rename_vca` gets no filled args.
  - `vcaStripControlsAreLabelled`: `VcaStripCells`' accessibility labels, as above.
- [ ] **Step 2: Run** `scripts/swift.sh test --filter MixerGroups`. Expected: FAIL.
- [ ] **Step 3: Implement** the Interfaces block and the Files list. The Mixer menu's items are disabled as their commands would refuse:
  - **Group** needs ≥ 1 selected non-master strip;
  - **Ungroup** needs the selection to share one group.
- [ ] **Step 4: Run** `scripts/swift.sh test` and `scripts/make-app.sh`. Expected: PASS.
- [ ] **Step 5: End-of-task checks. Commit** `feat(app): group tags, the VCA section and offsets, and the Mixer menu`.

### Task 10: The transport bar as items, on one row with »

**Files:**
- Create:
  - `app/Sources/VizDAWAppModel/TransportItems.swift`: `TransportItemID`, `ReadoutSize`, `TransportItem`, `TransportLayout`, `TransportLayoutStore`, `TransportRowFit`, `TransportMenuEntry`;
  - `app/Sources/VizDAWAppModel/TransportLayoutCommands.swift` (`ui.set_transport_bar`, `ui.reset_transport_bar`);
  - `app/Sources/VizDAWEditors/TransportRowView.swift` (the AppKit row: shown items, the » button, its `NSMenu`);
  - `app/Sources/VizDAW/TransportItemViews.swift` (an `NSView` per `TransportItemID`, hosting SwiftUI where needed);
  - `app/Tests/VizDAWAppModelTests/TransportRowTests.swift`.
- Modify:
  - `app/Sources/VizDAW/DocumentView.swift`: `DocumentTransportBar` hosts the row; delete `TransportBarLayout`;
  - `app/Sources/VizDAW/ArrangementPane.swift`: delete `TransportBar` and `TransportFlowLayout`; the hosts take subsets;
  - `app/Sources/VizDAWAppModel/TransportFields.swift`: delete `TransportBarRows` and `TransportFlow`;
  - `app/Sources/VizDAWEditors/TransportReadoutsView.swift`: `init(document:boxes:size:)`;
  - `app/Sources/VizDAWEditors/TransportFieldsView.swift` (`init(document:fields:)`);
  - `app/Sources/VizDAW/RecordControls.swift`, split into the cluster's Record part, `replaceOverdub` and `recordOptions`;
  - `app/Sources/VizDAWAppModel/DocumentStore.swift` (`transportLayout: TransportLayoutStore`);
  - `app/Sources/VizDAW/VizDAWApp.swift` (created with `MemoryDefaults.appStore()`);
  - `crates/core-control/src/registry.rs`: the two `ui.*` entries; count 233;
  - `app/Tests/VizDAWAppModelTests/TransportFieldsTests.swift:40` and `TransportReadoutsTests.swift:147`, whose two-row tests are replaced by this task's.

**Interfaces:**
- Produces:
  ```swift
  public enum TransportItemID: String, Codable, CaseIterable, Sendable {
      case cluster, loop, replaceOverdub, recordOptions, countIn, summary, bar, time, tempo, signature,
           positionField, editFields, punchFields, dspLoad, device, faults, mainView, panes, space, separator
      public var title: String                   // palette and » titles: "Loop", "Replace/Overdub", "Position in Bars", …
      public var sizes: Bool                     // bar, time, tempo
      public var documentOnly: Bool              // faults, mainView, panes
  }
  public enum ReadoutSize: String, Codable, Sendable { case small, large }
  public enum TransportItem: Codable, Equatable, Sendable { case builtIn(TransportItemID, ReadoutSize?) }  // Task 11 adds .command
  public struct TransportLayout: Codable, Equatable, Sendable {
      public static let version = 1
      public var items: [TransportItem]
      public static let standard: TransportLayout   // Ruling 13's default
      /// `ui.set_transport_bar`'s text: tokens split by ",", each `<id in snake_case>[:large|:small]`;
      /// exactly one `cluster`, at most 64 tokens; the error names the first bad token.
      public static func parse(_ text: String) -> Result<TransportLayout, TransportLayoutError>
  }
  @Observable @MainActor public final class TransportLayoutStore {
      public static let key = "VizDAW.transportBar"
      public init(defaults: UserDefaults)
      public private(set) var layout: TransportLayout
      public func set(_ l: TransportLayout)          // writes the preference
      public func reset()                            // .standard, written
  }
  public enum TransportRowFit {
      public enum Kind: Sendable { case fixed, content, space, separator }
      public struct Piece: Sendable { public var width: CGFloat; public var kind: Kind }
      /// Ruling 14: content folds last first until the shown pieces, their spacing and (when anything folds)
      /// the chevron fit; fixed never folds; spaces and separators are never in `folded`. NaN or ≤ 0 width:
      /// everything but fixed folds; +inf: nothing folds.
      public static func fit(_ pieces: [Piece], width: CGFloat, spacing: CGFloat, chevron: CGFloat)
          -> (shown: [Int], folded: [Int])
  }
  public enum TransportMenuEntry { case action(title: String, isOn: Bool?, enabled: Bool, run: @MainActor () -> Void)
                                   case text(String), popover(title: String) }
  ```
  - **`TransportRowView`.** It lays out on `layout()` with `TransportRowFit`. Folded views are hidden, not removed, so the fields view is never remade. The » is an `AppKitButton` labelled "More Transport Items", which builds its `NSMenu` from the folded items' `TransportMenuEntry`s.
  - **Large readouts** use `theme.type.title`'s digits font.
  - **The right-click menu** holds "Customize Transport Bar…", which Task 11 wires.

- [ ] **Step 1: Write the failing tests** in `TransportRowTests.swift`:
  - `nothingIsCut`. For widths 0 to 2000 in steps of 10, and for `.nan`, `-1` and `.infinity`, on the standard layout's 17 pieces with fixed test widths:
    - `shown ∪ folded` is every content piece, and the two are disjoint;
    - `shown` keeps bar order, and the cluster is in it;
    - for finite widths, `shown` widths plus spacing, plus the chevron when `folded` is non-empty, fit `width`, or `shown` is just the cluster.
  - `foldsTheLastItemFirst`: `[a, cluster, b, c]` folds `c`, then `b`, then `a`, as the width shrinks.
  - `spacesAndSeparatorsAreNeverListed`, and a separator next to » is hidden.
  - `theDefaultLayoutIsTodaysBar`: `standard.items` maps to `[cluster, loop, replaceOverdub, recordOptions, summary, bar, time, tempo, signature, positionField, editFields, punchFields, space, device, faults, mainView, panes]`.
  - `aDamagedPreferenceGivesTheDefaultAndIsKept` (Review Focus 4): with `MemoryDefaults` holding `"{not json"`, then `{"version":99,"items":[]}`, `layout == .standard` and the stored string is unchanged. After `set(...)` it is rewritten.
  - `itemsTextParses`:
    - `"cluster,loop,bar:large"` parses;
    - `"loop"` gives "the bar needs cluster once";
    - `"cluster,cluster"` gives the same error;
    - `"cluster,clik"` gives "unknown item clik";
    - `"cluster,loop:large"` gives "loop has no size";
    - 65 tokens give "at most 64 items".
  - `theMixerWindowRowAt300KeepsEverything` (Review Focus 5): a `TransportRowView` for a mixer window in an `NSWindow` 300 pt wide, on `FakeSession`, with `layoutSubtreeIfNeeded`:
    - every visible item's frame has the same `midY`, and `maxX ≤ 300`;
    - the cluster is visible;
    - the » menu's titles are the folded items' titles, in bar order;
    - no document-only item is in it.

    At 1600 pt, nothing is folded and » is hidden.
  - `largeReadoutsUseTheLargeFont`.
  - `setTransportBarOverTheChannel`: `TransportLayoutCommands.run("ui.set_transport_bar", items: "cluster,loop")` sets the store. A bad text is `invalid_params`, with the error and the store unchanged. `ui.reset_transport_bar` restores `.standard`.
- [ ] **Step 2: Run** `scripts/swift.sh test --filter TransportRow`. Expected: FAIL.
- [ ] **Step 3: Implement** the Interfaces block. Delete the old two-row layouts and their tests. Both windows build the row from `store.transportLayout.layout`. The mixer window passes `documentWindow: false`.
- [ ] **Step 4: Run** `cargo test -p core-control`, `scripts/swift.sh test` and `scripts/make-app.sh`. Expected: PASS.
- [ ] **Step 5: End-of-task checks. Commit** `feat(app): the transport bar as items on one row, with a » overflow`.

### Task 11: The customize sheet and command buttons

**Files:**
- Create:
  - `app/Sources/VizDAWAppModel/TransportCustomize.swift` (`TransportCustomizeModel`, `CommandPicker`);
  - `app/Sources/VizDAWAppModel/CommandButtons.swift` (`CommandAvailability`, `CommandGlyphs`, `CommandButtonAction`, `MenuPerformer`);
  - `app/Sources/VizDAW/TransportCustomizeSheet.swift` (SwiftUI);
  - `app/Tests/VizDAWAppModelTests/TransportCustomizeTests.swift`.
- Modify:
  - `app/Sources/VizDAWAppModel/TransportItems.swift` (`TransportItem.command(id: String, label: String?)`; `parse` takes `command:<id>`);
  - `app/Sources/VizDAW/TransportItemViews.swift` (the command button view);
  - `app/Sources/VizDAW/AppCommands.swift` (View › "Customize Transport Bar…");
  - `app/Sources/VizDAWEditors/TransportRowView.swift` (the right-click item);
  - `crates/core-control/src/registry.rs` (`ui.customize_transport`; count 234; its `DialogSpec`).

**Interfaces:**
- Consumes: Task 10's `TransportLayout`, `TransportLayoutStore`, `TransportRowView`; `KeyBindings.rows`, `KeyDispatch.command`.
- Produces:
  ```swift
  @Observable @MainActor public final class TransportCustomizeModel {
      public init(store: TransportLayoutStore)
      public private(set) var draft: TransportLayout
      public var palette: [TransportItem] { get }     // every built-in not in the draft, then space, separator, a command button
      public func add(_ i: TransportItem, at index: Int)
      public func remove(at index: Int) -> Bool       // false for the cluster
      public func move(from: Int, to: Int)
      public func setSize(at index: Int, _ s: ReadoutSize)
      public func setCommand(at index: Int, id: String, label: String?)  // label trimmed, 1–32 chars; blank → nil (the title)
      public func resetToDefault(); public func apply(); public func cancel()
  }
  public enum CommandPicker { public static func rows(_ all: [BindingRow], query: String) -> [BindingRow] }
      // registry order; case-insensitive on title, menu or id; empty query → all
  public enum CommandAvailability { public static func enabled(_ id: String, doc: DocumentModel?) -> Bool }
      // the family's `enabled` (Transport, Drum, View, Automation, Step, Track, Region); else a document command needs `doc`
  public enum CommandGlyphs { public static func icon(_ id: String) -> AppIcon? }
      // window.mixer → .mixer, editors.piano_roll → .pianoRoll, editors.list → .list, editors.drum_grid → .drumGrid,
      // transport.loop → .loop, transport.record → .record; nil for the rest
  @MainActor public protocol MenuPerformer { func perform(menu: String, title: String) -> Bool }
  public enum CommandButtonAction {
      /// Ruling 16: KeyDispatch.command; on `.pass`, `menu.perform(row.menu, row.title)`; false → status line
      /// "‹title› can't run from a button right now."
      public static func run(_ id: String, doc: (id: UUID, model: DocumentModel)?, store: DocumentStore,
                             host: any ControlHost, menu: any MenuPerformer)
  }
  public static let staleTooltip = "This command is no longer in VizDAW"
  ```
  - **The button.** It shows the label (the command's title by default), with `CommandGlyphs.icon` when there is one. Its tooltip and accessibility label are the command's title. It is disabled when `CommandAvailability.enabled` is false.
  - **A stale ID** (no `BindingRow`) is disabled, shows its stored label (or the ID), and has the tooltip `staleTooltip`. It is never dropped.
  - **The app's `MenuPerformer`** walks `NSApp.mainMenu` for the top-level menu titled `menu`, finds the item titled `title`, and performs it.
  - **The sheet** has a palette and the bar. Items drag in, out and along through `.draggable`/`.dropDestination` with item tokens. Each bar item has the accessibility actions "Remove" (not the cluster), "Move Left" and "Move Right". Each palette item has "Add to Bar". The sheet has **Reset to Default**, **Cancel** and **Done**. A command button opens the picker (a search field and the list) and a label field. Bar, Time and Tempo have a Small/Large picker. Every control has an accessibility label and uses theme tokens.

- [ ] **Step 1: Write the failing tests** in `TransportCustomizeTests.swift`:
  - `customizeCannotRemoveTheCluster`: `remove(at: clusterIndex)` is false, and the draft is unchanged.
  - `customizeOpsKeepOrder`: add, move and remove give the expected `draft.items`. `cancel()` leaves the store as it was, and `apply()` writes it.
  - `paletteOffersWhatIsNotOnTheBar`.
  - `commandPickerFindsByTitleAndMenu`: "mix" finds `window.mixer`, "File" finds `file.save`, and "" gives every row.
  - `commandLabelsAreTrimmedAndBounded`: "  Go  " gives "Go"; 33 characters is refused; blank gives nil.
  - `staleCommandsStayGreyed`: a layout with `command:gone.command` keeps the item through a store reload, and its view model is disabled with `staleTooltip`. `resetToDefault()` then `apply()` removes it.
  - `commandAvailabilityFollowsTheMenus`: `transport.play` is false with no sequence; `window.mixer` is true with a document; any document command is false with `doc: nil`.
  - `aDialogCommandFallsThroughToItsMenuItem`: a fake `MenuPerformer` records `("File", "Bounce…")` for `file.bounce`, which the channel path answers `dialog_required`. `window.mixer` runs without the menu.
  - `customizeTransportOverTheChannelNeedsTheDialog`: `ControlCommands.run("ui.customize_transport")` is `dialog_required`, and no sheet opens.
- [ ] **Step 2: Run** `scripts/swift.sh test --filter TransportCustomize`. Expected: FAIL.
- [ ] **Step 3: Implement** the Interfaces block. The sheet is presented on the window whose bar or View menu asked (`TransportLayoutStore.customizing: UUID?`, the window token).
- [ ] **Step 4: Run** `cargo test -p core-control`, `scripts/swift.sh test` and `scripts/make-app.sh`. Expected: PASS.
- [ ] **Step 5: End-of-task checks. Commit** `feat(app): customize the transport bar, with command buttons`.

### Task 12: README manual checks, e2e and backlog

**Files:** `app/README.md` (a new section "### Fader groups, VCAs and the transport bar (plan groups-vcas-transport)" after the last section), `crates/vizctl/tests/e2e.rs`, `docs/feature-backlog.md`

- [ ] **Step 1: Add the README manual checks**, each one line with its expected result:
  1. Group three strips; drag one; all move by the same dB; ⌘Z undoes "Move Group ‹name›" in one step.
  2. ⌥-drag moves one fader alone.
  3. Push a group up until a member hits +12: the whole move stops; drag back: the balance returns.
  4. Mute and solo on one member follow the group's switches; turn a switch off and they don't.
  5. Make a VCA from two strips and pull it to −6: the faders don't move, each strip shows "VCA −6.0", and the sound drops.
  6. Mute and solo the VCA: the members' own buttons stay off and their "VCA" mark lights.
  7. Automate the VCA in Touch, then play it back in Read.
  8. Delete a member's track, then undo: the membership returns.
  9. Export a DAWproject with a VCA and a group and read the report lines; import it back.
  10. Customize the transport bar: drag items in, out and along; add a Command button for Mixer; click it; Reset to Default.
  11. Add a Command button for Bounce…: clicking it opens the Bounce sheet.
  12. Narrow the mixer window to its smallest: the bar stays one row, and » lists the rest.
  13. Quit and reopen: the bar is the same in both windows.
- [ ] **Step 2: Add the e2e case** `groups_vcas_and_transport_bar_over_the_channel`:
  - two strips; `mixer.new_group`, then `mixer.set_strip {gain_db: "-3"}`;
  - `state.get mixer` shows both moved, and the undo title is "Move Group Group 1";
  - `mixer.new_vca` and `mixer.set_vca {gain_db: "-6"}` give `vca_offset_db: -6` and unchanged `gain_db`;
  - `ui.set_transport_bar {items: "cluster,command:window.mixer"}`, then `ui.click` on the transport bar's "Mixer" element opens a mixer window (the window count rises);
  - `ui.reset_transport_bar`.
- [ ] **Step 3: Update the backlog.**
  - Mark Done: "Linked fader groups and VCAs", "Customize the transport bar" (its Record ⌄ rethink stays as a row of its own), and "The mixer window's transport bar at narrow widths".
  - In "DAWproject: send pan, VCA channels and folder tracks", VCA channels are now done, with Ruling 8's limit; send pan and folders stay.
  - Remove "linked fader groups and VCAs" and "a customisable transport bar" from the 27.1.0 list (line 20).
  - Add the rows in "Not in this plan".
- [ ] **Step 4: Confirm.** Run the full verification:
  - `cargo test --workspace`;
  - Linux clippy;
  - `scripts/swift.sh test`;
  - `scripts/make-app.sh`;
  - the no-brand build (`VIZDAW_NO_BRAND=1` with a fresh `--scratch-path`);
  - the e2e, as `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`.

  Report against the baseline:
  - registry +17 (217 → 234);
  - `PREVIEWABLE` +14;
  - VERSION 10 → 11;
  - README +13;
  - e2e +1;
  - no change under `crates/core-host` or `crates/platform-macos`.
- [ ] **Step 5: Commit** `docs: manual checks, e2e and backlog for groups, VCAs and the transport bar`.
