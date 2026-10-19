# Feature backlog (pre-release candidates)

Ideas to consider before release. Not scheduled into a sub-project yet; each
gets its own brainstorm → spec → plan when picked up.

| Feature | Notes |
|---|---|
| Fugue sequencing | Generate or assist imitative counterpoint (subject, answers, entries at intervals/delays). |
| Euclidean sequencing | Distribute k hits over n steps (with rotation) per lane/track. |
| Aleatoric sequencing | Chance-driven sequencing: probabilities, random choice among events, constrained randomness. |
| Audio (generative / sequencing for audio) | Clarify scope: apply the sequencing modes above to audio regions/slices? |
| Live looping (Boomerang-style) | Real-time audio looper: record, overdub, reverse, once, stack; sync to tempo or free. |
| Built-in EQ and dynamics from ZL Audio's open-source plugins (https://zl-audio.github.io/plugins/) | Candidates for the channel strip's EQ and dynamics, e.g. their equalizer and compressor. Verify each plugin's licence before reusing code (believed AGPL-3.0; that combines with our GPL-3.0 under GPL §13, but confirm). Using them as bundled CLAP/VST3 plugins avoids merging code. Fits the colour-coded EQ in the SP7 mixer refinements. |
| Built-in instruments/effects from Mutable Instruments' open-source DSP | Port the open-source Eurorack DSP code (e.g. macro-oscillator, granular texture, resonator, modal and string models) as built-in or CLAP plugins. Check each module's licence (most DSP is MIT; some files differ) against GPL-3.0 and keep notices. Use our own names and UI, not the Mutable Instruments trademarks or panel art. |
