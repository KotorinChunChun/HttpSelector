# KotorinToys連携

更新日: 2026-10-23。開発ソースHttpSelector v0.0.3.6 / 管理契約2。署名付きMSIX 0.0.3.5をGitHubと公開サイトへ公開済みですが、今回このPCへの導入は行っていません。今回KotorinToys本体は改修・公開しません。

## 配置と管理対象

正本は `C:\develop\KotorinToys\tools\HttpSelector` です。HttpSelectorの独立したGit履歴を保持し、KotorinToys側は `catalog/tools.json` から子の [tool.json](../tool.json) を参照します。子の公開先は [KotorinChunChun/HttpSelector](https://github.com/KotorinChunChun/HttpSelector) です。親の採用Gitリンクや配信configへの反映は、子のソース・Release公開とは別に管理します。

通常利用はWindows 11 x64の署名付きMSIXを必須とします。[MSIX導入手順](INSTALL_MSIX.md)に従ってインストール・HTTP/HTTPS関連付けを行ってください。開発ソースの表示・管理定義・生成設定は `0.0.3.6`、Cargo版は `0.0.3+6` です。0.0.3.6は未生成・未公開で、公開済み実MSIXは0.0.3.5です。今回このPCへのインストール・本体配信一覧の変更は行っていません。定義の `distributionStatus: unpublished` は本体対応インストーラー台帳へ載せないことを表します。

KotorinToysは現在ユーザーのMSIX登録、Publisher、署名付き配布の登録由来、Manifestの製品識別と4部版を照合します。未導入・旧版・確認不能では開発EXEへ切り替えず導入・更新へ案内します。Windowsのパッケージ起動APIから起動し、MSIXのPackage Family別管理窓口へ接続します。管理資料は子リポジトリ側を参照します。

`target/management/debug/HttpSelector.exe` は隔離試験用です。次の直接起動は開発検証専用で、正式なMSIX導入ではありません。

```powershell
cd C:\develop\KotorinToys\tools\HttpSelector
cargo build --target-dir target/management
```

## 起動と画面の寿命

通常はKotorinToysの管理画面から操作します。管理用の無表示待機を直接確認する場合は次を使います。

```powershell
& .\target\management\debug\HttpSelector.exe --startup --launch-origin via_kotorintoys
```

`--startup` では初期化後に選択画面を出さず、URL・表示・停止要求を待ちます。選択画面を閉じたりURLの処理が終わったりしても、管理プロセスは待機を続けます。初回のプロファイル探索・データ初期化は行い、設定が破損している場合の復旧確認は省略しません。

単体起動とWindowsのHTTP/HTTPS関連付けも維持します。URLなしで単体起動すると選択画面を表示し、URLありなら既定ルールまたは選択プロファイルで処理します。`--startup` のない単体起動は、画面と受理済みURLの処理が終わると終了します。既存プロセスがある場合、後続起動はURLまたは表示要求を転送します。

デバッグ起動の共通入口は次です。通常の利用者設定と実ブラウザーを使用するため、URLを開く操作は実際のブラウザー起動になります。

```powershell
pwsh -NoProfile -File .\dev\scripts\run-test.ps1 -Url "https://example.com/path?a=b%20c"
```

## 管理窓口

非MSIX開発試験用の名前付きパイプは `\\.\pipe\KotorinToys.<現在のWindows利用者SID>.http-selector` です。正式管理するMSIX版は、同名にPackage FamilyのSHA-256（小文字16進数）を付けた別窓口を使います。KotorinToysは定義されたPackage Familyから同じ窓口名を生成し、非MSIX版と混在させません。

パイプの所有者とACLを現在のWindows利用者に限定し、リモート接続を拒否します。要求元のプロセスが同じ対話セッションにいることを確認します。製品の表示名による曖昧な検索では操作しません。管理応答にはURLやプロファイル情報を含めません。

| 操作 | 動作 |
| --- | --- |
| `status` | PID、準備状態、製品版、起動経路、対応操作、結果を返す |
| `show` | 対象PIDを確認して、画面の表示・前面化を要求する。UI応答後に完了を返す |
| `stop` | 対象PIDと処理状態を確認し、正常終了を受理する。プロセス終了は別に確認する |

要求はUTF-8の1行JSONで、`command`、操作対象の `expectedPid`、照合用の `requestId` を使用します。`status` には従来の文字列要求も利用できます。`show` と `stop` の対象PID欠落・不一致は拒否します。要求サイズは8,192バイト、要求IDは128バイトまでです。`shortcuts` 操作は未対応として返し、未対応操作を成功扱いしません。

接続後の管理要求は読み込み・UI応答・応答書き込みを含め約2秒の期限を持ちます。期限内にUIへ処理を渡せない場合は失敗を返し、期限切れの表示要求を後で勝手に実行しません。`stop` の `accepted` は正常終了の受理であり、終了完了ではありません。

設定・既定ルール編集中、ドメイン連携確認中、URLの外部検査・警告中は `stop` を拒否します。作業を終えてから再試行してください。管理上の正常停止で強制終了へ切り替えることはありません。

## URL検査と後続URL

画面とは独立したURL受付窓口で後続URLを保持します。処理中のURLを上書きせず、受理したURLはFIFOで扱います。保留URLは最大256件で、上限超過は送信側へ拒否を返します。

手動選択、既定ルール、割り込みオフの起動は同じ検査ゲートを通ります。ローカルの表記・ブランド照合とQuad9のHTTP/2 DNS over HTTPSを組み合わせ、必要な警告はキャンセルを初期選択にします。キャンセルでは対象URLの起動・ドメイン連携追加・自動履歴記録を行いません。

Quad9へ送るのはサブドメインを含むASCIIホスト名だけです。元URL・パス・クエリ・フラグメント・認証情報・ポートは送りません。ローカル名、IPリテラル、予約名、公開suffixを確認できない名前は外部照会から除外します。Quad9は接続元IPなどの通信情報を認識できるため、匿名化の仕組みではありません。WindowsのDNS設定は変更しません。

検査用の永続URLログは追加しません。通常の任意URLパターン履歴は独立した既存機能です。「ブロックなし」は安全保証ではなく、対象ページ取得・短縮URL展開・リダイレクト追跡も行いません。利用上の境界は[利用ガイド](USERS_GUIDE.md)、情報の扱いは[プライバシーポリシー案](PRIVACY_POLICY.md)を参照してください。

## データと検証記録

ソースの配置変更で利用者データは移動しません。非MSIX版は `%LOCALAPPDATA%\HttpSelector`、MSIX版は自身のPackage FamilyのLocalStateを使います。配布形態の異なる設定は自動移行しません。

再現可能なビルド・テスト手順は[開発ガイド](DEVELOPERS_GUIDE.md)に記載します。本版の要求・計画・結果は [dev/history/v0.0.3.2](../dev/history/v0.0.3.2/v0.0.3.2-imp.md)、実行時の動画・生ログはその配下の `logs/<実行識別子>/` にまとめます。オフライン単体試験、実Quad9照会、実GUI操作、ブラウザー起動、MSIX導入、公開確認はそれぞれの実施範囲を分けて報告します。
