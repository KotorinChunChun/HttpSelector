# HttpSelector v0.0.3.6（署名付きMSIX・ベータ版）

表示・MSIX・App Installerのバージョンはすべて **0.0.3.6** です。

## 追加した機能

- Shiftを押しながらリンクを開くと、登録済みルール・質問省略・割り込みOFFに関係なくプロファイル選択と確認を表示します。URL受付までShiftを保持してください。
- 一意に一致した既存ルールの条件を維持し、「保存」で登録先を置き換えます。重複追加せず、保存だけではURLを開きません。
- 未保存で「戻る」を選ぶと既存登録は変わりません。保存後に戻っても保存内容は残ります。
- 条件は読み取り専用です。複数ルール一致や編集中の変更・削除では誤置換を防ぐため保存を拒否します。必要なら既定ルール一覧で整理してください。
- 元URLと高リスクURL検査を維持します。Shift指定はURLごとの一時属性で、常駐への転送・待ち行列でも保持します。

## 導入と更新

関連付け登録のため、通常利用は署名付きMSIXのインストールが必須です。

[公開サイト・App Installer](https://excel-chunchun.sakura.ne.jp/download/HttpSelector.html) / [証明書導入手順](https://excel-chunchun.sakura.ne.jp/download/httpselector-certificate/index.html) / [詳細手順](https://github.com/KotorinChunChun/HttpSelector/blob/main/docs/INSTALL_MSIX.md)

GitHubでも同じMSIX・公開CER・App Installer・SHA256SUMS.txtを配布します。App Installerの更新先は適切なMIMEを提供する既存公開サイトです。

- 発行元: `CN=KotorinChunChun`
- 証明書拇印: `475BABC3C3D1E871D90503D45CC5236216D85EE8`
- Windows 10 1809以降 / Windows 11、x64。秘密鍵・PFXは配布しません。
- 0.0.3.5と同じPackage Family・DB schema 5です。旧形式から移行したDBは0.0.3.3以前で開けません。旧1.xや誤公開0.0.4.0からの自動更新はできません。
- 更新前の旧プロセスは新しいShift指定のIPCを受け付けません。作業を終えてHttpSelectorを正常終了してから更新してください。

## 検証範囲

Rust回帰試験120件、fmt、Clippy、非表示Win32フォームの条件保持・保存/戻る、隔離DBの同一ID更新・競合拒否を確認済みです。

実際のTeams/ブラウザーからのShiftクリック、コールドスタート時のキーを離すタイミング、実ブラウザー起動・実DB・MSIX導入/自動更新は未検証です。リンク元アプリがShiftクリックを独自処理してURLを渡さない場合は利用できません。警告がないことは安全を保証しません。
