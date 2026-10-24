# HttpSelector v0.0.3.9（署名付きMSIX・ベータ版）

表示・MSIX・App Installerのバージョンはすべて **0.0.3.9** です。

## 変更内容

- 設定・お気に入り・関連付け・履歴・検出結果などの保存処理を共通化しました。保存または署名に失敗した場合は、更新前の署名付きバックアップへ復元します。
- URLを処理するセッション、Win32の画面処理、URL転送、設定値の読み書きなどの重複を整理しました。プロファイル選択・自動振り分け・Shiftでの登録先変更・ゲスト・元URL転送の仕様を維持します。
- CLIから機能試験を一括実行できるようにしました。通常の機能試験、実EXEの無表示起動/状態取得/正常停止、明示実行する画面表示/ESC/前面化、配布スクリプトの検証を行い、JSON・JUnitへ記録します。
- 試験計画・実行・結果解析・レポート生成を分離し、ケースの欠落・重複・未実施を検出します。

## 導入と更新

[公開サイト・App Installer](https://excel-chunchun.sakura.ne.jp/download/HttpSelector.html) / [証明書の導入手順](https://excel-chunchun.sakura.ne.jp/download/httpselector-certificate/index.html) / [詳細手順](https://github.com/KotorinChunChun/HttpSelector/blob/main/docs/INSTALL_MSIX.md)

- 通常利用は署名付きMSIXから導入してください。GitHubにも同じMSIX・公開CER・App Installer・SHA256SUMS.txtを配布します。App Installerの更新先は公開サイトです。
- 発行元: `CN=KotorinChunChun`。証明書拇印: `475BABC3C3D1E871D90503D45CC5236216D85EE8`。秘密鍵・PFXは配布しません。
- Windows 10 1809以降 / Windows 11、x64。Package FamilyとDB schema 5を維持します。
- 更新前にHttpSelectorを正常終了してください。同一Familyの0.0.3.8以前から更新できます。旧1.x・誤公開0.0.4.0からは自動更新できません。
- Quad9への照会は既定OFFで、利用者が明示的にONにした場合だけドメイン名を送信します。OFFでも端末内の検査は行います。
- 正式版はMicrosoft Store掲載予定です。

## 検証範囲

CLI機能試験・署名・内容検証と公開後の照合結果を追記します。実ブラウザーの表示/サインイン状態、MSIXの実機導入・自動更新、実Quad9照会は今回のリリース作業では確認しません。本PCのアプリは更新しません。
