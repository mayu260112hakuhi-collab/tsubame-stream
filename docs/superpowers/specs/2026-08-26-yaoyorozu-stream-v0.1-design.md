# Yaoyorozu Stream v0.1 正式設計書

## 1. 目的

Yaoyorozu Stream は、Rustで構築する **YouTube専用の軽量ライブ配信・収録アプリケーション** とする。

OBSの全機能を再現することは目的とせず、以下を最優先する。

1. YouTube配信に必要な機能へ絞る
2. ゲーム・Blender等と同時使用しても軽量であること
3. 一つの処理停止がUI・録画・配信全体のフリーズへ波及しないこと
4. 配信と同時に編集しやすい素材を分離収録すること
5. AviUtl2へ編集データを引き渡せること

---

## 2. v0.1 機能範囲

### YouTube
- Google / YouTubeアカウント連携
- チャンネル情報取得
- Yaoyorozu Stream内から新規ライブを作成
- YouTube側で作成済みの予約ライブ一覧を取得・選択
- タイトル・公開設定などの配信情報を扱う
- 配信開始 / 停止
- 接続状態・配信状態をUIへ表示

### 映像
- 画面キャプチャ
- ウィンドウキャプチャ
- プレビュー
- シーン切替
- 1080p60 / 1080p30 / 720p60 / 720p30
- 標準プリセット:
  - ゲーム: 1080p60
  - 作業: 1080p30
  - 軽量: 720p30 または 720p60

### 音声
v0.1は以下の3系統に限定する。

- マイク
- PC音声
- YouTube配信用ミックス

アプリ別音声分離はv0.2以降とする。

### ローカル収録
- 標準コンテナ: MP4
- 映像: H.264
- 音声: AACを基本
- 編集用にマイク音声とPC音声を分離して保存
- 映像・音声・マーカーで共通タイムベースを使用

### 編集マーカー
配信中に以下を記録できる。

- CUT
- SHORT
- CHAPTER
- NOTE / 事故等の任意メモ

各マーカーはタイムコードとラベルを持つ。

---

## 3. UI

採用案: **D案 コンパクト / ミニマル型**

### 上部
- 配信
- シーン
- ソース
- マーカー
- 設定

### 中央左
- 大型プレビュー

### 中央右: YouTubeパネル
- LIVE状態
- タイトル
- 公開設定
- 解像度 / FPS
- ビットレート
- 新規ライブ作成
- 予約済みライブ選択
- 配信開始 / 停止

### 下部
- マイクレベル
- PC音声レベル
- 配信用ミックス
- ミュート等の最低限の操作

### 編集マーカー
- CUT
- SHORT
- CHAPTER
- NOTE

### 配信停止後
- 「AviUtl2へ送る」を表示
- JSON保存済みの場合は再送可能

---

## 4. アーキテクチャ

重い処理をUIスレッドへ置かない。

```text
                 ┌──────────────┐
                 │      UI      │
                 └──────┬───────┘
                        │ commands / status
        ┌───────────────┼────────────────┐
        ↓               ↓                ↓
   Capture Worker   Audio Worker    YouTube Worker
        │               │                │
        └──────┬────────┘                │
               ↓                         │
          Mixer / Sync                   │
               │                         │
        ┌──────┴─────────┐               │
        ↓                ↓               │
 Encoder Worker     Recording Worker     │
        │                │               │
        └──── YouTube Stream ────────────┘

 Recording Worker
        ↓
 MP4 + mic + desktop + edit JSON
        ↓
 AviUtl2 Bridge
```

各ワーカーは境界付きキュー / チャンネルで接続する。

### フリーズ耐性原則
- UIはキャプチャ・エンコード・ネットワークI/Oを同期的に待たない
- キューは無制限に成長させない
- 遅延時は全体停止よりフレームドロップを優先
- YouTube通信停止がローカル録画を停止させない
- AviUtl2 Bridge停止が配信・録画へ影響しない
- ワーカー単位でエラー状態を通知し、可能なら再初期化する
- UIは各ワーカーの状態を監視するだけにする

「絶対にフリーズしない」というOS/GPUレベルの保証は行わず、**障害を局所化して全体フリーズへ連鎖させないこと**を設計目標とする。

---

## 5. GPUエンコード

起動時に利用可能なハードウェアエンコーダーを検出する。

優先候補:

```text
NVIDIA → NVENC
AMD    → AMF
Intel  → Quick Sync
利用不可 → CPU H.264
```

UI設定:
- 自動
- GPU指定
- CPU

ゲームやBlenderと同時使用するため、ハードウェアエンコードを優先する。

---

## 6. 収録データ

1回の配信 / 収録を一つのセッションディレクトリとして管理する。

例:

```text
2026-08-26_tsubame_stream/
├─ recording.mp4
├─ microphone.wav
├─ desktop.wav
└─ yaoyorozu_edit.json
```

MP4を標準形式とする。

録画停止・異常終了時の破損リスクを低減するため、Muxerの終了処理、定期的な状態保存、セッションメタデータ保存を実装対象に含める。

---

## 7. AviUtl2 Bridge

採用方式:

**JSON + Windows Named Pipe + AviUtl2 Bridge Plugin**

### 永続データ
`yaoyorozu_edit.json` を正本とする。

### リアルタイム / 即時転送
Windows Named Pipeを利用する。

### AviUtl2操作
AviUtl2側BridgeプラグインのみがAviUtl2 SDKを扱う。

Rust本体からAviUtl2プロジェクトファイルを直接編集しない。

```text
Yaoyorozu Stream
       │
       ├─ recording.mp4
       ├─ microphone.wav
       ├─ desktop.wav
       └─ yaoyorozu_edit.json
                    │
          Windows Named Pipe
                    ↓
          AviUtl2 Bridge Plugin
                    ↓
              AviUtl2 SDK
                    ↓
                Timeline
```

### タイムライン配置

```text
Layer 1 : 録画映像
Layer 2 : マイク
Layer 3 : PC音声
Layer 4 : 編集マーカー
```

AviUtl2が起動していない場合でもJSONを保存し、後からBridge側から読み込めること。

---

## 8. 編集JSON

概念スキーマ:

```json
{
  "format": "tsubame_stream_edit",
  "version": 1,
  "session": {
    "fps": 60,
    "width": 1920,
    "height": 1080
  },
  "media": {
    "video": "recording.mp4",
    "microphone": "microphone.wav",
    "desktop_audio": "desktop.wav"
  },
  "markers": [
    {
      "time_ms": 185420,
      "type": "SHORT",
      "label": "ここ使う"
    }
  ]
}
```

パスは可能な限りセッションディレクトリ基準の相対パスとする。

JSONにはAviUtl2固有の内部構造を持たせず、編集上の意味データだけを保存する。

---

## 9. 推奨Rustモジュール境界

```text
tsubame/
├─ crates/
│  ├─ stream-core/
│  │  ├─ session
│  │  ├─ clock
│  │  ├─ marker
│  │  └─ message
│  ├─ stream-capture/
│  ├─ stream-audio/
│  ├─ stream-encoder/
│  ├─ stream-youtube/
│  ├─ stream-recording/
│  ├─ stream-aviutl-bridge/
│  └─ stream-ui/
└─ apps/
   └─ tsubame/
```

責務を分離し、特定のキャプチャAPI・エンコーダー・YouTube API・AviUtl2 SDKへの依存を局所化する。

---

## 10. v0.1 非対象

初版の肥大化を防ぐため以下は後回しとする。

- Twitch等の他配信サービス
- アプリ別音声分離
- 大量の映像フィルター
- 高度なトランジション
- プラグインマーケット
- 複雑なブラウザソース
- OBS完全互換
- 高度な動画編集機能

Yaoyorozu Streamは動画編集ソフトにはせず、編集はAviUtl2へ渡す。

---

## 11. 成功条件

v0.1完成条件:

1. YouTubeアカウントへ接続できる
2. 新規ライブを作成できる
3. 予約済みライブを選択できる
4. 画面またはウィンドウを配信できる
5. 1080p60でゲーム配信できる
6. 1080p30で作業配信できる
7. GPUエンコーダーを自動選択し、利用不可時はCPUへフォールバックできる
8. マイクとPC音声を配信ミックスできる
9. MP4・マイク・PC音声を編集用に保存できる
10. CUT / SHORT / CHAPTER / NOTEを記録できる
11. 編集JSONを保存できる
12. AviUtl2へ3素材＋マーカーを渡せる
13. YouTubeまたはAviUtl2側の一時停止がUI全体の停止へ波及しない
14. 配信停止後にAviUtl2へ再送できる

---

## 12. 実装順

一度に全機能を作らず、独立して検証可能な順に進める。

1. `stream-core` — セッション、共通時計、メッセージ、マーカー
2. `stream-ui` — D案UIの骨格
3. `stream-capture` — Windows画面 / ウィンドウキャプチャ
4. `stream-audio` — マイク / PC音声
5. `stream-encoder` — GPU検出とH.264
6. `stream-recording` — MP4＋分離音声
7. `stream-youtube` — OAuth、ライブ作成・選択、配信
8. 編集マーカー統合
9. JSON出力
10. Named Pipe
11. AviUtl2 Bridge Plugin
12. 障害注入・長時間安定性試験

各段階でテスト可能な状態を維持する。
