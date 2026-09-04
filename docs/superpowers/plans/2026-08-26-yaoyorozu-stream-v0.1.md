# Yaoyorozu Stream v0.1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** YouTube専用の軽量Rust配信アプリを、分離収録とAviUtl2 Bridgeへ拡張できる土台から段階実装する。

**Architecture:** Cargo workspaceで責務を分離し、UIと重い処理を非同期メッセージで隔離する。Phase 1では共通セッションモデル、配信プリセット、編集マーカー、状態メッセージ、D案UIの起動骨格を完成させる。

**Tech Stack:** Rust 2021, Cargo workspace, serde/serde_json, crossbeam-channel, eframe/egui（Windows UI実装時）, Windows capture/audio APIs, YouTube Data/Live Streaming APIs, H.264 hardware encoders, Windows Named Pipe, AviUtl2 Bridge Plugin.

**Spec:** `docs/superpowers/specs/2026-08-26-tsubame-v0.1-design.md`

## Global Constraints

- YouTube専用。
- UIスレッドはキャプチャ・エンコード・ネットワークI/Oを同期的に待たない。
- キューは境界付きとし、遅延時は全体停止よりフレームドロップを優先する。
- 標準録画形式はMP4、映像H.264、配信用音声AAC。
- v0.1音声はマイク、PC音声、配信用ミックスの3系統。
- ゲーム標準1080p60、作業標準1080p30、軽量720p。
- GPUエンコードはNVENC / AMF / Quick Syncを自動検出し、CPUへフォールバックする。
- AviUtl2連携はJSON + Windows Named Pipe + Bridge Plugin。
- AviUtl2やYouTubeの障害を配信UI・ローカル録画へ波及させない。

---

### Task 1: Core session model and presets

**Files:**
- Create: `Cargo.toml`
- Create: `crates/stream-core/Cargo.toml`
- Create: `crates/stream-core/src/lib.rs`
- Create: `crates/stream-core/tests/session.rs`

**Interfaces:**
- Produces: `StreamPreset`, `SessionConfig`, `SessionId`, `StreamState`.

- [ ] **Step 1: Write failing tests** for Game=1920x1080@60, Work=1920x1080@30, Light=1280x720@30 and session defaults.
- [ ] **Step 2: Run** `cargo test -p stream-core` and verify RED because the types do not exist.
- [ ] **Step 3: Implement** serializable preset/session types and constructors.
- [ ] **Step 4: Run** `cargo test -p stream-core` and verify GREEN.
- [ ] **Step 5: Commit** `feat(core): add stream session model`.

### Task 2: Edit markers and stable JSON schema

**Files:**
- Modify: `crates/stream-core/src/lib.rs`
- Create: `crates/stream-core/src/marker.rs`
- Create: `crates/stream-core/tests/edit_json.rs`

**Interfaces:**
- Produces: `MarkerKind::{Cut,Short,Chapter,Note}`, `EditMarker`, `EditManifest::to_json_pretty()`.

- [ ] **Step 1: Write failing tests** asserting marker millisecond timestamps and `format="tsubame_stream_edit"`, `version=1`.
- [ ] **Step 2: Run** `cargo test -p stream-core --test edit_json` and verify RED.
- [ ] **Step 3: Implement** marker and manifest serialization using relative media paths.
- [ ] **Step 4: Run** the test and full `cargo test -p stream-core`; verify GREEN.
- [ ] **Step 5: Commit** `feat(core): add AviUtl2 edit manifest`.

### Task 3: Bounded worker messaging

**Files:**
- Create: `crates/stream-core/src/message.rs`
- Create: `crates/stream-core/tests/message.rs`

**Interfaces:**
- Produces: `WorkerCommand`, `WorkerEvent`, `WorkerStatus`, `bounded_worker_channel(capacity)`.

- [ ] **Step 1: Write failing tests** proving a capacity-1 queue rejects a second non-blocking send instead of blocking.
- [ ] **Step 2: Run** `cargo test -p stream-core --test message` and verify RED.
- [ ] **Step 3: Implement** bounded crossbeam channels and status/event enums.
- [ ] **Step 4: Run** full core tests and verify GREEN.
- [ ] **Step 5: Commit** `feat(core): add bounded worker messaging`.

### Task 4: D-layout application shell

**Files:**
- Create: `apps/tsubame/Cargo.toml`
- Create: `apps/tsubame/src/main.rs`
- Create: `apps/tsubame/src/app.rs`
- Create: `apps/tsubame/src/view_model.rs`
- Create: `apps/tsubame/tests/view_model.rs`

**Interfaces:**
- Consumes: core session, marker and worker status types.
- Produces: `StreamViewModel` with preview state, YouTube panel state, audio meters, marker actions and post-stream AviUtl2 action.

- [ ] **Step 1: Write failing view-model tests** for default Game preset, four marker actions, and AviUtl2 action visibility only after stop.
- [ ] **Step 2: Run** `cargo test -p tsubame --test view_model` and verify RED.
- [ ] **Step 3: Implement** the view model without capture/network side effects.
- [ ] **Step 4: Implement** egui D-layout: large preview left, YouTube controls right, audio and markers below.
- [ ] **Step 5: Run** workspace tests and `cargo check --workspace`; verify GREEN.
- [ ] **Step 6: Commit** `feat(ui): add compact streaming shell`.

### Task 5: Windows capture worker

**Files:**
- Create: `crates/stream-capture/`
- Test: capture source enumeration and bounded frame delivery behind a platform adapter.

- [ ] RED test source selection and overflow behavior.
- [ ] Implement Windows screen/window source abstraction.
- [ ] Keep capture off UI thread and drop stale frames when queue is full.
- [ ] Run crate/workspace tests.
- [ ] Commit `feat(capture): add Windows capture worker`.

### Task 6: Three-path audio worker

**Files:**
- Create: `crates/stream-audio/`

- [ ] RED tests for mic/desktop/mix routing and shared timestamps.
- [ ] Implement microphone and desktop capture adapters plus stream mix.
- [ ] Keep bounded audio buffers and report underrun/overrun status.
- [ ] Run tests.
- [ ] Commit `feat(audio): add three-path audio pipeline`.

### Task 7: Encoder selection and H.264

**Files:**
- Create: `crates/stream-encoder/`

- [ ] RED tests for preference order NVENC → AMF → Quick Sync → CPU.
- [ ] Implement capability detection interface.
- [ ] Implement H.264 encoder adapter boundary.
- [ ] Verify preset parameters for 1080p60/30 and 720p.
- [ ] Commit `feat(encoder): add hardware encoder selection`.

### Task 8: MP4 and separated audio recording

**Files:**
- Create: `crates/stream-recording/`

- [ ] RED tests for session paths and monotonic common timestamps.
- [ ] Implement `recording.mp4`, `microphone.wav`, `desktop.wav`.
- [ ] Persist session/edit metadata during recording.
- [ ] Verify graceful finalize behavior.
- [ ] Commit `feat(recording): add separated session recording`.

### Task 9: YouTube account and live management

**Files:**
- Create: `crates/stream-youtube/`

- [ ] RED tests around an HTTP/API boundary for OAuth state, new-live request mapping and scheduled-live selection.
- [ ] Implement Google OAuth token lifecycle without storing secrets in source.
- [ ] Implement channel lookup, new broadcast creation and scheduled broadcast listing.
- [ ] Implement streaming worker isolated from recording/UI.
- [ ] Commit `feat(youtube): add YouTube Live integration`.

### Task 10: AviUtl2 IPC bridge

**Files:**
- Create: `crates/stream-aviutl-bridge/`
- Create: `aviutl2-plugin/`

- [ ] RED tests for Named Pipe message framing and JSON fallback.
- [ ] Implement local Named Pipe client with bounded timeout/retry.
- [ ] Implement Bridge plugin adapter that maps video/mic/desktop/markers to Layers 1–4 through AviUtl2 SDK.
- [ ] Ensure Bridge absence never blocks recording or UI.
- [ ] Commit `feat(aviutl): add edit bridge`.

### Task 11: Fault isolation and soak tests

**Files:**
- Create: `tests/fault_isolation.rs`
- Create: `tests/soak.rs`

- [ ] Inject stalled YouTube worker and verify UI/status and recording continue.
- [ ] Inject absent AviUtl2 and verify JSON remains valid.
- [ ] Fill capture queue and verify frame drop instead of producer deadlock.
- [ ] Run a bounded-duration soak test and assert no unbounded queue growth.
- [ ] Commit `test: add streaming fault isolation coverage`.

## Phase 1 deliverable

Tasks 1–4 produce the first runnable artifact: a Rust workspace with tested core models and the compact D-layout application shell. Tasks 5–11 add real capture, audio, encoding, recording, YouTube and AviUtl2 integration without changing the core isolation model.
