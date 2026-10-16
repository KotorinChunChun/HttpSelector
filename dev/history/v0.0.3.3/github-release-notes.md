# HttpSelector v0.0.3.3（ベータ版）

表示バージョン・MSIXバージョンはともに **0.0.3.3** です。関連付け登録のため、署名付きMSIXから導入してください。EXE単体配布は行いません。

## 主な変更

- ドメイン連携確認を独自フォームに変更。URLのドメイン・パス・クエリを個別に固定できます。既定はドメインのみONです。
- 「保存」は設定だけ保存し、「開く」と分離しました。ボタンのデザインを統一しました。
- 高リスクURLのローカル検査とQuad9へのホスト名のみの照会、KotorinToysからの署名付きMSIX管理に対応しました。
- MSIXの入手元をGitHubにも用意しました。自動更新と旧サイト切替は配信MIMEの問題で保留中です。既存の旧Webサイト更新経路は変更していません。

## 導入

[署名確認・インストール手順](https://github.com/KotorinChunChun/HttpSelector/blob/main/docs/INSTALL_MSIX.md)を確認し、Assetsから公開証明書・チェックサム・MSIX本体を取得して手動で導入・更新してください。

**App Installerについて:** 正しいMIMEを指定してアセットを登録しましたが、GitHubの実配信はすべて `application/octet-stream` です。[Microsoftの配信要件](https://learn.microsoft.com/en-us/windows/msix/app-installer/troubleshoot-appinstaller-issues)との違いを検出したため、`HttpSelector.appinstaller` は移行検証用とし、自動更新には現時点で推奨しません。旧サイトの更新経路は維持しています。

- 発行元: `CN=KotorinChunChun`
- 証明書拇印: `475BABC3C3D1E871D90503D45CC5236216D85EE8`
- MSIX SHA-256: `B5E8D8FA7B60CB81DC9D82BF0DA1CFE021BA48363711B3A6F71ADE41FE9DC0F4`
- MSIXビルド元: `f91374488aa32122f12e3c146661ab25ab1027f2`。旧Web公開済みと同一の署名済みファイルです。Releaseタグには配布先移行用の資料・スクリプト更新も含みます。

## 注意事項

自己署名のベータ配布です。初回は発行元と拇印を確認し、証明書をローカルコンピューターの「信頼されたユーザー」へ登録してください。秘密鍵は含みません。

0.0.3.1/0.0.3.2と同じPackage Familyです。設定DBは初回起動時にschema 4へ移行し、旧版へ戻せません。旧1.xや誤公開0.0.4.0からは番号が下がるため自動更新できません。必要な設定を退避し、対象旧版だけを削除してから新規導入してください。

Quad9へ送るのはサブドメインを含むホスト名だけで、元URL・パス・クエリは送りません。警告がないことは安全の保証ではありません。連携条件として選んだ値は端末内DBへ保存され、DB自体は暗号化していません。

署名・パッケージ内容を検証済み。Rustテスト103件成功、既存外部照会1件ignore。実機へのインストール・上書き更新・旧App Installerからの自動移行は今回未検証です。GitHubからMSIXをダウンロードする手動導入・更新を推奨します。
