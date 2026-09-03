# Phase 9.2.3 アプリ別音声キャプチャ 実装計画

**Goal:** Discordを最初の対象として、Windowsの実行中アプリ音声を独立チャンネルとして選択・キャプチャ・メーター表示できるようにする。

**Architecture:** Windows音声セッションから候補PIDを列挙し、同名親プロセスまで遡ってcapture PIDを決定する。wasapi 0.24のprocess-loopback AudioClientを専用スレッドで起動し、各AudioChannelへピーク値を反映する。個別WAVと完成MP4への実音声合流はPhase 9.2.4へ分離する。

**Tech Stack:** Rust / wasapi 0.24 / sysinfo 0.38 / egui / Windows Process Loopback API

**Spec:** `docs/superpowers/specs/2026-08-27-phase9-2-audio-sources-discord-design-ja.md`

## Task 1
- ApplicationAudioSourceモデルを追加
- Discord優先ソートとラベル生成
- テストを追加

## Task 2
- Windows render audio sessionを全endpointから列挙
- session PIDをsysinfoでプロセス名へ解決
- Discord/Chromium系の同名親PIDをcapture rootとして採用

## Task 3
- AudioChannelへsource_id/process_idを追加
- Applicationチャンネル生成APIを追加
- 同一PIDの重複追加を防止

## Task 4
- AudioClient::new_application_loopback_client(pid, true)で独立キャプチャ
- Float32 48kHz stereoでピーク計測
- Gain/Muteをチャンネルメーターへ反映
- 専用stop flag/threadで削除・終了可能にする

## Task 5
- eguiへアプリ音声候補ComboBox、一覧更新、追加UIを実装
- Discordを候補先頭へ表示
- 接続/切断、PID、Gain/Mute、配信Mix、個別WAV予約を表示

## Task 6
- 既存PC音声/マイク/GPU録画/AviUtl2導線を維持
- cargo test --workspace
- cargo check --workspace
- cargo run -p tsubame
