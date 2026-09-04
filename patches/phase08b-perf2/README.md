# Phase 8B-Perf.2 — 音声ミキサー Deferred 化

## 目的

Phase 8B-Perf.1 でプレビューを `show_viewport_deferred` に分離したのに続き、
音声ミキサーも親ウィンドウと独立した再描画周期へ移します。

これにより、音声メーターを約30 FPSで動かしていても、待機中のメイン操作パネルを
同じ周期で再描画し続けない構成にします。

## 変更点

- 音声ミキサーを `show_viewport_immediate` から `show_viewport_deferred` へ変更
- ミキサーは約33 ms周期（約30 FPS）で独立更新
- メイン操作パネルは待機時100 ms周期のまま
- `AudioWorker` を `Arc` 共有に変更し、Deferredミキサーからライブ音量レベルと各チャンネル状態を直接参照
- PC音声 / マイク / Master / アプリ音声の既存Gain・Mute・Mix・WAV操作を維持
- 音声デバイス選択、アプリ音声追加・削除・一覧更新も維持
- BGMプレイヤーは引き続きメインApp所有のため、BGM Gain / Mute / 削除だけコマンドで親へ返す
- ミキサーの開閉・位置・サイズ保存、サブモニター復元を維持
- 配信中 / 録画中のデバイス構成固定も維持
- Phase 8B-Audio前のため、BGM実メーター / Mix / WAVはまだ未接続

## 意図的に変えていないもの

- 固定済みのミキサーストリップUI
- プレビューDeferred化（Perf.1）
- 上部アイコンツールバー（UI.2）
- マルチモニター位置・サイズ復元（UI.1）
- BGMの現在のローカル再生方式

## Windows側での確認コマンド

```powershell
cargo test -p tsubame-stream deferred_mixer_uses_independent_meter_repaint_period
cargo test -p tsubame-stream audio_worker_can_be_shared_with_deferred_mixer
cargo test --workspace
cargo build --release -p tsubame-stream
```

## 実機確認

1. プレビューOFF / ミキサーOFFで待機CPUを確認
2. ミキサーだけONにして10秒ほど待ち、メイン側CPU負荷の増え方を見る
3. PC音声 / マイクのメーターが滑らかに更新されることを確認
4. Gain / Mute / Mix / WAVが従来通り操作できることを確認
5. BGM Gain / Mute / 削除が従来通り動くことを確認
6. アプリ音声追加・削除、デバイス選択が動くことを確認
7. ミキサーをサブモニターへ置いて再起動し、位置・サイズが復元されることを確認
8. プレビューとミキサーを両方ONにして、メイン操作パネルが不要に30 FPSへ引っ張られないことを確認

## 次のフェーズ

Phase 8B-Audio：BGMを独立PCMチャンネルとして `stream-audio` へ直接統合します。
