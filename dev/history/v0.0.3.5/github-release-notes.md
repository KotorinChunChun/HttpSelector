# HttpSelector v0.0.3.5（署名付きMSIX・ベータ版）

表示バージョン・MSIXバージョン・App Installerバージョンはすべて **0.0.3.5** です。

## 修正内容

Teamsのリンク登録で「SharePointドメインとTeamsCIDをONにしてください」と拒否される制限を修正しました。保存時だけでなく、自動振り分け時の必須判定も見直しています。

- url内のドメイン・パス・クエリのうち、ONにした部分だけで振り分けます。個人領域などの安定したパスだけでも登録できます。ドメインをOFFにすると他ホストの同じ位置のパスにも一致するため、必要ならドメインもONにしてください。
- TeamsCIDは任意です。通常はOFFにし、変動・欠落を条件に含めない使い方ができます。
- 既存のCID固定ルールは自動的に広げません。不要な固定条件は利用者が見直すか、削除して登録し直してください。
- 外側の共通CDNだけの広い自動振り分けは拒否し、複数プロファイルの条件競合時は手動選択に戻します。
- 元のSafe Linksと高リスクURL検査は維持します。同じURL条件を共有する複数Teamsアカウントを認証・識別する機能ではありません。

## 導入

HTTP/HTTPSの関連付け登録のため、通常利用には署名付きMSIXからのインストールが必須です。

[公開サイト・App Installer](https://excel-chunchun.sakura.ne.jp/download/HttpSelector.html) / [証明書の導入手順](https://excel-chunchun.sakura.ne.jp/download/httpselector-certificate/index.html) / [詳細手順](https://github.com/KotorinChunChun/HttpSelector/blob/main/docs/INSTALL_MSIX.md)

GitHubのAssetsにも同じMSIX・公開CER・App Installer・SHA256SUMS.txtを掲載します。App Installerの更新先は適切な配信MIMEを提供する上記サイトです。GitHubからのMSIX直接取得も可能です。

- 発行元: `CN=KotorinChunChun`
- 証明書拇印: `475BABC3C3D1E871D90503D45CC5236216D85EE8`
- Windows 10 1809以降 / Windows 11、x64。
- 秘密鍵・PFXは配布しません。署名確認を無効化しないでください。

## 更新と検証範囲

同一ベータ系列0.0.3.4からの更新を想定し、DB schema 5は変更しません。旧形式DBは移行され、移行後は0.0.3.3以前で開けません。旧1.xや誤公開0.0.4.0は番号が下がるため自動更新できません。

選択したドメイン・パス・クエリは暗号化されない設定DBに保存されます。URL履歴は既定OFFです。

Rust回帰試験116件、fmt、Clippy、非表示Win32フォームの保存操作を検証済み。実Teamsアカウント・業務PDF・実ユーザーDB・実機のMSIX導入と更新操作は未検証です。警告がないことは安全を保証しません。
