# HttpSelector v0.0.3.7（署名付きMSIX・ベータ版）

表示・MSIX・App Installerのバージョンはすべて **0.0.3.7** です。

## 変更内容

- **Quad9へのドメイン照会を既定OFF**にしました。新規・既存設定とも、明示的にONとして保存した記録がなければ送信しません。
- 利用する場合だけ「設定」の「Quad9へドメインを照会する（既定OFF）」をONにして保存してください。チェックだけ、またはキャンセルでは反映しません。OFFにして保存すると以後の照会を停止します。
- OFFでも端末内の類似ドメイン・Unicode等の検査を維持します。外部照会をしないことを確認不能として警告しません。
- 有効時に送るのはドメイン名とDNS問い合わせに必要な情報だけです。URLのパスやパラメーターは送りませんが、送信先には接続元IP等が伝わります。通常URLと内側URLに同じ方針を適用します。
- READMEを仕様中心に整理し、目的を説明する16:9構図のインフォグラフィックを冒頭に掲載しました。

チャット・メール・その他のアプリからのリンクを意図したプロファイルで開くためのツールです。OAuth認証等のワンタイムURL、誤ったプロファイルでのログインや意図しない共有・送信、開き直しの手間を考慮しています。**導入直後は確認が多く、約2クリック増えます。** 操作設定とルール登録により回数は変わります。

## 導入と更新

通常利用は署名付きMSIXからの導入が必須です。[公開サイト・App Installer](https://excel-chunchun.sakura.ne.jp/download/HttpSelector.html) / [証明書の導入手順](https://excel-chunchun.sakura.ne.jp/download/httpselector-certificate/index.html) / [詳細手順](https://github.com/KotorinChunChun/HttpSelector/blob/main/docs/INSTALL_MSIX.md)

GitHubにも同じMSIX・公開CER・App Installer・SHA256SUMS.txtを配布します。App Installerの更新先は適切なMIMEを提供する公開サイトです。正式版はMicrosoft Store掲載予定です。

- 発行元: `CN=KotorinChunChun`
- 証明書拇印: `475BABC3C3D1E871D90503D45CC5236216D85EE8`。秘密鍵・PFXは配布しません。
- Windows 10 1809以降 / Windows 11、x64。
- 同じPackage Family・DB schema 5を維持します。既存設定にQuad9への同意がなければOFFです。既存の保存ルールや承認済みドメインは維持します。
- 更新前に作業を終え、HttpSelectorを正常終了してください。0.0.3.6以前の同一Familyから更新できます。旧1.x・誤公開0.0.4.0からは自動更新できません。

## 検証範囲

Rust試験125件、fmt、全ターゲットのClippy、OFF時の通信関数呼出0回、既存設定の既定OFF、設定の保存・キャンセル・解除、非表示Win32フォームと説明文の配置を確認しました。実Quad9照会は行っていません。

実画面の目視操作、実ブラウザー起動、実利用者DB、MSIXの実機導入・自動更新は今回の検証対象外です。警告がないことは安全を保証しません。

署名・パッケージ内容・改ざん拒否を検証し、GitHubと公開サイトから認証なしで再取得して一致を確認しました。MSIXは2,561,127バイト、SHA-256 `AA1F9FEA7D6DFEB9D469FF84CCA7963E2E26763CDFCE949B5424E7C13A291243`。ビルド元・タグは `5aec42a6d236ffa96511b0c9e2f4beefc8f09770` です。
