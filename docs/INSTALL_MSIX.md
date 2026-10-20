# 署名付きMSIXの導入

HttpSelectorの通常利用には、署名付きMSIXのインストールが必須です。MSIXがHTTP/HTTPSの関連付け候補をWindowsへ登録します。EXEのコピーや開発用の `cargo run` では正式導入になりません。

開発ソースは0.0.3.4で、この版のMSIXは未生成・未公開です。以下の公開物案内は0.0.3.3向けです。0.0.3.4を将来導入すると初回起動時にDBをschema 5へ移行するため、移行後DBを0.0.3.3以前では開けません。今回は実ユーザーDBを移行していません。

## 1. 配布物を取得する

配布元のMSIX、公開証明書、SHA-256と署名者の案内を確認します。署名付きBeta **0.0.3.3** は [GitHub Release](https://github.com/KotorinChunChun/HttpSelector/releases/latest) のAssetsから取得してください。`SHA256SUMS.txt` のハッシュを `Get-FileHash -Algorithm SHA256` の結果と照合します。新しい連携フォームと高リスクURL警告を含みます。通常利用には本版の署名付きMSIXが必要です。今回の公開作業では実機へのインストール・更新は行っていません。

現在はReleaseからMSIX本体をダウンロードして手動で導入・更新してください。GitHub上のApp Installerは移行検証用で、自動更新の動作保証はありません。GitHubが配信する汎用MIMEとWindowsの要件の差を検出したため、旧サイトの更新経路は変更せず維持しています。詳細は[GitHub配布と移行状況](GITHUB_RELEASE.md)を参照してください。

ソースから署名付き成果物を作る場合:

```powershell
cd C:\develop\KotorinToys\tools\HttpSelector
pwsh -NoProfile -File dev/scripts/release-msix.ps1 -Mode Build -Channel beta -ReleaseIntentConfirmed
```

現在のソースからの出力先設定は `release/msix/beta/0.0.3.4/` です。MSIX、CER、SHA256SUMS.txt、verification.jsonを生成しますが、今回の実装作業では実行していません。生成はインストールや公開を行いません。

## 2. 署名を確認する

署名者は `CN=KotorinChunChun`、既存の共通証明書の拇印は `475BABC3C3D1E871D90503D45CC5236216D85EE8` です。配布元の案内とMSIX・CERの値を照合してください。秘密鍵やPFXは配布しません。

自己署名Betaを初めて導入するPCでは、配布元と拇印を確認したうえで、Windowsの証明書画面から公開CERを「ローカル コンピューター」の「信頼されたユーザー」へ登録します。登録には管理者権限が必要です。既に同じ証明書を信頼済みなら再登録は不要です。信頼確認を省略したり、署名検証を無効化したりしないでください。

## 3. MSIXをインストールする

MSIXをダブルクリックし、App Installerの発行元・バージョンを確認して「インストール」または「更新」を選びます。現在利用者へ導入します。コマンドから行う場合も、署名検証付きの入口を使用できます。

```powershell
cd C:\develop\KotorinToys\tools\HttpSelector
pwsh -NoProfile -File dev/scripts/install-msix.ps1
pwsh -NoProfile -File dev/scripts/install-msix.ps1 -Install
```

最初のコマンドは検証のみです。`-Install` を付けるとWindowsのパッケージ登録を実行します。未信頼署名・製品不一致・ダウングレードでは停止し、証明書や利用者データを自動削除しません。

同一Package Familyの `0.0.3.1 / 0.0.3.2 → 0.0.3.3` は更新として扱い、LocalStateの設定を保持します。初回起動時に設定DBをschema 4へ移行するため、更新後のDBは旧版では開けません。署名済みバックアップは更新時に保持します。誤公開の `0.0.4.0` や旧1.x版からは番号が下がるため、この手順では更新しません。必要な設定を退避したうえで、旧版の削除と再導入を個別に行ってください。

開発中に同じ版番号のMSIXを作り直すと、Windowsが内容の異なる同番号パッケージを `0x80073CFB` で拒否する場合があります。インストール入口は自動アンインストールしません。再導入が必要な場合はアプリを正常終了し、対象パッケージのLocalStateを退避してから、当該パッケージだけを削除・再導入し、データを復元してください。公開配布では同番号の内容差し替えを避けます。

## 4. HTTP/HTTPSを関連付ける

Windowsの「設定」→「アプリ」→「既定のアプリ」で **HttpSelector（開発用）** を選び、HTTPとHTTPSにHttpSelectorを指定します。アプリの「既定のアプリ設定を開く」からも設定画面を開けます。

MSIXの導入は候補の登録までです。インストーラーやKotorinToysが既定値を強制変更することはありません。通常のリンクを開いてプロファイル選択へ届くことを確認してください。

## 5. KotorinToysから使う

導入後にKotorinToysで状態を更新し、HttpSelectorを選んで起動します。現在ユーザーの署名付きMSIX登録、Package Family、Publisher、Manifestの4部版を照合します。未導入や旧版の場合はMSIX導入・更新を案内し、開発EXEへ自動切替しません。

パッケージIDを保持するWindowsのアプリ起動経路を使用します。起動済みの場合は同じPIDの画面を表示し、設定やURL確認中の停止は拒否します。自動起動を使う場合は「KotorinToys経由」を選んでください。WindowsApps内のEXEを直接タスク登録しません。

本体の旧配布済みバイナリには新しい登録が含まれない場合があります。管理機能の実装・検証状況は[本版の報告](../dev/history/v0.0.3.2/v0.0.3.2-imp.md)、詳しい契約は[KotorinToys連携](kotorintoys-integration.md)を参照してください。
