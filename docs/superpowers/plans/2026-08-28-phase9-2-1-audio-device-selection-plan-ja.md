# Phase 9.2.1 音声デバイス選択 実装計画

**目的:** PC音声とマイクについて、Windows音声デバイスの列挙・選択・再接続を追加する。

**構成:** `stream-audio` にデバイス型・列挙・選択状態・再接続制御を追加し、`yaoyorozu-stream` のegui側ではComboBoxで選択する。既存のGain/Mute/Master、原音WAV、GPU録画、Fix12プレビュー、AviUtl2連携は維持する。

**参照設計書:** `2026-08-27-phase9-2-audio-sources-discord-design-ja.md`

## 全体制約
- Windows既定デバイスを残す
- 入力/出力デバイスを個別選択
- 切替時にUIをブロックしない
- 対象チャンネルだけ再接続
- 切断時も他チャンネル継続
- Gain/Mute/Master維持
- 個別WAVは原音維持

## Task 1: デバイス型
**Files**
- Create: `crates/stream-audio/src/device.rs`
- Modify: `crates/stream-audio/src/lib.rs`
- Test: `crates/stream-audio/tests/device_selection.rs`

追加型:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDeviceKind {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub kind: AudioDeviceKind,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AudioDeviceSelection {
    #[default]
    Default,
    DeviceId(String),
}
```

テスト:
```rust
#[test]
fn audio_device_info_keeps_id_and_kind() {
    let d = AudioDeviceInfo {
        id: "mic-1".into(),
        name: "USB Mic".into(),
        kind: AudioDeviceKind::Input,
        is_default: false,
    };
    assert_eq!(d.id, "mic-1");
    assert_eq!(d.kind, AudioDeviceKind::Input);
}
```

確認:
```powershell
cargo test -p stream-audio --test device_selection
```

## Task 2: Windowsデバイス列挙
**Files**
- Modify: `crates/stream-audio/src/device.rs`
- Modify: `crates/stream-audio/Cargo.toml`

追加API:
```rust
pub fn enumerate_input_devices() -> Result<Vec<AudioDeviceInfo>, AudioError>;
pub fn enumerate_output_devices() -> Result<Vec<AudioDeviceInfo>, AudioError>;
```

WASAPI/Core Audioで、
- 入力: `eCapture`
- 出力: `eRender`
を列挙し、endpoint ID / friendly name / defaultを取得する。

確認:
```powershell
cargo test -p stream-audio
```

## Task 3: 選択状態
**Files**
- Modify: `crates/stream-audio/src/lib.rs`
- Test: `crates/stream-audio/tests/device_selection.rs`

追加:
```rust
#[derive(Debug, Clone)]
pub struct AudioDeviceState {
    pub input: AudioDeviceSelection,
    pub output: AudioDeviceSelection,
}
```

`Arc<Mutex<AudioDeviceState>>`で共有し、MixerControlとは分離する。

公開API:
```rust
pub fn selected_input_device(&self) -> AudioDeviceSelection;
pub fn selected_output_device(&self) -> AudioDeviceSelection;
pub fn set_input_device(&self, selection: AudioDeviceSelection);
pub fn set_output_device(&self, selection: AudioDeviceSelection);
pub fn refresh_devices(&self);
```

## Task 4: 指定マイクでキャプチャ
**Files**
- Modify: `crates/stream-audio/src/windows_impl.rs`

解決:
```text
Default -> GetDefaultAudioEndpoint(eCapture)
DeviceId(id) -> IMMDeviceEnumerator::GetDevice(id)
```

選択した`IMMDevice`から既存マイクキャプチャを開始する。

実機確認:
- 既定マイク
- USBマイク
- 切替時UIフリーズなし
- メーター追従

## Task 5: 指定出力デバイスでloopback
**Files**
- Modify: `crates/stream-audio/src/windows_impl.rs`

解決:
```text
Default -> GetDefaultAudioEndpoint(eRender)
DeviceId(id) -> IMMDeviceEnumerator::GetDevice(id)
```

選択した出力デバイスでloopback captureする。

## Task 6: 個別再接続
**Files**
- Modify: `crates/stream-audio/src/lib.rs`
- Modify: `crates/stream-audio/src/windows_impl.rs`

追加型:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioReconnectTarget {
    Input,
    Output,
}
```

ルール:
- Input変更 -> マイクだけ再接続
- Output変更 -> PC音声だけ再接続
- 他方は継続

## Task 7: egui ComboBox
**Files**
- Modify: `apps/yaoyorozu-stream/src/app.rs`
- Test: `apps/yaoyorozu-stream/tests/phase9_ui.rs`

UI:
```text
PC音声デバイス
[ Windows既定 ▼ ]

マイクデバイス
[ Windows既定 ▼ ]

[ 音声デバイス更新 ]
```

選択変更時:
- PC音声 -> `set_output_device`
- マイク -> `set_input_device`

## Task 8: 切断状態
**Files**
- Modify: `crates/stream-audio/src/lib.rs`
- Modify: `apps/yaoyorozu-stream/src/app.rs`

追加型:
```rust
pub enum AudioDeviceConnectionState {
    Connected,
    Disconnected,
    Reconnecting,
}
```

UI表示:
```text
マイク: 接続
マイク: 切断
マイク: 再接続中
```

他チャンネルは継続する。

## Task 9: 回帰確認

```powershell
cargo test -p stream-audio
cargo test --workspace
cargo check --workspace
cargo run -p yaoyorozu-stream
```

手動確認:
- 入力デバイス一覧
- 出力デバイス一覧
- Windows既定
- USB等への切替
- メーター追従
- Gain/Mute/Master維持
- 録画成功
- 個別WAV原音維持
- AviUtl2導線維持

## Phase 9.2.1 完了条件
- PC音声デバイス選択可能
- マイクデバイス選択可能
- Windows既定選択可能
- 非同期切替
- 対象チャンネルだけ再接続
- 切断状態表示
- 既存ミキサー/録画/WAV/AviUtl2に回帰なし
