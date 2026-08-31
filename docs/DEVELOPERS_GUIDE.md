# HttpSelector 開発ガイド

## 必要環境

- Windows 10 / 11 x64
- Rust 1.85以降（MSVC toolchain）
- PowerShell 7.4以降（MSIXスクリプト）
- Windows SDK（MSIXのMakeAppx / SignTool）
- Inno Setup 6（従来のEXEインストーラー作成時のみ）

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

本体のdebug EXEが使用中の場合は、停止せず `cargo build --all-targets --target-dir target/review-check` で検証用ビルドを分離できます。自動テストは非表示の独立Win32ウィンドウを使ってフォーカス通知・クリック方式・ESC・ルール登録を確認します。実ブラウザ起動、MSIX導入、表示の目視確認を代替するものではありません。

debug版プレビューは画面操作ツールで検出できる通常ウィンドウ形式を使います。本体のツールウィンドウ形式は維持し、描画・配置・操作処理は本体とプレビューで共通です。

UIの回帰テストでは、100/125/150/200%相当の幅・お気に入り0〜5件/通常0〜20件の配置、設定画面の1列/2列配置、矢印移動、日本語フォントの文字高を検証します。実GUIでは表示倍率、モニター移動、長文の表示とコピー、Tab/Shift+Tab、矢印、Enter、ESC長押し、スクロール、保存/キャンセル/再スキャンを別途確認してください。ESC長押し時間は0.1秒刻みの入力、既定値1.0秒、最小/最大値を確認します。

## データ保存先

非MSIX版は以下に保存します。MSIX版は `%LOCALAPPDATA%\Packages\<PackageFamilyName>\LocalState` に同じ構成で保存し、開発版と正式版を分離します。自動移行は行いません。

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
- `app_identity`: OSのPackage Familyに基づくデータ保存先とIPC名の分離
- `ui` / `settings_ui`: Win32ネイティブ画面

`Settings.record_url_history` は既定値 `OFF` です。ONの場合だけ `UrlHistory` テーブルへワイルドカード化済みURLパターンを記録します。末尾パス・パラメータ値・フラグメントは `*` へ置換し、生URLは保存しません。通常起動で履歴テーブルを検索する処理はありません。

`Settings.escape_hold_duration_tenths` はESC長押し終了時間を0.1秒単位で保持します。既定値は `10`（1.0秒）で、設定画面では `1`（0.1秒）から `100`（10.0秒）まで指定できます。

`DefaultRules` は履歴設定と独立し、手動入力したパターンをそのまま保存します。生URLを自動的に伏せる処理は現時点でありません。保存内容の保護方針は確認中です。

設定画面のプロファイルと一般設定は1トランザクションで保存します。外部キー制約は接続ごとに有効化し、スキーマ移行前には署名済みDBをバックアップします。DB署名はトランザクションの外で更新するため、DB確定と署名確定の間の電源断は復旧対象です。

## 関連付け方針

`installer.iss`はRegisteredApplicationsとURLAssociationsへHttpSelectorを候補登録します。Windowsの既定値は変更しません。利用者がWindows設定からhttpとhttpsを選択します。

MSIX版は `packaging/AppxManifest.xml.template` の `uap3:Protocol` 宣言で候補登録します。既定アプリを強制変更するレジストリ操作は行いません。

## Microsoft Store向け配布

[Microsoft Store公開準備](MICROSOFT_STORE.md)に、Plan/Build/Verifyのコマンド、自己署名証明書の再利用、正式Identityへの切り替え、ローカル導入と残課題を記載しています。Storeは更新を管理するため、App Installerや独自更新処理は実装しません。

Store掲載準備には[掲載情報の草案](STORE_LISTING.md)と[プライバシーポリシー案](PRIVACY_POLICY.md)を使用し、未確定の公開情報や未実施の実機検証を確定済みとして扱わないでください。
