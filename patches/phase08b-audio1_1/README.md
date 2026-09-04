# Phase 8B-Audio.1.1 — BGM controls persistence fix

## Symptom
BGM continued playing, but after stopping a stream the BGM transport controls could disappear
when the currently selected layer was no longer the BGM layer.

## Root cause
The BGM transport section was rendered only when `selected_layer` resolved to an audio/BGM layer.
Playback state and transport visibility were incorrectly coupled to layer selection.

## Fix
- Prefer the selected BGM when one is selected.
- Otherwise, fall back to a currently playing BGM.
- Keep playback itself independent from stream start/stop.
- Add a regression test for the selected-vs-active fallback.

No streaming, PCM, mixer, or audio-processing behavior is changed.
