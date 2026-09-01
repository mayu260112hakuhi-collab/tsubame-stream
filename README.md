# 燕 / Tsubame

> 軽量・安定を重視した、Rust製の配信・録画ソフト。  
> A lightweight and stability-focused streaming and recording application written in Rust.

---

## 日本語

### 概要

**燕（Tsubame）** は、ゲーム配信や作業配信での負荷を抑えつつ、必要な配信・録画機能をシンプルに扱えることを目標に開発している Windows 向け配信ソフトです。

OBS のような多機能さをそのまま追いかけるのではなく、**軽さ・安定性・分かりやすさ**を重視し、配信中にゲームや作業を邪魔しにくい構成を目指しています。

現在は開発版です。

### 主な特徴

- Windows Graphics Capture（WGC）による画面・ウィンドウキャプチャ
- 1080p / 60fps を想定した配信・録画
- PC音声 / マイク / アプリ音声の個別ミキサー
- 音量調整、Mute、Mix、個別WAV録音
- YouTube / Twitch 配信
- FFmpeg 連携
- 画像・オーバーレイ表示
- プレビュー別ウィンドウ
- 録画中プレビュー負荷軽減
- ハードウェアエンコーダー自動選択
  - NVIDIA NVENC
  - AMD AMF
  - Intel Quick Sync
  - CPU fallback
- アドオン基盤
  - Core
  - Addon API v1
  - 公式アドオン
  - 外部アドオン追加用の基盤

### 設計方針

燕では、次の方針を大切にしています。

- **軽量であること**
- **配信中に落ちにくいこと**
- **設定が分かりやすいこと**
- **音声ミキサーを直感的に扱えること**
- **コア機能とアドオン機能を分離すること**
- **Addon API をできるだけ安定させること**

コア機能として以下を維持し、拡張機能はアドオン側へ分離していく予定です。

- キャプチャ
- 配信
- 録画
- 音声

### 現在の開発状況

現在、以下の機能が動作しています。

- ウィンドウ / デスクトップキャプチャ
- YouTube / Twitch 配信
- 録画
- PC音声 / マイク / アプリ音声
- リアルタイム音量調整
- Mute / Mix
- 個別WAV録音
- 画像オーバーレイ
- プレビュー別ウィンドウ
- 録画時のプレビューFPS切り替え
- CPU負荷軽減処理
- Addon API v1 の管理基盤
- 設定保存・初回起動基盤

### 開発環境

- Rust
- eframe / egui
- Windows Graphics Capture
- WASAPI
- FFmpeg
- Windows 11

### ビルド

Rust 環境と FFmpeg が必要です。

```powershell
cargo fmt --all
cargo test --workspace
cargo build --release
```

実行:

```powershell
cargo run --release
```

ビルドされた実行ファイル:

```text
target\release\yaoyorozu-stream.exe
```

### 注意

このプロジェクトは現在開発中です。

- UIや設定項目は変更される可能性があります
- 一部機能は未実装、または試験実装です
- 外部アドオン実行基盤は今後拡張予定です
- ストリームキーなどの秘密情報はリポジトリへコミットしないでください

### プロジェクト名について

内部のCargo package名には、過去の開発名である `yaoyorozu-stream` が残っている箇所があります。  
製品名・公開名は **燕 / Tsubame** です。

---

## English

### Overview

**Tsubame** is a Windows streaming and recording application written in Rust, designed with a strong focus on **low resource usage, stability, and simplicity**.

Instead of trying to reproduce every feature found in large streaming suites, Tsubame aims to keep the core workflow compact and dependable, especially while gaming or doing long work streams.

The project is currently under active development.

### Main Features

- Screen and window capture using Windows Graphics Capture (WGC)
- Streaming and recording targeting 1080p / 60fps
- Separate mixer channels for:
  - Desktop audio
  - Microphone
  - Application audio
- Gain control, Mute, Mix, and individual WAV recording
- YouTube and Twitch streaming
- FFmpeg integration
- Image and overlay support
- Separate preview window
- Reduced preview workload while recording
- Automatic hardware encoder selection
  - NVIDIA NVENC
  - AMD AMF
  - Intel Quick Sync
  - CPU fallback
- Addon foundation
  - Core
  - Addon API v1
  - Official addons
  - External addon management foundation

### Design Goals

Tsubame is built around the following goals:

- **Lightweight performance**
- **Streaming stability**
- **Simple and readable settings**
- **An intuitive audio mixer**
- **Clear separation between core features and addons**
- **A stable addon API whenever possible**

The following features are intended to remain part of the core:

- Capture
- Streaming
- Recording
- Audio

Optional or advanced functionality will gradually move into the addon layer.

### Current Development Status

The following features are currently implemented or working:

- Window / desktop capture
- YouTube / Twitch streaming
- Recording
- Desktop / microphone / application audio
- Real-time gain control
- Mute / Mix controls
- Individual WAV recording
- Image overlays
- Separate preview window
- Recording preview FPS switching
- CPU load reduction for preview processing
- Addon API v1 management foundation
- Settings persistence and first-run foundation

### Development Stack

- Rust
- eframe / egui
- Windows Graphics Capture
- WASAPI
- FFmpeg
- Windows 11

### Build

Rust and FFmpeg are required.

```powershell
cargo fmt --all
cargo test --workspace
cargo build --release
```

Run:

```powershell
cargo run --release
```

Built executable:

```text
target\release\yaoyorozu-stream.exe
```

### Notes

This project is still under development.

- UI and settings may change
- Some features are experimental or incomplete
- External addon execution support will be expanded later
- Do not commit secrets such as stream keys to the repository

### Project Name

Some internal Cargo package names still use the historical development name `yaoyorozu-stream`.  
The public product name is **燕 / Tsubame**.

---

## License / ライセンス

This project is licensed under the **Apache License 2.0**.  
このプロジェクトは **Apache License 2.0** の下で公開されています。

See `LICENSE` for the full license text.  
ライセンス全文は `LICENSE` を参照してください。

## Status

Development / 開発中
