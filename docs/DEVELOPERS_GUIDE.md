# HttpSelector 開発ガイド

## 必要環境

- Windows 10 / 11 x64
- Rust 1.85以降（MSVC toolchain）
- PowerShell 7
- Inno Setup 6（インストーラー作成時）

## ビルドとテスト

```powershell
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

テストURLでの起動:

```powershell
pwsh ./dev/scripts/run-test.ps1
```

実データやブラウザ起動に触れないUIプレビュー:

```powershell
cargo run --example ui_preview
cargo run --example ui_preview -- --no-url
cargo run --example ui_preview -- --empty
cargo run --example ui_preview -- --settings
cargo run --example ui_preview -- --settings --empty
```

プレビューは長い日本語名・複数行メモ・25件のサンプルを使用します。選択してもブラウザは起動せず、設定画面の保存操作もDBへ書き込みません。アイコン未指定時の代替表示を確認できます。

UIの回帰テストでは、100/125/150/200%相当の幅・お気に入り0〜5件/通常0〜20件の配置、設定画面の1列/2列配置、矢印移動、日本語フォントの文字高を検証します。実GUIでは表示倍率、モニター移動、長文の表示とコピー、検索、Tab/Shift+Tab、矢印、Enter、ESC長押し、スクロール、保存/キャンセル/再スキャンを別途確認してください。

## データ保存先

```text
%LOCALAPPDATA%\HttpSelector\
├── HttpSelector.db
├── HttpSelector.db.sig
├── HttpSelector.db.bak
├── HttpSelector.db.bak.sig
└── integrity.key
```

`integrity.key`はDPAPI CurrentUserで保護されています。DB更新前に署名済みバックアップを作成し、更新完了後にDBを再署名します。

## アーキテクチャ

- `url_input`: URLの長さ、スキーム、ホスト、制御文字を検証
- `profile_scan`: ChromiumのLocal StateとFirefoxのprofiles.iniを解析
- `storage`: SQLiteスキーマと利用者編集項目を維持する再スキャンマージ
- `integrity`: DPAPI鍵、HMAC署名、バックアップ、復旧
- `browser`: App Pathsレジストリ／既知のパス解決と安全な引数起動
- `single_instance`: 名前付きMutexとWM_COPYDATAによる後続URL転送
- `ui` / `settings_ui`: Win32ネイティブ画面

`Settings.record_url_history` は既定値 `OFF` です。ONの場合だけ `UrlHistory` テーブルへワイルドカード化済みURLパターンを記録します。末尾パス・パラメータ値・フラグメントは `*` へ置換し、生URLは保存しません。通常起動で履歴テーブルを検索する処理はありません。

## 関連付け方針

`installer.iss`はRegisteredApplicationsとURLAssociationsへHttpSelectorを候補登録します。Windowsの既定値は変更しません。利用者がWindows設定からhttpとhttpsを選択します。
