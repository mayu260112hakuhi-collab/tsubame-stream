# Phase 9.2.4 個別WAV / Mixルーティング 実装計画

**Goal:** PC音声・マイク・Discord等のアプリ音声を原音WAVとして個別保存し、各AudioChannelの配信Mix設定を完成MP4へ反映する。

**Architecture:** 録画中は編集用原音を常に独立PCMへキャプチャし、Gain/Muteは非破壊で保持する。録画停止時にChannelMixerControlを参照してFFmpegの入力とfilter_complexを可変構築する。個別保存OFFのトラックもMixに必要なら一時WAVとして利用し、mux成功後に削除する。

## Task 1: アプリ音声WAV録音
- ApplicationRecordingPathを追加
- process-loopback AudioClientを録画専用スレッドでも起動
- Float32 48kHz stereo -> PCM16 WAVへ変換
- アプリごとに独立スレッド化

## Task 2: RecordingSessionへ可変AudioChannelを接続
- start_with_audio_routingを追加
- ChannelMixerControlを保持
- 録画開始時にApplicationチャンネルと保存パスを確定

## Task 3: 可変FFmpeg Mix
- FinalMixTrackモデルを追加
- 配信Mix ONのみfilter_complexへ接続
- Gain/Mute/Masterを最終Mixにだけ適用
- 1トラック / 複数トラック / 0トラックを処理

## Task 4: 個別WAV保存ON/OFF
- PC音声 / マイクにもUIチェックを追加
- アプリ音声の「予約」表示を実保存へ変更
- 保存OFFのWAVは完成MP4生成後に削除

## Task 5: 障害分離
- アプリ音声録音スレッドの失敗をPC音声・マイクから分離
- 利用可能なWAVだけ最終Mix入力へ採用

## Task 6: テスト
- 可変Mix filterの参加/除外
- Mute=0 gain
- 全除外時None
- Application WAVファイル名
- cargo test/check/runはWindows実機で確認
