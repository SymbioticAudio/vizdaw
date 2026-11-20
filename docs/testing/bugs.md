# Bug log

One file per testing session's findings. Dictated or rough notes are fine:
Claude tidies each entry into the shape below when it triages them.

## How it works

1. Test one build: `scripts/install-test-app.sh` puts the current
   `dist/VizDAW.app` at `~/Applications/VizDAW Test.app`, which nothing else
   rebuilds. About VizDAW shows its build, e.g. `0.1.0 (23b48c6 · 2026-11-05 18:49)`.
2. Note the build once at the top of your session, then log what you find.
   Screenshots go in `docs/testing/shots/` (not committed), named after the
   bug if you can (`B-012.png`), otherwise anything.
3. Tell Claude "bugs ready". Claude numbers and tidies the entries, finds
   the cause of each, writes findings and questions into the entry, and
   moves its status along: open → investigating → fixed (commit) → verify.
4. Retest the "verify" ones on the next test build and mark them closed.

## Entry shape

```
## B-000 Short title
- Build: 23b48c6
- Where: view › pane, theme
- Steps: 1. … 2. … 3. …
- Expected: …
- Actual: …
- Screenshot: shots/B-000.png
- Status: open
- Notes: (Claude's findings and questions)
```

---

# Session: (date, build)


# Session: 2026-11-19, build dd36914d (26.0.0), README checks 439–456 run by Claude through the control channel

## B-001 A VCA's automation track exports to DAWproject as a synth track
- Build: dd36914d
- Where: File › Export DAWproject, a project with a VCA that has automation
- Steps: 1. Make a VCA from two strips. 2. Set it to Touch, play and move
  its level (this makes the track "VCA VCA 1 Fader"). 3. Export a
  DAWproject. 4. Import it as a new project.
- Expected: the VCA's moves go out as the VCA channel's automation; no
  extra track.
- Actual: the export counts the VCA's track as a sixth track and reports
  "Track … plays the built-in synth"; on import, "VCA VCA 1 Fader" comes
  back as an instrument track on Built-in Synth with no events. The VCA
  itself comes back with level, mute and solo (check 447 holds).
- Status: backlog (owner, 2026-11-19; docs/feature-backlog.md)
- Notes: The report said "7 automation points" were written, so the moves
  may also reach the VCA channel; not yet traced. The track name repeats
  "VCA" ("VCA VCA 1 Fader", `StripLane.swift:172`) when the VCA keeps its
  default name; the strip's accessibility labels do the same ("Fader of VCA
  VCA 1").
