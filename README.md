# HttpSelector

Windowsでリンクを開く前に、Edge・Chrome・Firefoxのブラウザプロファイルを選択するための軽量ランチャーです。

## 主な機能

- http / httpsリンクを開くブラウザプロファイルを毎回選択
- Edge、Chrome、Firefoxの既存プロファイルを自動検出
- お気に入り、エイリアス、コメント、並び順
- ダブルクリック・キーボード操作とURLの既定ルール
- URLパターンを既定で保存しない設計
- SQLite設定のDPAPI + HMAC-SHA256完全性チェック

## 対応環境

- Windows 10 1809以降
- Windows 11
- x64

## 開発状況

v0.0.3-beta（MSIX 0.0.3.0）を公開しています。Microsoft Store向けMSIXの準備も進めていますが、Storeでの公開はまだ行っていません。既知の制約と検証結果は[リリース記録](dev/history/v0.0.3/v0.0.3-imp.md)を参照してください。

```powershell
cargo test
cargo run -- "https://example.com/path?a=b%20c"
```

詳細は[利用ガイド](docs/USERS_GUIDE.md)と[開発ガイド](docs/DEVELOPERS_GUIDE.md)を参照してください。

MSIXの生成・検証、開発証明書、Store用Identityの設定は[Microsoft Store公開準備](docs/MICROSOFT_STORE.md)を参照してください。開発用MSIXと正式版は設定データを分離します。

## プライバシーとセキュリティ

URLパターンの自動記録は既定でOFFです。有効にした場合も、自動記録では末尾・パラメータ値・フラグメントを `*` に置き換えます。既定ルールの新規登録初期値と「ワイルドカード」でも、URLの認証情報・末尾パス要素・クエリ値・フラグメントを `*` に置き換えます。保存前に一致範囲と残る情報を確認してください。既存ルールや手動入力はそのまま保存され、DB自体は暗号化していません。

ブラウザには受け取った元URLを起動引数として渡します。ブラウザ実行ファイルパスは設定DBへ保存せず、Windowsレジストリと既知のインストール先から都度解決します。
