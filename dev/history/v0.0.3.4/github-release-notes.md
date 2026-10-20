# HttpSelector v0.0.3.4（署名付きMSIX・ベータ版）

表示バージョン・MSIXバージョンはどちらも **0.0.3.4** です。

## 主な変更

- Microsoft公式CDNの誤警告を修正。明示承認した完全一致ホストの名称偽装警告を保存・解除できます。Unicodeや外部脅威の警告は抑制しません。
- クエリ内URLを展開し、内側のドメイン・パス・クエリを選択して条件にできます。長いパラメータは表示だけを短縮します。
- Teams Safe Links内のSharePointドメインとTeamsCIDをプロファイルに対応付け。未登録・欠落・重複した識別値は手動選択に戻ります。
- 判定に内側URLを使っても、起動する元のSafe Linksは変えません。TeamsCIDはURL由来のヒントであり、認証済み本人識別ではありません。

## 導入

HTTP/HTTPSの関連付け登録のため、通常利用には署名付きMSIXからのインストールが必須です。

[公開サイト・App Installer](https://excel-chunchun.sakura.ne.jp/download/HttpSelector.html) / [証明書の導入手順](https://excel-chunchun.sakura.ne.jp/download/httpselector-certificate/index.html) / [詳細手順](https://github.com/KotorinChunChun/HttpSelector/blob/main/docs/INSTALL_MSIX.md)

GitHubのAssetsにも同じMSIX・公開CER・App Installer・SHA256SUMS.txtを掲載しています。App Installerの更新先は、適切な配信MIMEを提供する上記サイトです。GitHubではMSIXの直接取得も可能です。

- 発行元: `CN=KotorinChunChun`
- 証明書拇印: `475BABC3C3D1E871D90503D45CC5236216D85EE8`
- Windows 10 1809以降 / Windows 11、x64。
- 秘密鍵やPFXは配布しません。署名確認を無効にしないでください。

## 更新時の注意・検証範囲

設定DBはschema 5へ移行します。移行後のDBを旧版では開けません。必要に応じて設定をバックアップしてください。選択した入れ子のドメイン・TeamsCID等は暗号化されない設定DBに保存します。URL履歴の自動記録は既定OFFです。

同一ベータ系列0.0.3.3からの更新を想定しています。旧1.xや誤公開0.0.4.0は番号が下がるため自動更新できません。

Rust回帰試験113件、fmt、Clippy、合成URLの実画面操作を確認済み。実Teamsアカウント・業務PDF・実ユーザーDB・このPCへのMSIX導入と更新操作は未検証です。警告がないことは安全を保証しません。

署名付きMSIXは2,552,234バイト、SHA-256は `C9DD9F33453AC1F5B2DBCE20D5CEAA2127F8196ACCC2738DE30416AEE8C8A2E4`。ビルド元はタグv0.0.3.4の `82168282fc18f02bf1a5e31b75080b46aa5f2b0e` です。署名・パッケージ内容・改ざん拒否を検証し、両配布先からの匿名再取得で一致を確認しました。
