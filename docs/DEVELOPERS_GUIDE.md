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
