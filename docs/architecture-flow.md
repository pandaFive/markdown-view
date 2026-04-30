# markdown-view 処理フローAST

このドキュメントは `markdown_view::architecture::application_flow()` が表す、
アプリケーション処理フローの概要です。

Mermaidブロックは `application_flow().to_mermaid()` の出力です。手編集せず、
ASTを変更したら出力で差し替えてください。

```mermaid
flowchart TD
  n0["markdown-view"]
  n1["起動フロー"]
  n0 --> n1
  n2["ログ初期化"]
  n1 --> n2
  n3["CLI引数解析"]
  n1 --> n3
  n4["パス検証"]
  n1 --> n4
  n5["モード判定"]
  n1 --> n5
  n6["単一ファイルモード"]
  n5 --> n6
  n7["ディレクトリモード"]
  n5 --> n7
  n8["テーマ検証"]
  n1 --> n8
  n9["AppState作成"]
  n1 --> n9
  n10["監視サービス開始"]
  n1 --> n10
  n11["localhostサーバーbind"]
  n1 --> n11
  n12["ルーター作成"]
  n1 --> n12
  n13["HTTPサーバー起動"]
  n1 --> n13
  n14["HTTPレンダリングフロー"]
  n0 --> n14
  n15["Host検証"]
  n14 --> n15
  n16["ルーティング"]
  n14 --> n16
  n17["Markdown読み込み"]
  n14 --> n17
  n18["render_markdown"]
  n14 --> n18
  n19["Markdownイベント処理"]
  n14 --> n19
  n20["TOC生成"]
  n14 --> n20
  n21["ページHTML生成"]
  n14 --> n21
  n22["HTTPレスポンス"]
  n14 --> n22
  n23["ファイル監視更新フロー"]
  n0 --> n23
  n24["ファイル監視"]
  n23 --> n24
  n25["debounce処理"]
  n23 --> n25
  n26["変更ファイル解決"]
  n23 --> n26
  n27["Markdown再読み込み"]
  n23 --> n27
  n28["render_markdown"]
  n23 --> n28
  n29["TOC再生成"]
  n23 --> n29
  n30["WebSocket broadcast"]
  n23 --> n30
  n31["ブラウザWebSocketフロー"]
  n0 --> n31
  n32["初期HTML読み込み"]
  n31 --> n32
  n33["クライアント初期化"]
  n31 --> n33
  n34["WebSocket接続"]
  n31 --> n34
  n35["更新メッセージ受信"]
  n31 --> n35
  n36["本文差し替え"]
  n31 --> n36
  n37["メモ同期"]
  n31 --> n37
```

## 使い方

Rust側から処理フローを扱う場合は、`application_flow()` でASTを取得し、
`to_mermaid()` でMermaid flowchartに変換できます。

```rust
use markdown_view::architecture::application_flow;

let mermaid = application_flow().to_mermaid();
```

各ノードは名前、対応モジュール、対応関数を保持します。これはRust構文木ではなく、
設計・ドキュメント・レビュー用のアプリケーション意味ASTです。
