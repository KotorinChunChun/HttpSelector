# GitHub・公開サイトでの配布と更新

更新日: 2026-10-23。署名付きBeta MSIX **v0.0.3.5**を[GitHub Release](https://github.com/KotorinChunChun/HttpSelector/releases/tag/v0.0.3.5)と[公開サイト](https://excel-chunchun.sakura.ne.jp/download/HttpSelector.html)へ公開しました。表示・MSIX・App Installerの数値はすべて0.0.3.5です。通常利用には署名付きMSIXが必須です。

## 配布先の役割

- GitHub: ソース、版別Release、同一のMSIX・公開CER・App Installer・SHA256SUMS.txt。
- 公開サイト: 導入案内、証明書案内、App InstallerとMSIXの更新先。既存サイトは廃止しません。
- App InstallerのUriとMainPackageのUriは公開サイトです。GitHubから取得するApp Installerも同じ内容です。

前版と同様、GitHub APIに正しいMIMEを登録しても匿名取得時はapplication/octet-streamでした。このため、GitHubをApp Installerの直接更新先にはしません。公開サイトではapplication/appinstaller・application/msix・application/pkix-cert、正のContent-LengthとHTTPS取得を確認済みです。自動更新の実機操作は別に検証が必要です。

## 利用者向け

1. [公開サイト](https://excel-chunchun.sakura.ne.jp/download/HttpSelector.html)で版と注意事項を確認します。
2. 初回は[署名の確認・証明書導入手順](INSTALL_MSIX.md)に従います。拇印は `475BABC3C3D1E871D90503D45CC5236216D85EE8`、発行元は `CN=KotorinChunChun` です。同じ証明書を信頼済みなら再登録は不要です。
3. 公開サイトの[HttpSelector.appinstaller](https://excel-chunchun.sakura.ne.jp/download/beta/HttpSelector.appinstaller)を開いて導入・更新します。GitHubのMSIXを直接取得し手動導入することもできます。
4. GitHubのSHA256SUMS.txtと `Get-FileHash -Algorithm SHA256` を照合できます。設定DBはschema 5へ移行し、旧版では開けません。

## 保守者向け

- `packaging/release-config.json` のpackageVersionと表示版を一致させます。distribution.modeはdual、beta.baseUriとbeta.appInstallerUriは既存公開サイトです。
- `publish-msix-release.ps1 -Mode Build -Channel beta -ReleaseIntentConfirmed` で既存証明書による署名と検証、App Installer生成を行います。公開済み同版MSIXは再生成・差し替えず、同一の署名済みバイト列を再利用します。
- `-Mode Stage` で対象をステージ・検証し、`-Mode Publish -ReleaseIntentConfirmed -PublishConfirmed` でFTPS dry-run・公開前退避・対象限定配信・匿名HTTPS検証を実施します。対象は製品ページ、証明書案内、アイコン、MSIX、公開CER、App Installerの6ファイルだけです。共有メニュー・他製品を含めません。
- ソース・版タグをpushし、GitHub Releaseをdraftで準備します。MSIX、公開CER、App Installer、SHA256SUMS.txtの4ファイルを添付し、Web公開検証後にReleaseを公開します。秘密鍵・PFX・利用者データ・生ログは公開しません。
- アセットのAPI登録にはMSIX=application/msix、App Installer=application/appinstaller、CER=application/pkix-cert、チェックサム=text/plainを指定します。ただし配信時のMIMEとは区別します。
- ベータ品質であることを名称・本文へ明記します。最新Releaseから入手できるようprereleaseではなくlatestを指定しますが、Store正式版を意味しません。
- 公開後は両拠点から認証なしで再取得してローカルの長さとSHA-256に照合し、XML参照版・参照URL・署名・タグを記録します。
- Web側は独立したdownloadリポジトリで対象ファイルだけコミットし、親・他製品の変更を混ぜません。

## v0.0.3.5公開結果

MSIX: 2,551,843バイト、SHA-256 `5B7DCF830B306577E559CB6EF450EC29DBD36BCED09DEA2C3D5B1461BD2328BA`。ビルド元・タグは `1b1548eddd17e166cf21aef9d86018d61e8ca95a`。署名・内容・改ざん拒否、両拠点の匿名取得内容一致を検証しました。GitHubの実配信MIMEは引き続きapplication/octet-streamなのでApp Installer更新先はWebです。[本版報告](../dev/history/v0.0.3.5/v0.0.3.5-imp.md)。実機導入・更新は未実施です。

## v0.0.3.4公開結果（履歴）

MSIX: 2,552,234バイト、SHA-256 `C9DD9F33453AC1F5B2DBCE20D5CEAA2127F8196ACCC2738DE30416AEE8C8A2E4`。
署名元コミットは `82168282fc18f02bf1a5e31b75080b46aa5f2b0e`（タグv0.0.3.4）。署名・ペイロード・BlockMap・改ざん拒否を検証し、両配布先の匿名取得内容が一致しました。

実Teamsアカウント・業務PDF・実利用者DB移行・実機のMSIX導入と自動更新は未検証です。今回このPCへ導入せず、証明書の信頼登録も変更していません。[版別報告](../dev/history/v0.0.3.4/v0.0.3.4-imp.md)を参照してください。
