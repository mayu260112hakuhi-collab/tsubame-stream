# Phase 8B-Perf.3 — 実測パフォーマンス表示

Perf.2.1 適用後の `apps/tsubame/src/app.rs` を対象にした差分です。

## 追加表示

下部ステータス欄に約1秒窓の実測値を追加します。

- `Capture`: WGC callback の既存実測FPS
- `Preview`: Deferred Preview viewport の実描画周期
- `Mixer UI`: Deferred Audio Mixer viewport の実描画周期
- `Output`: 配信中はStreamingへ渡したフレームの実測FPS。録画のみの場合は録画エンコードフレーム差分から算出
- `Drop`: 既存の preview worker dropped jobs 累計

CPU / RAM 表示は従来どおりです。

## 実装メモ

- Preview / Mixer / Streaming Output は `AtomicU64` カウンタで低コストに計数します。
- 約1秒ごとに前回値との差分 / 実経過秒でFPSを算出します。
- 設定FPSではなく実測値です。
- 配信と録画を同時に行っている場合、`Output` は配信側の送出FPSを表示します。

## 確認コマンド

```powershell
cargo test -p tsubame-stream measured_fps_uses_actual_elapsed_time
cargo test -p tsubame-stream runtime_frame_counters_track_each_pipeline_independently
cargo test --workspace
cargo build --release -p tsubame-stream
```

ビルド後、Preview / Mixer をそれぞれON/OFFして表示値が追従することを確認してください。
