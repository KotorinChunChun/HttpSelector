# HttpSelector

Windowsでリンクを開く前に、Edge・Chrome・Firefoxのブラウザプロファイルを選択するための軽量ランチャーです。

## 主な機能

- http / httpsリンクを開くブラウザプロファイルを毎回選択
- Edge、Chrome、Firefoxの既存プロファイルを自動検出
- お気に入り、エイリアス、コメント、並び順
- ダブルクリック・キーボード操作とURLの既定ルール
- URLのドメイン・パス・クエリを個別選択し、自動起動または関連付けの質問省略を設定
- 全角英字・類似文字・大手サービス名の誤字や偽ドメインを開く前に警告
- Quad9へのドメイン照会と、確認できない場合の警告
- KotorinToysからの無表示起動・状態取得・画面表示・正常停止
- URLパターンを既定で保存しない設計
- SQLite設定のDPAPI + HMAC-SHA256完全性チェック

## 対応環境

- Windows 10 1809以降
- Windows 11
- x64

## 開発状況

公開Beta版は **v0.0.3.5**（Cargo `0.0.3+5`）です。TeamsCIDの必須制限をやめ、url内でONにしたドメイン・パス・クエリで振り分けます。安定したパスだけの固定も可能です。CIDがOFFなら変動・欠落を無視します。[修正記録](dev/history/v0.0.3.5/v0.0.3.5-imp.md)。表示版・実MSIX版・App Installer版を0.0.3.5に揃え、GitHubと公開サイトへ公開済みです。

v0.0.3.4で追加した入れ子条件・名称警告の承認も含みます。TeamsCIDを必須とした制限はv0.0.3.5で修正済みです。

公開済み自己署名Beta MSIX **v0.0.3.5** は [GitHub Releases](https://github.com/KotorinChunChun/HttpSelector/releases/latest) から取得できます。[公開サイト](https://excel-chunchun.sakura.ne.jp/download/HttpSelector.html)でも同じ署名済みバイト列を配布しています。Microsoft StoreおよびKotorinToys用Innoインストーラーは未公開です。

[署名付きMSIXの導入手順](docs/INSTALL_MSIX.md) ／ [GitHub・公開サイトの配布手順](docs/GITHUB_RELEASE.md)。導入と自動更新には公開サイトのApp Installerを利用してください。GitHubからのMSIX直接取得も可能です。実機での導入・更新操作は今回実施していません。

独立Gitを保持したまま、KotorinToysの `tools/HttpSelector` で管理します。**通常利用は署名付きMSIXからのインストールが必須です。** [MSIX導入手順](docs/INSTALL_MSIX.md)に従って導入し、WindowsでHTTP/HTTPSを関連付けてください。KotorinToysも登録済みのMSIX版を管理します。[KotorinToys連携](docs/kotorintoys-integration.md)と[本版の実装記録](dev/history/v0.0.3.3/v0.0.3.3-imp.md)を参照してください。

```powershell
cd C:\develop\KotorinToys\tools\HttpSelector
cargo test
pwsh -NoProfile -File .\dev\scripts\run-test.ps1 -Url "https://example.com/path?a=b%20c"
```

上記のEXE直接起動は開発検証専用で、MSIXによる関連付け登録を代替しません。

詳細は[利用ガイド](docs/USERS_GUIDE.md)と[開発ガイド](docs/DEVELOPERS_GUIDE.md)を参照してください。

MSIXの生成・検証、開発証明書、Store用Identityの設定は[Microsoft Store公開準備](docs/MICROSOFT_STORE.md)を参照してください。開発用MSIXと正式版は設定データを分離します。

## プライバシーとセキュリティ

新しい連携フォームでは既定はドメインのみ固定し、パス・クエリはOFFです。ONにした部分の文字列と生成済み正規表現だけを保存します。`tenantId`等をONにすると、その名前と値も端末内DBへ保存されます。「保存」は設定だけを保存して折りたたみを閉じます。URLを開くには続けて「開く」を押してください。

URLパターンの自動記録は既定でOFFです。有効にした場合も、自動記録では末尾・パラメータ値・フラグメントを `*` に置き換えます。既定ルールの新規登録初期値と「ワイルドカード」でも、URLの認証情報・末尾パス要素・クエリ値・フラグメントを `*` に置き換えます。保存前に一致範囲と残る情報を確認してください。既存ルールや手動入力はそのまま保存され、DB自体は暗号化していません。

開く直前に端末内のパターン判定とQuad9へのDNS over HTTPS照会を行います。Quad9へ送るのはサブドメインを含むASCIIホスト名で、元URL、認証情報、ポート、パス、クエリ、フラグメントは送りません。警告を表示する前に照会が行われる場合があります。Quad9は接続元IPなどの通信情報を認識できるため、匿名通信ではありません。ローカル名、IPリテラル、公開suffixを確認できない名前は外部照会の対象外です。WindowsのDNS設定は変更しません。

危険性が疑われる場合と照会できない場合は、キャンセルを初期選択にした確認画面を表示します。ルール一致や割り込みオフでも適用し、明示的に「それでも開く」を選ぶまで、ブラウザ起動・ドメイン連携・履歴記録を行いません。「ブロックなし」は安全保証ではなく、短縮URLの展開、対象ページの取得、リダイレクト追跡は行いません。検査用の永続URLログは追加していません。詳しくは[プライバシーポリシー案](docs/PRIVACY_POLICY.md)を参照してください。

ブラウザには受け取った元URLを起動引数として渡します。ブラウザ実行ファイルパスは設定DBへ保存せず、Windowsレジストリと既知のインストール先から都度解決します。
