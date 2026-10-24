# CLIによる機能試験

Windows x64、Rust MSVC、PowerShell 7.4以降、Python 3、Windows SDKのMakeAppxを使用します。コンピュータユース、マウス操作、ブラウザーでの手動操作を使わずに実行できます。初回の依存ライブラリー取得にはCargoの接続が必要です。

```powershell
cd C:\develop\KotorinToys\tools\HttpSelector
./dev/scripts/test-all.ps1
```

PythonはPATHまたはCodex同梱ランタイム、CargoはPATHまたはユーザーのScoop Rustから検出します。それ以外は `-PythonPath`、`-CargoPath` で実行ファイルを指定します。`-TimeoutSeconds` は各検証コマンドの制限時間（既定600秒）です。処理が止まった場合は開始したプロセスの子孫だけを停止し、失敗として記録します。

## 通常実行の範囲

- fmt、全Rustターゲットの試験、Clippyの警告拒否。
- 製品と同じSession、Storage、IntegrityManagerを使う通し試験。プロファイル選択、保存、自動振り分け、警告、起動要求、後着URL、設定保存、DB復旧を検証します。
- 非表示の実Win32コントロール、ネイティブのメッセージ配送、ツールチップ説明文、キー操作、レイアウト、DPIの試験。
- 製品debug EXEを別プロセスで起動し、無表示起動・管理パイプの状態取得・誤PID拒否・正常停止を検証する統合試験。
- 署名方針、MSIX配布スクリプト、FTPS公開対象の限定・順序・拒否条件の試験。接続・公開はしません。
- 一括実行入口自体の失敗、タイムアウト、JUnit、テスト対応表の整合性の試験。

一時DBと架空プロファイルを使用します。実利用者の設定・Cookie・既定ブラウザー・信頼ストアは変更しません。画面選択・通知・ブラウザー起動・外部照会の境界だけを試験用に置き換え、振り分け・保存・署名・警告判定は製品コードを実行します。試験用ホストは全メソッドの実装を必須とし、不足した実装が実画面や実通信へ流れる既定処理を持ちません。

## 明示して実行する検証

```powershell
# releaseビルドも確認する。署名付きMSIXの再生成はしない。
./dev/scripts/test-all.ps1 -ReleaseBuild

# 署名済みMSIXのペイロード・BlockMap改変拒否まで検証する。インストールしない。
./dev/scripts/test-all.ps1 -SignedPackagePath ./release/msix/beta/0.0.3.8/HttpSelector_0.0.3.8_x64_dev.msix

# 前面化・最小化復帰と、実EXEの画面表示・ESC・正常停止。一時的に前面が変わる。
./dev/scripts/test-all.ps1 -InteractiveDesktop

# Quad9への実通信。合成した安全な照会用ドメインと公式ブロック確認用ドメインだけを送信する。
./dev/scripts/test-all.ps1 -LiveQuad9
```

オプションは併用できます。対話デスクトップが使えないCIでは `-InteractiveDesktop` を指定しません。実通信と対話デスクトップの試験は、未指定なら合格ではなくスキップです。設定DB内のQuad9 ON/OFFは変更しません。

実EXEの隔離試験では既存のdebug専用データ位置に加え、`HTTPSELECTOR_TEST_NAMESPACE` でmutex・窓クラス・管理パイプを分離します。データ位置とスキャン停止の両方が指定されたdebug版だけで有効です。release/MSIX版はこの環境変数を参照しません。統合試験もdebug時だけ実行します。

## 結果の確認

`packaging/release-config.json` のpackageVersionから対象版を決め、`dev/history/v<版>/logs/cli-<実行識別子>/` へ保存します。

| ファイル | 内容 |
| :--- | :--- |
| results.json | 全コマンドの終了コード、成否、タイムアウト、実行時間、Rust各ケース、外部依存 |
| junit.xml | CIで取り込める各Rustケースと検証コマンドの成否。スキップを区別 |
| cases.json | 機能別のテスト対応表を、実際のRust試験一覧へ展開した結果 |
| 各工程名.log | 標準出力・標準エラー。失敗した場合も残す |

失敗・タイムアウト・試験0件・テスト一覧と実行結果の不一致は終了コード非0です。[機能対応表の正本](../tests/case-map.json)のテスト名が変更・削除されていた場合も失敗します。スキップや未実施を試験合格には数えません。

## 機能とケースの対応

| ID | 主な確認対象 | 主な試験 |
| :--- | :--- | :--- |
| F01 | URL・デコード・元文字列・起動引数 | url_input、sessionの引数/自動起動 |
| F02 | Edge/Chrome/Firefox検出・ゲスト・ポリシー | profile_scan、browser、guest_selection |
| F03–F04 | お気に入り・設定・既定表示・ルール優先・割り込みOFF | settings_ui、ui、sessionのfavorite/interruption |
| F05–F07 | URL部品・クエリ・入れ子・正規表現・Shift・保存/戻る | association、association_ui、rules、sessionのshift/saving/suppression |
| F08–F09 | 偽装ドメイン・承認・警告・Quad9任意送信 | risk_local、risk、quad9、sessionのwarning/approval/quad9 |
| F10–F11 | 履歴の伏せ字・DB署名・復元・移行 | url_pattern、integrity、storage、sessionの復旧/履歴 |
| F12–F13 | 自動保存・試し起動・再検出・後着URL・ルール一覧 | settings_ui、default_rule_ui、sessionの通し試験 |
| F14 | 管理パイプ・無表示起動・URL受付・順序・実EXE表示/停止 | management、single_instance、実WM_COPYDATA、management_lifecycle |
| F15–F16 | ESC・レイアウト・DPI・ツールチップ・起動引数・前面化 | ui、ui_layout、ui_support、win32、browser |
| F17–F18 | 表示版・MSIX・配布制限・試験入口 | app_title、app_identity、PowerShell/Python試験 |

## 外部製品・OSの確認との区別

アプリ全機能に自動試験を対応付けていますが、外部製品の実動作まで代替ホストで検証したとは扱いません。

- 実Edge/Chrome/Firefoxの画面内容、拡張機能、ログイン済みアカウントは外部製品の状態です。通常試験では起動先プロファイル・元URL・ゲスト引数・空白ページ引数を検証します。
- 前面化の実Win32 APIは `-InteractiveDesktop` で検証できます。ブラウザー側が別プロセス/別窓へURLを渡す挙動は別の外部境界です。
- Windowsの既定アプリ選択は利用者の同意が必要です。MSIXのプロトコル登録定義と署名は自動検証し、OSの既定値をテストのために書き換えません。
- 実MSIXの導入・更新を行う必要がある場合は、[導入手順](INSTALL_MSIX.md)の `dev/scripts/install-msix.ps1` を専用環境で明示実行します。通常試験には含めません。
- Store認証と公開サイトの稼働は、ローカル機能試験とは別の公開・配布検証です。[二拠点配布手順](GITHUB_RELEASE.md)を参照してください。

## 個別実行とケース追加

```powershell
cargo test --locked session::scenarios -- --test-threads=1
cargo test --locked ui_support::tests -- --test-threads=1
cargo test --locked --all-targets -- --list
```

業務フローの新ケースは `src/session/scenarios.rs` へ追加し、必要なら `tests/case-map.json` の接頭辞を更新します。試験用ホストの選択・確認キューは予定外の画面要求をエラーにし、未消化の操作も失敗とするため、期待した分岐が実行されず合格することを防ぎます。Win32の詳細なコントロール試験は各画面モジュールのtestsへ追加します。
