# Phase 8B-Perf.2.1 — BGM snapshot double-reference compile fix

Perf.2 introduced a compile error at `apps/tsubame/src/app.rs:1110`.

`bgm_mixer_snapshot` is already `&Vec<(...)>`:

```rust
let bgm_mixer_snapshot = &snapshot.bgm_rows;
```

The loop incorrectly added another reference (`&&Vec`):

```rust
for (...) in &bgm_mixer_snapshot {
```

This patch removes only that extra `&`:

```rust
for (...) in bgm_mixer_snapshot {
```

No audio, mixer, viewport, or persistence behavior is otherwise changed.
