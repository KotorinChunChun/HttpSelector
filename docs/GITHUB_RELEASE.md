# GitHubでの配布・更新

2026-10-21時点の開発ソースは0.0.3.4で、この版のpush・MSIX作成・Release公開は未実施です。以下の公開アセットと配信検証は0.0.3.3に対する記録です。

2026-10-17から配布の正本を [KotorinChunChun/HttpSelector](https://github.com/KotorinChunChun/HttpSelector) に移します。通常利用には署名付きMSIXが必須です。

## 現在の制約

GitHub Releaseの公開アセットは、API登録時に正しいMIMEを指定しても実際のHEAD/GETではすべて `application/octet-stream` でした。MicrosoftのApp Installer配信要件は正しいMIME・長さを必要とするため、GitHubへの自動更新移行は完了としていません。現時点ではMSIXをダウンロードして手動導入・更新してください。旧Webサイトの変更・既存更新経路の切替は実施せず維持しています。以下のApp Installer・旧サイト切替の記述は準備済みの移行案であり、動作保証・実施済みではありません。

## 利用者向け

1. [最新Release](https://github.com/KotorinChunChun/HttpSelector/releases/latest)で版と注意事項を確認します。
2. `HttpSelector.Development.cer` と `SHA256SUMS.txt` を取得し、[署名の確認・導入手順](INSTALL_MSIX.md)に従います。拇印は `475BABC3C3D1E871D90503D45CC5236216D85EE8`、発行元は `CN=KotorinChunChun` です。
3. MSIX本体をダウンロードして開きます。GitHub上の [HttpSelector.appinstaller](https://github.com/KotorinChunChun/HttpSelector/releases/latest/download/HttpSelector.appinstaller) は移行検証用です。実機試験は未実施で、自動更新には現時点で使用を推奨しません。MSIXの直接導入とApp Installerの更新登録は別です。

## 旧サイト移行案（未実施）

旧Webサイトの製品ページ・証明書ページは移行案内に置き換える準備をしました。旧 `beta/HttpSelector.appinstaller` のXMLのUriをGitHubの固定URLへ変更する案は、MIMEの問題を解決し、実機で確認するまで公開しません。既に同版を導入している環境で更新先が自動的に切り替わることも実機未検証です。設定の自動削除やアンインストールは行いません。

過去のMSIX・CERは破壊的に削除せず、旧ページからの配布リンクを終了します。共有ダウンロードメニューや他製品は変更しません。

## 保守者向け

- `packaging/release-config.json` のMSIX版と表示版を一致させます。`beta.baseUri` は版付きGitHub Release、`beta.appInstallerUri` は `releases/latest/download/HttpSelector.appinstaller` とします。
- MSIX生成・署名・検証後、`dev/scripts/build-appinstaller.ps1` を実行します。同版MSIXの再生成・差し替えは禁止し、既存の署名済みバイト列を再利用します。
- GitHub Releaseはdraftで準備し、MSIX、公開CER、App Installer、SHA256SUMS.txtをアップロードしてから公開します。秘密鍵・PFX・ユーザーデータ・実行ログは公開しません。
- GitHub APIのアセットアップロード時にMSIXは `application/msix`、App Installerは `application/appinstaller`、CERは `application/pkix-cert`、チェックサムは `text/plain` を指定します。公開URLのGET/HEAD、MIME・長さ・SHA-256とXML参照を検査します。
- ベータ品質であることを名称・本文へ明記しつつ、固定 `latest/download` を更新経路に使うため、GitHub上ではprereleaseフラグを使わずlatest Releaseに指定します。Store正式版を意味しません。
- 旧サイトへの通常Publishは停止します。移行案内更新だけ `publish-httpselector-ftps.py --migration --dry-run` → `--migration --execute` を使い、案内2ファイルと旧App InstallerだけをFTPSでバックアップ付き更新します。

App InstallerのUriによる移行動作は[Microsoftの仕様](https://learn.microsoft.com/en-us/uwp/schemas/appinstallerschema/element-appinstaller)、配信時のMIME・長さ要件は[Microsoftのトラブルシューティング](https://learn.microsoft.com/en-us/windows/msix/app-installer/troubleshoot-appinstaller-issues)を参照してください。HTTPS配信検証と実機インストール・更新の証明は区別します。
