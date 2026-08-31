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

v0.0.1を開発中です。Microsoft Store向けMSIXの準備を進めており、ソースからの実行と開発用自己署名MSIXの生成が可能です。Storeでの公開はまだ行っていません。

```powershell
cargo test
cargo run -- "https://example.com/path?a=b%20c"
```

詳細は[利用ガイド](docs/USERS_GUIDE.md)と[開発ガイド](docs/DEVELOPERS_GUIDE.md)を参照してください。

MSIXの生成・検証、開発証明書、Store用Identityの設定は[Microsoft Store公開準備](docs/MICROSOFT_STORE.md)を参照してください。開発用MSIXと正式版は設定データを分離します。

## プライバシーとセキュリティ

URLパターンの自動記録は既定でOFFです。有効にした場合も、自動記録では末尾・パラメータ値・フラグメントを `*` に置き換えます。一方、利用者が登録する既定ルールは入力したパターンをそのまま保存します。現在の登録画面の初期値には元URLが入るため、保存前に機微な値が含まれていないか確認してください。DB自体は暗号化していません。

ブラウザには受け取った元URLを起動引数として渡します。ブラウザ実行ファイルパスは設定DBへ保存せず、Windowsレジストリと既知のインストール先から都度解決します。
