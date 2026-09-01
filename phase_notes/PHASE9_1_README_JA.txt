Yaoyorozu Stream Phase 9.1 — UI再構成

・通常プレビュー 約300x169px
・録画解像度とは独立
・上部に録画/配信/録画+配信/YouTube状態
・中央に音声ミキサー予約パネル
・Scenes/Sources予約パネル
・BottomにAviUtl2導線とマーカーを維持
・Phase 8 Fix12のGPU録画/軽量プレビュー処理は変更なし

Windows確認:
cargo test --workspace
cargo check --workspace
cargo run -p yaoyorozu-stream

生成環境にはcargoが無いため、RustコンパイルはWindows実機で確認してください。
