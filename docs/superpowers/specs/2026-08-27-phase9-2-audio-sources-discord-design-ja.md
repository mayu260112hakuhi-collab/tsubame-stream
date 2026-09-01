# Yaoyorozu Stream Phase 9.2 拡張音声ミキサー設計書

作成日：2026-08-27

## 1. 目的
Phase 9.2では、現在の固定式ミキサーを「追加式の音声ソースミキサー」へ拡張する。

主な目的：
- マイク入力デバイスをユーザーが選択できる
- PC音声の出力デバイスも選択できる
- 音声ソースを後から追加できる
- Discord通話音声を独立チャンネルとして扱える
- 将来Chrome、ゲーム、音楽プレイヤー等のアプリ音声も同じ方式で追加できる
- 各音声ソースを個別WAVとして保存できる
- 配信Mixと編集用原音を分離する

## 2. 基本UI

```text
音声ミキサー

PC音声
[ スピーカー / 出力デバイス ▼ ]
■■■■■■────  -15.6 dB
[────────●] 100%   [Mute]

マイク
[ マイク / 入力デバイス ▼ ]
■■■■──────  -24.2 dB
[──────●──]  80%   [Mute]

Discord
[ Discord.exe ▼ ]
■■■■■─────  -18.0 dB
[─────●───]  70%   [Mute]

[ ＋ 音声ソース追加 ]

────────────
配信Mix
■■■■■■────
[────────●] 100%
```

## 3. 音声ソースの種類

### 3.1 出力デバイス
- Windows既定のスピーカー
- USB DAC
- HDMI音声
- Bluetooth出力

### 3.2 入力デバイス
- Windows既定のマイク
- USBマイク
- ヘッドセットマイク
- オーディオインターフェース

### 3.3 アプリ音声
- Discord
- Chrome
- ゲーム
- 音楽プレイヤー

## 4. Discord対応
Discordは「特別な専用実装」ではなく、アプリ音声ソースの最初の対応対象とする。

内部方針：
- Windowsのアプリ音声セッションを列挙
- Discordプロセス/音声セッションを識別
- Discord音声を独立PCMストリームとして取得
- Discord専用メーター・Gain・Muteを持つ
- Discord個別WAVを保存できる

将来Chromeやゲームを追加するときも同じ仕組みを使う。

## 5. チャンネルデータモデル

```text
AudioChannel
- id
- name
- source_kind
- source_id
- gain
- muted
- enabled
- record_individual
- include_in_stream_mix
- current_level
```

source_kind:
- OutputDevice
- InputDevice
- Application

source_id:
- デバイスID
- アプリ音声セッションID
- プロセスID等、再接続に必要な識別情報

## 6. MixerControl
固定フィールド方式からチャンネル集合方式へ拡張する。

```text
MixerControl
  ├─ AudioChannel: PC音声
  ├─ AudioChannel: マイク
  ├─ AudioChannel: Discord
  ├─ AudioChannel: Chrome
  └─ Master
```

MixerControlはスレッドセーフな共有状態として維持する。

## 7. 編集用WAV
編集用WAVは必ず原音を保存する。

```text
recordings/
  desktop.wav
  microphone.wav
  discord.wav
  chrome.wav
```

Gain・Muteは原音WAVには反映しない。配信Mix / 完成MP4にだけ反映する。

## 8. Mixルーティング
各チャンネルは個別に以下を持つ。

- 配信Mixに入れる / 入れない
- 個別WAVを保存する / しない
- Mute
- Gain

Phase 9.2では必要最小限として、
- include_in_stream_mix
- record_individual
まで対応する。

## 9. 実装フェーズ

### Phase 9.2.1 音声デバイス選択
- 入力デバイス列挙
- 出力デバイス列挙
- Windows既定
- マイク選択
- PC音声デバイス選択
- 切替時の再接続

### Phase 9.2.2 可変チャンネル化
- AudioChannel導入
- `＋ 音声ソース追加`
- 動的なGain/Mute
- Master維持

### Phase 9.2.3 Discordアプリ音声
- Windowsアプリ音声セッション列挙
- Discord識別
- Discord個別キャプチャ
- Discordメーター
- Discord Gain / Mute

### Phase 9.2.4 個別WAV / Mixルーティング
- 各チャンネル原音WAV
- 配信Mix参加設定
- WAV保存ON/OFF
- 完成MP4へMixer設定反映

## 10. エラー処理
デバイスが消えた場合：
- UIを固めない
- 対象チャンネルだけ「切断」と表示
- 他チャンネルは継続
- 再接続可能なら候補へ残す

Discord未起動：
- Discord候補を非活性または「未起動」
- ソフト全体は正常動作
- 起動後に一覧更新で検出可能

## 11. 性能方針
- 音声キャプチャ処理はeguiスレッドから分離
- チャンネルごとにブロッキングしない
- Mix処理は音声バッファ駆動
- 1つのチャンネル停止で全Mixを停止しない
- メーター更新はUI描画周期に依存させない

## 12. 後方互換
維持：
- PC音声Gain
- マイクGain
- Master
- Mute
- 原音WAV方針
- GPU録画
- Fix12軽量プレビュー
- AviUtl2連携
- マーカー

## 13. テスト方針
- 入力/出力デバイス列挙
- 既定デバイス選択
- デバイスID保持
- チャンネル追加/削除
- Gain/Mute/Master
- Mix参加ON/OFF
- Discord候補検出
- Discord未起動/終了
- Discord独立レベル
- Discord個別WAV
- 原音保持

## 14. 完了条件
- マイクデバイスを選択できる
- PC音声デバイスを選択できる
- 音声ソースを追加できる
- Discordを独立チャンネルとして追加できる
- Discord音量/Muteを操作できる
- Discordを個別WAV保存できる
- 配信Mixへの参加/除外ができる
- 編集用WAVは原音維持
- 他チャンネル障害でソフト全体が停止しない
