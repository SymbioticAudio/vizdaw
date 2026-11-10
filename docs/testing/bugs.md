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

