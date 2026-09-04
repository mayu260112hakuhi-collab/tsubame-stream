# Phase 8B-Audio.2 — BGM individual WAV + recording mux

## What changes

- External/BGM PCM buses can be subscribed to by the recording pipeline.
- Recording starts a synchronized WAV writer for each external PCM channel that exists at recording start.
- Missing BGM packets are padded with 20 ms silence so a BGM started later in the recording remains aligned to the recording timeline.
- BGM tracks are added to the existing routed final MP4 mux.
- `Mix` controls whether the BGM track is included in the final MP4 audio mix.
- `WAV` controls whether the BGM individual WAV and its `.audio.txt` diagnostic sidecar are kept after finalization.
- The BGM mixer strip now exposes its WAV checkbox.

## Files

- `apps/tsubame/src/app.rs`
- `crates/stream-audio/src/lib.rs`
- `crates/stream-recording/src/lib.rs`

## Suggested Windows verification

```powershell
cargo test -p stream-audio external_pcm_recording_source_subscribes_to_same_raw_bus
cargo test -p stream-recording external_audio_path_is_stable_and_distinct
cargo test --workspace
cargo build --release -p tsubame-stream
```

### Functional recording test

1. Add one BGM layer.
2. Turn BGM `Mix` ON and `WAV` ON.
3. Start recording while BGM is stopped.
4. Wait about 5 seconds, then start BGM.
5. Stop recording after another 10–15 seconds.
6. Confirm the final MP4 has about 5 seconds of silence before the BGM begins.
7. Confirm an `external_<id>_bgm_*.wav` file remains in the recording folder.
8. Confirm its matching `external_<id>_bgm_*.audio.txt` reports 48000 Hz, 2 channels, PCM16.
9. Repeat with BGM `WAV` OFF; final MP4 should still contain BGM when `Mix` is ON, but the individual external WAV/sidecar should be removed after finalization.
