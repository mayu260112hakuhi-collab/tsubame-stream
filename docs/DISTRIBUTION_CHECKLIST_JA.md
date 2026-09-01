# 燕 / Tsubame 配布前チェックリスト

## 1. ビルド
```powershell
cargo fmt --all --check
cargo test --workspace
cargo build --release
```

## 2. 起動スモークテスト
- release exe が起動する
- 設定画面が開閉できる
- プレビューを開閉できる
- キャプチャ対象を切り替えられる
- PC音声 / マイクメーターが動く
- アプリ音声を追加・削除できる
- 公式 / 外部アドオン設定画面が開く

## 3. 録画
- 1080p60 録画開始 / 停止
- 完成 MP4 が再生できる
- PC音声 / マイクが入る
- 個別 WAV ON/OFF が反映される
- 録画終了後に一時ファイルが残り続けない

## 4. 配信
- YouTube ストリームキー設定
- 1080p60 接続
- 映像 / PC音声 / マイクを確認
- 配信中 Gain / Mute / Mix / Master が反映される
- プレビュー OFF でも配信が継続する

## 5. 負荷確認
- release exe で計測する
- プレビュー ON / OFF をそれぞれ確認
- CPU / RAM / Capture FPS / Preview Drop を記録

## 6. 配布物
- release exe
- 必要 DLL / FFmpeg の扱いを確定
- README
- ライセンス表記
- 既知の制限事項
- バージョン番号
