# Phase 8B-Audio.1 — BGM direct PCM streaming

BGMをWindowsデスクトップ音声に依存せず、48 kHz / stereo / PCM16として燕のライブ音声ブリッジへ直接流す第1段階です。

## この段階で入るもの

- `stream-audio` に外部PCMチャンネル / `ExternalPcmSender` を追加
- BGMデコーダーを48 kHz stereoへ統一し、ローカル再生と同じタイミングでPCMをtap
- BGMチャンネルの実レベルをミキサーメーターへ反映
- BGM `Mix` ON/OFF を実際のライブMixへ反映
- BGM Gain / Mute をローカル再生と直接PCMの両方へ同期
- 外部PCMチャンネルがある配信では、PC音声のライブ取得から燕自身のプロセス音を除外し、BGMの二重取りを防止

## まだ入れないもの

- BGM個別WAV録音（Phase 8B-Audio.2）
- 録画最終MuxへのBGM個別トラック統合（Phase 8B-Audio.2）
- 配信開始後に新規追加したBGMチャンネルを動的にライブブリッジへ追加する処理

### 注意

ライブ音声ブリッジは配信開始時にチャンネル一覧を確定するため、Audio.1ではBGMを配信に直接入れたい場合、BGMレイヤーを追加してから配信開始してください。
