# Phase 8B-Perf.4 — Mixer meter cap + Encode In / Target metrics

Base: Phase 8B-Perf.3 applied and building.

## Changes

- Separates mixer viewport render FPS from actual audio meter refresh FPS.
- Caps audio meter sampling/refresh work to about 30 Hz (33 ms minimum interval).
- Keeps immediate UI interaction/rendering intact even if the viewport repaints faster.
- Renames the old runtime `Output` metric semantics to `Encode In`.
- Shows `Target <n> FPS` separately from measured encoder input FPS.
- Keeps Capture / Preview / Drop metrics.

Expected status text example:

```text
CPU 3.1% | RAM 325 MB | Capture 24.2 FPS | Preview 0.0 FPS | Mixer Render 58.1 FPS | Meter 29.7 FPS | Encode In 23.6 FPS | Target 60 FPS | Drop 0
```

`Mixer Render` may exceed 30 FPS because egui/OS can repaint the viewport for other reasons. `Meter` is the expensive audio-level sampling/update path and is capped independently.

## Test / build on Windows

```powershell
cargo test -p tsubame-stream mixer_meter_refresh_is_capped_at_about_30hz
cargo test -p tsubame-stream performance_labels_distinguish_encode_input_from_target_fps
cargo test --workspace
cargo build --release -p tsubame-stream
```

## Runtime check

1. Open mixer only and confirm `Meter` stays around <= 30 FPS even if `Mixer Render` is higher.
2. Start recording and confirm `Encode In` follows real frames submitted to the encoder.
3. Confirm `Target` remains the configured target FPS (e.g. 60 FPS).
4. Confirm fader/mute controls still react immediately.

Note: This patch does not add frame duplication/output pacing. Therefore it intentionally does not claim a measured 60 FPS output timeline when capture/encoder input is lower.
