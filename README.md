# HttpSelector

Windowsでリンクを開く前に、Edge・Chrome・Firefoxのブラウザプロファイルを選択するための軽量ランチャーです。

## 主な機能

- http / httpsリンクを開くブラウザプロファイルを毎回選択
- Edge、Chrome、Firefoxの既存プロファイルを自動検出
- お気に入り、エイリアス、コメント、並び順
- インクリメンタル検索とキーボード操作
- URLパターンを既定で保存しない設計
- SQLite設定のDPAPI + HMAC-SHA256完全性チェック

## 対応環境

- Windows 10 1809以降
- Windows 11
- x64

## 開発状況

v0.0.1を開発中です。現時点ではソースからのテスト実行が可能です。

```powershell
cargo test
cargo run -- "https://example.com/path?a=b%20c"
```

詳細は[利用ガイド](docs/USERS_GUIDE.md)と[開発ガイド](docs/DEVELOPERS_GUIDE.md)を参照してください。

## プライバシーとセキュリティ

URLパターンは既定で保存しません。記録を明示的に有効にした場合も、生URLではなく末尾・パラメータ値・フラグメントを `*` に置き換えたパターンだけを保存します。URLはブラウザ起動のためだけにプロセス引数として渡されます。ブラウザ実行ファイルパスも設定DBへ保存せず、Windowsレジストリと既知のインストール先から都度解決します。
