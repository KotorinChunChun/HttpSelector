# Microsoft Store公開準備

HttpSelectorの正式配布はMicrosoft StoreのMSIXを予定する。開発用MSIXとStore提出用MSIXを分離し、Storeの更新機構を利用する。既存のInno Setupは従来方式として残しているが、Store提出には使わない。

## 現在の構成

|項目|beta（ローカル開発用）|stable（Store提出用）|
|---|---|---|
|Identity Name|HttpSelector.Development|Partner Centerの値を設定|
|Publisher|CN=KotorinChunChun|Partner Centerの値を設定|
|署名|開発者PCの永続自己署名証明書|未署名で生成し、Storeが署名|
|証明書|CurrentUser\My、PC共通設定の拇印で固定|開発証明書は使用しない|
|更新|検証用途だけ。同一IdentityでPackage Versionを上げる|Microsoft Store|
|保存先|自身のPackage FamilyのLocalState|自身のPackage FamilyのLocalState|

betaは公開ベータ配布チャネルではなくローカル検証用の名称。App Installer、Web/FTPへの公開、独自アップデーターは作らない。開発用SubjectはPC共通の`CN=KotorinChunChun`を使う。Storeの公開情報は実際のPartner Center画面で確認する。匿名性を保証するものではない。

MSIXの版は `packaging/release-config.json` の `packageVersion` で管理する。初期値は **1.0.1.0**。アプリ本体の `Cargo.toml` は **0.0.1** のまま。MSIXは先頭1以上、各部65535以下、4桁目0とし、更新時は前回より増やす。両者のバージョン番号を機械的に同一視しない。

## 前提

- Windows x64、PowerShell 7.4以降（`pwsh`）。
- RustのMSVC x64ツールチェーン、Visual Studio C++ Build Tools、Windows SDK。
- SDKのMakeAppxとSignToolを使用。最新のインストール済みSDKを検索する。
- 公開用パッケージはx64のみ。必要OSはWindows 10 1809以降。Windows 11以外の実機検証は別途必要。

## 開発用の生成と検証

プロジェクトのルートで実行する。

```powershell
pwsh -NoProfile -File dev/scripts/release-msix.ps1 -Mode Plan -Channel beta
pwsh -NoProfile -File dev/scripts/test-msix-scripts.ps1
pwsh -NoProfile -File dev/scripts/release-msix.ps1 -Mode Build -Channel beta -ReleaseIntentConfirmed
pwsh -NoProfile -File dev/scripts/release-msix.ps1 -Mode Verify -Channel beta
```

Buildはfmt、Rustテスト、Clippy、static CRTのreleaseビルド、画像生成、MakeAppxのManifest検証、署名、内容検証を順に行う。途中の外部コマンドが失敗したら停止し、前回のrelease成果物は検証成功まで変更しない。ビルド元commit、作業ツリーの変更有無、Cargo.lockと成果物のSHA-256を検証JSONへ記録する。

出力は `release/msix/beta/1.0.1.0/`。MSIX、公開CER、SHA256SUMS.txt、verification.jsonを出力する。MSIXの中身は実行ファイル、3枚の画像、ManifestとMSIXメタデータだけ。ソース、PDB、PFX、証明書の秘密鍵、開発用文書は含めない。コンパイル時にローカルパスを置換し、配布物内の代表的な個人パスも検出する。

同じ版の出力先が存在する場合は停止する。再現確認で作り直す場合だけBuildへ `-AllowRebuild` を追加する。同じ版を新しい更新として配布してはいけない。日時・証明書・ツールチェーン等によりMSIXのバイト列が毎回一致する保証はない。

Verifyは既存パッケージを読むだけで、生成・修復・インストール・信頼登録をしない。構成の許可リスト、Identity、起動引数、Capability、BlockMapの全ファイルハッシュ、署名者とCERの一致、有効期限、Windowsの署名検証を確認する。信頼未登録の自己署名証明書では `UntrustedDevelopmentCertificate` と明記し、インストール可能な状態と誤認しない。署名の暗号学的検査とWindowsの信頼判定は区別する。

署名時にSignToolが追加する `AppxMetadata/CodeIntegrity.cat` も検証対象とする。署名済みのBlockMap・Content Types・CodeIntegrityのダイジェストを実ファイルと照合し、実行ファイルとBlockMapを同時に改変したケースも拒否する。既存の署名済み成果物を使う追加テストは、`test-msix-scripts.ps1 -SignedPackagePath <MSIXのパス>` で実行できる（対象は変更せず、target内のコピーだけを改変する）。

## 証明書の扱い

2026-08-31の共通化後は、`%LOCALAPPDATA%\KotorinChunChun\MSIX\signing.json`で指定した既存証明書を使う。拇印は`475BABC3C3D1E871D90503D45CC5236216D85EE8`、Subjectは`CN=KotorinChunChun`、有効期限は2031-08-10。Code Signing用途、RSA 3072 bit。設定・証明書・秘密鍵の欠落、期限まで7日以下、Publisher不一致では停止する。自動発行や個別PFXへの切替はしない。

秘密鍵はWindowsに保持し、PFXを出力しない。共通キーの元のエクスポート可能属性は変更しない。新規アプリ用の共通手順は`C:\develop\default\dev\templates\MSIX_SIGNING.md`を参照する。

旧開発版のPublisherは`CN=HttpSelector Development`だった。共通Publisherへの切替でPackage Familyが変わるため、旧版への上書き更新・LocalStateの自動共有はできない。旧1.0.1.0の成果物と証明書は保存し、次回はpackageVersionを上げて新規インストールと設定移行を別環境で確認する。現在の設定を使ったVerifyは旧成果物のIdentity不一致を正しく検出する。今回、インストール済みアプリや設定は変更しない。
約2か月後に署名方針が決まっても、この開発用Publisherを正式Identityへ流用しない。Store経由なら開発者による有料コード署名証明書は不要。Store外での正式配布を追加する場合は別途、証明書Subject、Identity、既存インストールからの移行を設計する。

開発証明書にはタイムスタンプを付けない。期限後は再インストール等ができなくなる可能性があるため、長期配布には使わない。証明書を紛失したPCへの再作成や鍵の変更は、既存インストールとの更新互換性を確認してから行う。

## インストール検証（この準備作業では未実施）

クリーンVMまたは検証用Windowsアカウントを推奨する。信頼登録はこの証明書で署名されたコードを信頼する操作なので、生成元と証明書の拇印を確認してから行う。証明書をRootへ入れる手順は使わない。

1. `verification.json` の署名者拇印とCERを照合する。MSIXのSHA-256も照合する。
2. 検証端末の管理者PowerShell 7で、CERを **LocalMachine\TrustedPeople** に登録する。秘密鍵は不要。
3. 通常ユーザーのPowerShell 7で `Add-AppxPackage -Path <MSIXのパス>` を実行する。
4. スタートメニューの「HttpSelector (開発用)」を起動し、初回検出・保存・再起動を確認する。
5. Windowsの既定のアプリ設定でHTTP/HTTPSの候補を確認し、検証時だけ利用者が明示選択する。選択前の既定アプリを記録し、検証後に戻す。
6. 別アプリから日本語、クエリ、`&`、`%20`、`#` を含むHTTP/HTTPSリンクを渡し、URL原文が保たれることを確認する。起動中の画面へのURL転送、各ブラウザープロファイル起動も確認する。
7. 非MSIX版との同時起動でデータやURL転送が混ざらないことを確認する。
8. 同一証明書・同一IdentityでPackage Versionを上げ、上書き更新後に設定が残ることを確認する。
9. アプリ削除時の候補登録とLocalStateの扱いを確認する。最後に検証で追加した証明書だけを拇印で特定して信頼ストアから取り除く。

登録例（検証端末で明示的に実行する。管理者権限が必要なのは信頼登録の操作であり、アプリ本体は要求しない）:

```powershell
Import-Certificate -FilePath <公開CERのパス> -CertStoreLocation Cert:\LocalMachine\TrustedPeople
```

非MSIX版のデータは `%LOCALAPPDATA%\HttpSelector`、MSIX版は `%LOCALAPPDATA%\Packages\<PackageFamilyName>\LocalState`。自動移行はしない。HMAC鍵はWindowsユーザーに結び付くため、DBだけを別ユーザーへコピーしても利用できない。旧版からの正式な移行が必要なら公開前に移行方法を別途決める。

## Store提出版

1. 個人Partner Centerアカウントの確認・アプリ名の予約を行う。公開名や問い合わせ先はブランド用に準備する。
2. `packaging/store-identity.example.json` を `packaging/store-identity.local.json` へコピーする（後者はGit対象外）。`name`、`publisher`、`publisherDisplayName` はPartner Centerの製品IDの値をそのまま入力し、`displayName` は予約した製品名と整合させる。
3. `-Mode Plan -Channel stable` で解決値を確認する。空欄・開発用Identityは拒否する。
4. `-Mode Build -Channel stable -ReleaseIntentConfirmed` で `release/msix/stable/<版>/HttpSelector_<版>_x64_store-unsigned.msix` を生成する。
5. `-Mode Verify -Channel stable` で確認する。この未署名MSIXはStore提出用であり、そのまま通常のローカルインストールに使わない。未署名ローカル導入専用の特殊OIDをPublisherへ追加しない。
6. Windows App Certification Kit（WACK）とクリーン環境での受け入れ検証を実施する。Storeのフライト等、正式な署名済み配布経路で起動・関連付け・更新を再確認する。
7. 公開スクリーンショット、説明、カテゴリ、対象年齢、プライバシーポリシーURL、サポートURL/ブランド連絡先、`runFullTrust` 説明を準備し、Partner Centerで申請する。

### runFullTrustの審査説明案

HttpSelectorは、利用者がHTTP/HTTPSリンクを開くブラウザーとプロファイルを選ぶWin32ユーティリティです。インストール済みのEdge、Chrome、Firefoxのプロファイル構成とブラウザーの登録情報を読み取り、選択されたブラウザーをその利用者の権限で起動します。管理者権限への昇格、サービス、ドライバー、既定アプリの強制変更は行いません。ファイルの書き込みは原則として自身のLocalStateに限定します。独自の更新処理やテレメトリー送信は行いません。

HTTP/HTTPS候補はpackaged full-trust向けの `uap3:Protocol` で宣言する。UWP向けの予約スキーム制約と混同しない。ただし、ブラウザー本体ではない本アプリのStore審査通過や各Windows版での候補表示は、このManifestの生成成功だけでは保証できない。

## 参照資料

- [MSIXの署名とStoreによる署名](https://learn.microsoft.com/en-us/windows/msix/package/sign-msix-package-guide)
- [Storeのパッケージ要件とバージョン規則](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-package-requirements)
- [Win32パッケージのプロトコル関連付け](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/desktop-to-uwp-extensions)
- [Firefoxの公開MSIX ManifestのHTTP/HTTPS宣言](https://searchfox.org/firefox-main/source/browser/installer/windows/msix/AppxManifest.xml.in)
- [MSIX環境でのファイル・レジストリの扱い](https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-behind-the-scenes)

確認日: 2026-08-31。Storeの画面・要件・審査は変更される場合があるため提出前に再確認する。
