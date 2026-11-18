# Fader Groups, VCAs and a Customizable Transport Bar: Design

Owner conversation, 2026-11-18. These go into 26.0.0, a beta the owner will share with friends and test. 26.1.0 is the official release.

## Why

The owner wants two standard mixing tools:
- riding a whole section, such as all the drums, as one;
- keeping a set of parts, such as backing vocals, in balance while moving them together.

They also want a transport bar they can shape to how they work, including buttons for the commands they use all the time, such as opening the mixer.

Both items are backlog rows: "Linked fader groups and VCAs" and "Customize the transport bar". The latter covers "The mixer window's transport bar at narrow widths" too.

## Goals

- Linked fader groups that move their members by the same dB.
- VCA faders that offset their members' levels without moving the members' faders.
- A transport bar the user can rearrange, trim and extend, with command shortcut buttons.

## Non-goals

- Nested VCAs (a VCA assigned to a VCA).
- Group "relative or absolute" modes beyond the one below.
- Spill (showing only a VCA's members). Both can come later.
- Per-project transport layouts.

## Anti-goals

- A group or VCA that quietly changes the user's balance. Every move must be undoable as one step, and a VCA must never rewrite its members' fader values or automation.
- A transport bar that loses an item without the user seeing where it went.

## 1. Linked fader groups (owner-approved)

- **Making one.** Select some strips, then choose Mixer › Group. A group has a name and a colour, shown as a small tag on each member strip. A strip belongs to at most one group (my call; the usual rule, and it keeps moves unambiguous).
- **Moving.** Moving any member's fader moves every member by the same number of dB.
  - A member at −∞ stays at −∞.
  - If a member would pass the fader's top (`GAIN_MAX_DB`), the whole move stops there, so the balance holds (my call).
  - Holding ⌥ while dragging moves that one fader alone.
- **What links.** The group has switches for **Mute** and **Solo**, both on by default (my call). When they're on, pressing mute or solo on one member does it for all.
- **Undo.** A group move is one undo step: "Move Group Drums".
- **Every source.** The rule applies wherever a fader move comes from: the mouse, a control surface, the control channel, automation writing in Touch or Latch, and Co-Producer. So it belongs in the core command (`mixer.set_strip` with gain), not in the Swift view. The command takes `alone: true` for the ⌥ case.
- **Automation.** Writing a grouped fader writes each member's own gain lane, and each member's existing automation reads as before. A group has no lane of its own.

## 2. VCAs (owner-approved)

- **What it is.** A VCA strip carries no audio. It has a fader, mute and solo, a name and a colour. VCAs sit in their own section of the mixer, to the right of the buses and before the master.
- **Assigning.** Select strips, then choose Mixer › Assign to VCA ›, or use the VCA picker on a strip. A strip may belong to several VCAs.
- **Level.** A strip's heard level is its own fader plus the sum of its VCAs' offsets, in dB. A VCA at 0 dB changes nothing, and a VCA at −∞ silences its members.
  - The members' faders and automation are never rewritten.
  - The strip shows a small "VCA −3.0" offset readout so the user can see why it sounds quieter.
- **Mute and solo.** A VCA's mute mutes its members. Its solo solos its members.
- **Automation.** A VCA fader can be automated. It's a new automation target, read and written like a strip's gain.
- **Engine.** The engine applies the summed offset smoothly, with the same ramping as fader moves. It's computed in the engine's mix, not by moving faders.
- **Removing.** Deleting a VCA leaves its members exactly as their faders say. That is a change in what's heard, so it asks when Co-Producer does it (§4).

## 2a. Settled after the builder check (my calls)

- **Members.** Track strips and buses can be members. The master and VCAs can't.
  - A strip may be in one group and in any number of VCAs at once.
  - Deleting a member strip removes it from its group and VCAs in the same undo step.
  - A group or VCA left with one member, or none, stays until the user deletes it.
- **Group moves and automation.** A group move is an ordinary fader move on every member, in one undo step.
  - A member in a writing mode (Touch or Latch) writes its own gain lane.
  - A member in Read moves its fader exactly as a single fader move would, and playback reads its automation as usual.
  - The ceiling rule counts every member, whatever its mode.
- **Floor.** On a downward move, the whole move stops when any finite member would reach the fader's lowest finite step. Members don't fall to −∞ one by one, so the drag is reversible and the balance holds. A member already at −∞ stays there.
- **What a VCA scales.** A VCA acts as part of the member's fader. It scales the main output and the post-fader sends. It does not scale pre-fader sends or pre-fader sidechain taps, which is the usual rule.
  - A VCA fader has the same range as a strip fader.
  - The heard total, the member's fader plus its VCA offsets, is clamped to `GAIN_MAX_DB`.
  - The live engine, offline bounce and render all use the same sum.
- **VCA mute and solo.** These are flags of the VCA, never written into the members.
  - A member is heard muted if its own mute or any of its VCAs' mutes is on.
  - A VCA's solo counts as a solo of each member, for the existing solo logic. The member's own solo button stays as it is.
  - A member shows a small "VCA" mark lit when a VCA is muting or soloing it.
  - A group's linked solo still works through the members' own solo flags.
- **Transport items.** Play, Stop, Record, Rewind, Fast Forward and Return to Start form one fixed cluster. The user can't remove it, and it never folds away. The document window's summary chip is an ordinary item.
- **Stale layouts.** The layout stores command IDs. A saved button whose command no longer exists stays in place, greyed out, with the tooltip "This command is no longer in VizDAW". It is never removed silently. Reset to Default clears it.

## 3. Saved, undone, driven

- Groups and VCAs are saved in the project, with the mixer.
- The builder decides with the existing format rules whether this needs a format VERSION bump, so that an older build says "made by a newer VizDAW" rather than "Corrupt".
- Every action is an undoable command in the registry, with hostile-input tests: create, rename, colour, assign and unassign, delete, set the VCA level, mute and solo, and link switches. All of them are reachable from the control channel and from key bindings.
- **Interchange:**
  - DAWproject export and import carry VCAs as VCA channels. This closes part of "DAWproject: send pan, VCA channels and folder tracks".
  - Groups have no DAWproject or AAF counterpart. They are dropped on export with a report line.
  - AAF drops VCAs with a report line too.

## 4. Co-Producer (my calls, following the existing rules)

- Creating an empty group or VCA is an add.
- Each of the following changes the composer's sound or routing, so it asks:
  - assigning the composer's strips to a group or VCA;
  - moving a VCA or group that controls any of the composer's strips;
  - deleting a VCA that controls any of the composer's strips;
  - flipping a link switch on a group that holds any of the composer's strips.
- Moves on groups and VCAs made only of Co-Producer's own strips are free, under the "strip decides" rule.
- The previews show the per-strip dB change.

## 5. Customizable transport bar (owner-approved, plus shortcut buttons)

- **Customizing.** Right-click the transport bar, or choose View › Customize Transport Bar…, to open a sheet in the style of a Mac toolbar. The user drags items in, out, and into a new order. **Reset to Default** restores today's bar.
- **Items:**
  - every button and readout the bar has today: Count-in, Click, Loop, the Record options and Replace/Overdub; position in bars, position in time, tempo, meter, the punch and edit fields, DSP load, and the audio device;
  - a flexible space and a separator.
  - Readouts with room for a big font, starting with position and tempo, offer **Small** and **Large**.
- **Command buttons.** A **Command button** item picks any registry command, the same list key bindings use: Show Mixer, Show Piano Roll, Bounce…, Save, a Co-Producer recipe.
  - The button shows a short label (the command's title by default, editable) and an icon from the command's menu glyph when it has one.
  - Its tooltip and accessibility label are the command's name.
  - It greys out when the command can't run, as the menu item would.
  - A command that opens a dialog opens it: that's a user click, not a programmatic call.
- **Storage.** The layout is saved per user, in preferences, not per project. The document window and the mixer window share it.
- **Room (owner, 2026-11-18): one row, always.** The transport bar never wraps onto a second row, in the document window or the mixer window. This replaces today's two-row wrap (`TransportBarLayout` and `TransportFlowLayout`). When items don't fit, they fold into a » (double-chevron) overflow menu at the right end, last item first, so nothing vanishes. The fixed play cluster never folds. The existing "never cut at default or minimum" layout tests become "nothing is cut: every item is either shown or listed in »".

## Left to the builder

- Widget details within the theme tokens.
- How the core stores groups and VCAs (IDs, maps).
- The exact VCA-section layout.
- Which glyphs the command buttons get.
- Whether the customize sheet is SwiftUI or AppKit (it must follow Swift 6.1 rules and carry accessibility labels).
- How the engine sums offsets, as long as it adds no allocation or lock on the audio thread.
