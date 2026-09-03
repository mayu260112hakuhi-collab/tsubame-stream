燕 / Tsubame - Phase 8A.1 Releaseコンソール非表示

対象:
  apps/tsubame/src/main.rs

変更:
- Windowsのreleaseビルドでコンソールウィンドウを生成しないように変更
- debugビルドでは従来どおりコンソールを残すため、開発時のログ確認は可能

追加行:
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

確認:
1. cargo build --release
2. target/release/tsubame-stream.exe をエクスプローラーから起動
3. 黒いPowerShell/コンソール画面が別途開かず、燕だけ起動することを確認
4. cargo run （debug）ではコンソールが残ることを確認

注意:
- これはPowerShellプロセスを強制終了する処理ではありません。
- release版をGUIアプリとして起動し、余計なコンソール画面を出さない方式です。
- 既に開いているPowerShellからexeを実行した場合、そのPowerShell自体は閉じません。
