# Phase 8B-Perf.1 — プレビュー Deferred 化

## 目的

`show_viewport_immediate` で親ウィンドウとプレビューが同じ再描画周期に縛られていた状態を分離し、
プレビューを開いたままでもメイン操作パネルが待機時 30 FPS で再描画され続けないようにします。

## 変更点

- プレビューを `egui::Context::show_viewport_deferred` へ移行
- プレビュー子ウィンドウは約 30 FPS で独立更新
- 配信・録画をしていないメイン操作パネルは待機時 100 ms 周期へ戻る
- `stream-capture::FrameQueue` に非消費型の最新フレームスナップショットを追加
  - プレビューが最新フレームを読んでも既存の配信用キューを消費しません
- レイヤー状態は `Arc<RwLock<...>>` の読み取りスナップショットとしてプレビューへ共有
- プレビュー上の選択・移動・リサイズはコマンドでメイン状態へ返す
- 画像RGBAを `Arc<[u8]>` 化して、プレビュー状態共有時の画像バッファ丸ごとコピーを回避
- 配信中は、配信へ送った合成済みフレームをプレビューでも再利用し、オーバーレイ合成の二重実行を避ける
- Phase 8B-UI.1 のサブモニター位置・サイズ復元を維持
- 音声ミキサーはまだ `show_viewport_immediate` のまま（Perf.2 で分離予定）

## この段階で意図的に残している点

配信中は現在もメイン `App::update` が映像フレームを `StreamingSession` へ渡しているため、
配信中のメイン側は 33 ms 周期です。Perf.1 はまず **プレビュー由来の親子再描画結合** を外す段階です。
ストリーミング投入処理のUIスレッド分離は別フェーズで扱います。

## Windows側での確認コマンド

```powershell
cargo test -p stream-capture latest_preview_snapshot_does_not_consume_delivery_queue
cargo test -p tsubame-stream deferred_preview_does_not_force_main_ui_to_30fps
cargo test --workspace
cargo build --release -p tsubame-stream
```

## 実機確認

1. プレビューOFFで待機CPUを見る
2. プレビューONにして10秒ほど待ち、CPUを比較する
3. プレビュー映像が約30 FPSで継続することを確認する
4. ししゃも / 画像レイヤーの選択・移動・リサイズを確認する
5. プレビューをサブモニターへ置いて再起動し、位置・サイズが復元されることを確認する
6. 配信を開始して映像が継続することを確認する

## 備考

このパッチは Phase 8B-UI.2（上部アイコンツールバー）を適用済みの `app.rs` を基準にしています。
