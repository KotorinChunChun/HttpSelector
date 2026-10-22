# HttpSelector 開発ガイド

更新日: 2026-10-23。公開Beta版v0.0.3.5。正本の配置は `C:\develop\KotorinToys\tools\HttpSelector` で、独立Gitリポジトリを維持します。管理連携の契約と運用は[KotorinToys連携](kotorintoys-integration.md)を参照してください。

## 必要環境

- Windows 10 1809以降 / Windows 11 x64（KotorinToys管理連携の対象はWindows 11 x64）
- Rust 1.85以降（MSVC toolchain）
- PowerShell 7.4以降（MSIXスクリプト）
- Windows SDK（MSIXのMakeAppx / SignTool）
- Inno Setup 6（従来のEXEインストーラー作成時のみ）

## ビルドとテスト

```powershell
cd C:\develop\KotorinToys\tools\HttpSelector
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

テストURLでの起動:

```powershell
pwsh -NoProfile -File .\dev\scripts\run-test.ps1 -Url "https://example.com/path?a=b%20c"
```

`run-test.ps1` はルートを解決して `cargo run` を実行するデバッグ起動用の入口です。通常起動では実際の設定を使用し、プロファイル選択後にはQuad9へ照会してから実ブラウザを開きます。URLを指定しない単体起動は `cargo run` です。

正式な利用・管理は[署名付きMSIXの導入](INSTALL_MSIX.md)が必須です。次の開発ビルドは隔離連携試験だけに使用し、通常のKotorinToysはこのEXEへ自動フォールバックしません。通常の `target/debug` や `target/release` と出力を分離します。

```powershell
cargo build --target-dir target/management
& .\target\management\debug\HttpSelector.exe --startup --launch-origin via_kotorintoys
```

`--startup` は管理用の常駐待機です。初期化後に選択画面を出さず、URL・表示・停止要求を待ちます。初回のデータ初期化・プロファイル探索と、設定破損時の復旧確認は行います。`--launch-origin` は `standalone`、`host_manual`、`direct_task`、`via_kotorintoys` を受け付けます。

実データやブラウザ起動に触れないUIプレビュー:

```powershell
cargo run --example ui_preview
cargo run --example ui_preview -- --no-url
cargo run --example ui_preview -- --empty
cargo run --example ui_preview -- --settings
cargo run --example ui_preview -- --settings --empty
cargo run --example ui_preview -- --association
```

プレビューは長い日本語名・複数行メモ・25件のサンプルを使用します。選択してもブラウザは起動せず、設定画面の保存操作もDBへ書き込みません。アイコン未指定時の代替表示を確認できます。

`--association` は架空のPower Apps URIと10個のクエリを使います。製品と同じフォームを使用し、保存コールバックだけを出力へ差し替えています。実DB、外部照会、ブラウザー起動には触れません。

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
- `app_options`: 単体・管理・隔離試験の起動引数と検証専用条件を解釈
- `association`: URLの部分選択、固定部分だけの永続モデル、正規表現生成とAND照合
- `association_ui`: 初期折りたたみの連携フォーム。保存コールバックと開く／戻るの結果を分離
- `risk_local`: 元ホスト表記・IDNA・Unicode類似文字・8サービスのtypoと偽装名を副作用なしで判定し、外部照会可能なASCIIホストだけを抽出
- `quad9`: HTTP/2のDNS over HTTPSとDNS応答検証。対象ページのHTTP取得は行わない
- `risk`: ローカル・外部結果の集約と、承認前の起動・保存を抑止する共通ゲート
- `risk_ui`: キャンセルを初期選択にした起動前確認画面
- `profile_scan`: ChromiumのLocal StateとFirefoxのprofiles.iniを解析
- `storage`: SQLiteスキーマと利用者編集項目を維持する再スキャンマージ
- `integrity`: DPAPI鍵、HMAC署名、バックアップ、復旧
- `browser`: App Pathsレジストリ／既知のパス解決と安全な引数起動
- `single_instance`: 名前付きMutexとWM_COPYDATAによる後続URL転送
- `management/pipe`: 利用者専用ACL・同一セッション照合・期限付きパイプI/Oを管理画面の状態から分離
- `management`: KotorinToys管理契約2と、検査・画面の寿命に依存しないURL受付・順序付きキュー
- `app_identity`: OSのPackage Familyに基づくデータ保存先とIPC名の分離
- `ui` / `settings_ui`: Win32ネイティブ画面

手動選択・ルール一致・割り込みオフの起動は `main::open_profile` へ集約し、プロファイル識別子の検証、危険性検査、必要な利用者確認、連携フォーム、ブラウザ起動の順で処理します。フォームは手動選択かつ質問省略ルールに一致しない場合だけ表示します。保存コールバックは署名済みバックアップ→DB保存→署名の順で実行し、署名失敗時はバックアップを復元します。開くを選んだ場合だけ起動し、保存後に戻っても設定は残ります。警告キャンセルではルール追加やURL履歴を保存しません。全角表記を失わないよう `ValidatedUrl::original()` を保持し、ブラウザへは元の文字列を渡します。

URLの検査・警告中も独立した受付窓口で後続URLを保持し、処理中のURLを置き換えません。受理済みURLはFIFOで扱い、上限超過は送信側へ拒否を返します。管理用 `stop` は検査・確認・設定編集中に拒否し、`show` / `status` はUI応答を確認して返します。

### 高リスクURL判定の境界

元authorityのホストだけを調べ、パス・クエリ内のサービス名や全角文字は類似ドメイン判定に使いません。Unicode正規化、Punycode復元、UTS39のskeletonを組み合わせます。Google・Microsoft・Apple・Amazon・PayPal・LINE・Yahoo! JAPAN・楽天を対象とし、公式ドメインはラベル境界で照合します。確認済み公式ドメイン表は網羅的ではありません。LINEの短語や一部一般語は誤検知を抑える条件を設けています。

Quad9の照会先は `https://dns.quad9.net/dns-query` 固定です。WindowsのWinHTTPを使い、HTTP/2使用・TLS検証・応答サイズ・DNS質問との一致を確認し、HTTPリダイレクトを禁止します。DNS本文にはサブドメインを含むASCIIホストのA照会だけを含め、ECSは付けません。パス・クエリ・フラグメント・認証情報・元URLは渡しません。IPリテラル、ローカル・予約名、PSL未知suffixなどは照会前に除外します。組織内でのみ使う公開形式のドメインをすべて識別することはできません。

外部照会の結果待ちを約5秒で打ち切り、通信失敗・DNSエラー・不正応答は確認不能の警告にします。Quad9のブロック応答と通常の不存在応答は区別し、「ブロックなし」を安全保証として扱いません。WindowsのDNS設定変更、短縮URL展開、対象ページ取得、リダイレクト追跡、URLを識別する永続検査ログは追加しません。検査前に実行する通常の初期化と、利用者が有効にする既存のURLパターン履歴は別です。

ローカル判定、外部結果の集約、DNSの正常・不正応答、キャンセル時の副作用抑止はオフライン単体テストで確認できます。実Quad9への照会テストは通常テストから除外されており、必要な場合だけ明示実行します。GUI・実ブラウザ・導入確認の結果は版別報告で区別してください。

`Settings.record_url_history` は既定値 `OFF` です。ONの場合だけ `UrlHistory` テーブルへワイルドカード化済みURLパターンを記録します。末尾パス・パラメータ値・フラグメントは `*` へ置換し、生URLは保存しません。通常起動で履歴テーブルを検索する処理はありません。

`Settings.escape_hold_duration_tenths` はESC長押し終了時間を0.1秒単位で保持します。既定値は `10`（1.0秒）で、設定画面では `1`（0.1秒）から `100`（10.0秒）まで指定できます。

`DefaultRules` は履歴設定と独立し、入力欄のパターンをそのまま保存します。新規登録時とワイルドカードプリセットは `rules::wildcard_pattern` で認証情報・末尾の非空パス要素・クエリ値・フラグメントを伏せます。連続・末尾スラッシュやクエリの順序・重複を維持し、生成したパターンが元URLに一致することを検証しています。既存ルール、手動編集、履歴生成の処理は変更しません。伏せ字生成は登録候補だけに適用し、起動時の元URLは維持します。

設定画面のプロファイルと一般設定は1トランザクションで保存します。外部キー制約は接続ごとに有効化し、スキーマ移行前には署名済みDBをバックアップします。DB署名はトランザクションの外で更新するため、DB確定と署名確定の間の電源断は復旧対象です。

schema 4ではDefaultRules.ProfileIdをNULL可能にし、質問省略ルールをプロファイル削除と独立させます。従来ルールのID・順序・文字列を維持して移行します。新形式はPattern内の `urlparts-v1:` に続くJSONです。生成器の版、固定したhost/path/query、生成済み正規表現群、質問省略フラグを保存します。未選択の値と認証情報・フラグメントを保存しません。

v0.0.3.4はschema 5です。入れ子条件を知らない旧版が外側ホストだけで自動起動しないよう、版ガードを上げます。schema 4のテーブルと既存行は維持します。移行前は既存の署名済みバックアップ経路を利用し、移行後DBは旧版で開けません。

入れ子はCondition.nestedへ親クエリ名・内側条件・生成済み式を保存します。各段の選択された条件だけを保存し、最大3段・子候補16件・URL長32 KiBに制限します。同名の親クエリは曖昧性のため拒否し、各段のクエリ値を1回ずつ復号します。実行時には各段の式と保存条件の整合性を検証し、原文を起動します。

Teamsの特定ホスト・パスでは、matches_teams_targetにより厳密なHTTPS wrapperのurl展開・内側の固定条件・全条件一致を検査します。ドメイン・パス・クエリの選択は任意で、TeamsCIDの存在やGUID形式を要求しません。外側だけ/固定部分のない入れ子は拒否。保存ガードとresolve_profileで同じ判定を使います。未選択CIDは変化・欠落・重複を無視し、選択CIDでは重複を従来どおり拒否します。既存JSON/DB schema 5とCID固定条件は変更しません。未一致・重複url・別プロファイルへの登録競合ではお気に入りへ転送しません。サインイン済みの本人認証ではなく、同じURL条件のアカウントは区別できません。

Settings.risk_approved_domainsへ完全一致のASCIIホスト名をJSON配列で保持します。明示承認は名称偽装の理由だけを抑制し、他の検査と内側検査は維持します。署名更新が失敗した場合は既存バックアップへ戻します。

正規表現はRustのregexクレートでコンパイルし、全体一致式のANDとして照合します。URL本体と選択したクエリごとに式を分け、先読みや順列列挙を使いません。実行時は保存式と条件の整合性を確認し、保存式で正規化URL（認証情報を除く）を照合します。不正な新形式は一致させず、旧形式のワイルドカードとして解釈しません。既定ルール一覧では要約と読み取り専用の生成式を表示し、削除または対象プロファイルの変更を行えます。条件変更は削除・再登録です。

## 関連付け方針

`installer.iss`はRegisteredApplicationsとURLAssociationsへHttpSelectorを候補登録します。Windowsの既定値は変更しません。利用者がWindows設定からhttpとhttpsを選択します。

MSIX版は `packaging/AppxManifest.xml.template` の `uap3:Protocol` 宣言で候補登録します。既定アプリを強制変更するレジストリ操作は行いません。

## Microsoft Store向け配布

Beta配布は[GitHub Releases](https://github.com/KotorinChunChun/HttpSelector/releases/latest)と既存公開サイトの二拠点です。手順は[二拠点配布](GITHUB_RELEASE.md)を正本とします。distribution.mode=dualでWeb向けStage/Publish/Verify/Fullを使用します。App Installerの参照先は公開サイトです。

開発ソースの表示版・MSIX生成設定は `0.0.3.5`、Cargo版は `0.0.3+5` です。署名付きBeta MSIX 0.0.3.5を生成・公開済みです。実機への導入・更新操作は未実施です。KotorinToys向けの `distributionStatus: unpublished` は本体対応インストーラーの台帳とは未連携であることを示し、Beta MSIXの公開状況とは別です。

[Microsoft Store公開準備](MICROSOFT_STORE.md)に、Plan/Build/Verifyのコマンド、自己署名証明書の再利用、正式Identityへの切り替え、ローカル導入と残課題を記載しています。StoreチャネルはStoreの更新管理を使い、BetaチャネルのApp Installerとは分けます。独自アップデーターは実装しません。

Store掲載準備には[掲載情報の草案](STORE_LISTING.md)と[プライバシーポリシー案](PRIVACY_POLICY.md)を使用し、未確定の公開情報や未実施の実機検証を確定済みとして扱わないでください。
