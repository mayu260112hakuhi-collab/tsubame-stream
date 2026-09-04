燕 / Tsubame Phase 8B-UI
音声ミキサー別ウィンドウ化

ベース:
- Phase 8A BGM mixer 適用済み
- Phase 8A.1 release黒窓非表示修正 適用済み

変更内容:
- メイン画面の音声ミキサー本体を別ウィンドウへ分離
- メイン画面には「音声ミキサーを開く」だけを残す
- 初回起動ではミキサーを自動表示
- 2回目以降は開閉状態・位置・サイズを settings.json に保存/復元
- サブモニター上の位置も保存
- 保存座標が現在のモニター外なら、eguiのモニター情報を使って現在のモニター中央へ救出
- × / 閉じるボタンはミキサーUIを隠すだけ
- 音声ワーカー、録画、配信、BGM再生はミキサー窓の開閉から独立
- 既存のPC音声 / マイク / BGM / アプリ / MasterストリップUIはそのまま再利用
- BGM PCM直接Mix / 実メーター / 個別WAVはこのPhaseでは未実装

変更ファイル:
- apps/tsubame/src/app.rs
- apps/tsubame/src/settings.rs

Phase 8A.1確認:
apps/tsubame/src/main.rs 先頭の以下は残してください。
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

Windows実機確認:
1. cargo test -p tsubame-stream settings::tests
2. cargo test --workspace
3. cargo build --release -p tsubame-stream
4. 初回起動でミキサーが自動表示される
5. ×でミキサーだけ消え、録画/配信/BGM音声が継続する
6. メインの「音声ミキサーを開く」で再表示できる
7. ミキサーを移動・リサイズして再起動すると位置・サイズが復元する
8. サブモニター上に置いて再起動すると同じ位置へ戻る
9. サブモニターを外して起動すると、ミキサーが画面外に取り残されない
10. release exe起動時に黒いコンソール窓が出ない

注意:
この作成環境にはRust/Cargoツールチェーンが無いため、cargo test / cargo build は未実行です。
Rustファイルの括弧整合、差分の空白エラー、ZIP内容は静的確認済みです。
