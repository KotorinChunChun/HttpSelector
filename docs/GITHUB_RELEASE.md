# GitHubでの配布・更新

2026-10-17から配布の正本を [KotorinChunChun/HttpSelector](https://github.com/KotorinChunChun/HttpSelector) に移します。通常利用には署名付きMSIXが必須です。

## 利用者向け

1. [最新Release](https://github.com/KotorinChunChun/HttpSelector/releases/latest)で版と注意事項を確認します。
2. `HttpSelector.Development.cer` と `SHA256SUMS.txt` を取得し、[署名の確認・導入手順](INSTALL_MSIX.md)に従います。拇印は `475BABC3C3D1E871D90503D45CC5236216D85EE8`、発行元は `CN=KotorinChunChun` です。
3. 自動更新を使う場合は [HttpSelector.appinstaller](https://github.com/KotorinChunChun/HttpSelector/releases/latest/download/HttpSelector.appinstaller) をダウンロードして開きます。MSIXの直接導入は可能ですが、App Installerの更新登録とは別です。

旧Webサイトの製品ページ・証明書ページは移行案内に置き換えます。既存利用者向けに旧 `beta/HttpSelector.appinstaller` を残し、XMLのUriをGitHubの固定URLへ変更します。既に同版を導入している環境で更新先が自動的に切り替わることは実機未検証です。確実に移行するにはGitHubのApp Installerをダウンロードして開き、Windowsの確認画面に従ってください。設定の自動削除やアンインストールは行いません。

過去のMSIX・CERは破壊的に削除せず、旧ページからの配布リンクを終了します。共有ダウンロードメニューや他製品は変更しません。

## 保守者向け

- `packaging/release-config.json` のMSIX版と表示版を一致させます。`beta.baseUri` は版付きGitHub Release、`beta.appInstallerUri` は `releases/latest/download/HttpSelector.appinstaller` とします。
- MSIX生成・署名・検証後、`dev/scripts/build-appinstaller.ps1` を実行します。同版MSIXの再生成・差し替えは禁止し、既存の署名済みバイト列を再利用します。
- GitHub Releaseはdraftで準備し、MSIX、公開CER、App Installer、SHA256SUMS.txtをアップロードしてから公開します。秘密鍵・PFX・ユーザーデータ・実行ログは公開しません。
- GitHub APIのアセットアップロード時にMSIXは `application/msix`、App Installerは `application/appinstaller`、CERは `application/pkix-cert`、チェックサムは `text/plain` を指定します。公開URLのGET/HEAD、MIME・長さ・SHA-256とXML参照を検査します。
- ベータ品質であることを名称・本文へ明記しつつ、固定 `latest/download` を更新経路に使うため、GitHub上ではprereleaseフラグを使わずlatest Releaseに指定します。Store正式版を意味しません。
- 旧サイトへの通常Publishは停止します。移行案内更新だけ `publish-httpselector-ftps.py --migration --dry-run` → `--migration --execute` を使い、案内2ファイルと旧App InstallerだけをFTPSでバックアップ付き更新します。

App InstallerのUriによる移行動作は[Microsoftの仕様](https://learn.microsoft.com/en-us/uwp/schemas/appinstallerschema/element-appinstaller)、配信時のMIME・長さ要件は[Microsoftのトラブルシューティング](https://learn.microsoft.com/en-us/windows/msix/app-installer/troubleshoot-appinstaller-issues)を参照してください。HTTPS配信検証と実機インストール・更新の証明は区別します。
