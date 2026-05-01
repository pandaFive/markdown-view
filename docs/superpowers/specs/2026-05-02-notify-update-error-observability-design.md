# notify_update receiver=0 エラー観測性改善 設計

## 背景

`notify_update` は WebSocket 受信者が 0 の場合、`build_change_broadcast_message` を呼ぶ前に return する。これにより、通常の `Update` 生成コストは避けられている。一方で、ファイル削除、読込失敗、検証失敗から生成される `BroadcastMessage::Error` も生成されず、既存の warn ログ経路にも入らない。

監視ライブラリ由来の `WatchEvent::Error` は `broadcast_error` を通り、受信者の有無に関わらず送信を試み、送信失敗は warn に残る。現在の問題は、ファイル変更イベントから派生した Error だけが、受信者 0 のとき silent drop される点にある。

## 目的

- WebSocket 受信者が 0 のときでも、ファイル変更イベントから派生する検証・読込エラーをローカル warn ログで観測できるようにする。
- 受信者がいる場合の WebSocket 配送仕様は変えない。
- 受信者が 0 の正常更新では、従来通り Markdown 本文読込・描画コストを避ける。

## 非目的

- watcher 健全性 API の追加。
- WebSocket メッセージ形式の変更。
- 通常更新の receiver=0 時ログ追加。
- `build_change_broadcast_message` 全体の結果型再設計。
- renderer、TOC、検索のパース回数削減。

## 採用方針

`notify_update` の receiver=0 分岐に、エラーだけを観測する補助経路を追加する。

receiver がいる場合は現行通り `build_change_broadcast_message(state, changed_file).await` の結果を `send_broadcast_message` に渡す。receiver が 0 の場合は新しい補助関数を呼び、ターゲット解決と軽量なファイル読込前検査だけを実行する。検証失敗、ファイル削除、metadata/open 失敗、サイズ超過のように本文読込前に分かる異常だけ warn ログに残す。

この方針により、通常更新の receiver=0 fast path は維持しつつ、運用上よく起きる削除・検証・サイズ系の異常が silent にならない。非 UTF-8 のように本文読込後にしか分からないエラーは、receiver=0 時の観測対象外とする。

## コンポーネント

### `notify_update`

責務は、受信者有無に応じた更新通知の配送またはエラー観測の分岐に限定する。

- receiver が 1 以上: 既存通りメッセージ生成と broadcast 送信を行う。
- receiver が 0: エラー観測用ヘルパーを呼んで return する。

### エラー観測ヘルパー

新しい private async 関数 `log_change_error_without_receivers` を `src/server/broadcast.rs` に追加する。

この関数は `build_change_broadcast_message` を呼ばない。代わりに、既存の変更ターゲット解決と同じ分類を使う軽量ヘルパーで、本文読込前に分かるエラーだけを検出する。ログには、受信者がいないため WebSocket 送信しなかったことと、既存エラーメッセージ形式に揃えた分類を含める。

本文 HTML、Markdown 本文、未サニタイズの外部入力パスはログに出さない。

### 既存メッセージ生成

`build_change_broadcast_message` は変更しない。receiver がいる場合のエラー文言、パス表示、検証・読込失敗の分類は既存の経路を維持する。

receiver=0 用には、本文読込を避ける軽量ヘルパーを `src/server/files/content.rs` に追加する。このヘルパーは以下だけを行う。

- `resolve_change_target` による既存の変更ターゲット解決。
- `tokio::fs::metadata` による存在確認とサイズ上限確認。
- metadata 成功後、必要最小限の `tokio::fs::File::open` による open 可否確認。

この helper は Markdown 本文を読まず、`render_markdown` と `generate_toc` を呼ばない。戻り値は `Option<BroadcastMessage>` ではなく、ログ用の `Option<String>` とし、送信用メッセージ生成と混同しない。

## データフロー

1. watcher が `WatchEvent::FileChanged(changed_path)` を forwarder に渡す。
2. forwarder が `notify_update(&state, &changed_path).await` を呼ぶ。
3. `notify_update` が `state.tx().receiver_count()` を確認する。
4. receiver が 0 の場合、エラー観測ヘルパーが変更対象の解決と読込前検査を行う。
5. 結果がログ対象エラーの場合だけ warn ログに記録する。
6. 結果が正常またはディレクトリモードの対象なしの場合は何もしない。

## エラーハンドリング

- 検証失敗の分類は `build_change_broadcast_message` の既存実装と同じ `resolve_change_target` の結果に従う。
- 読込前検査の失敗は、既存の `ReadMarkdownError::Io` と `ReadMarkdownError::TooLarge` の user message に揃える。
- `ReadMarkdownError::NotUtf8` は本文読込後にしか分からないため receiver=0 時の観測対象外とする。
- 受信者なしログは送信失敗ではないため、既存の `send_broadcast_message` の warn とは別文言にする。
- ログ文言はテストで固定できる短い日本語にする。例: `"[markdown-view] WebSocket受信者がいないためファイル変更エラーをローカル記録しました"`。

## テスト方針

`src/server/broadcast.rs` の既存テストへ追加する。

- receiver=0 かつ変更対象が読めない場合、`notify_update` が warn ログを出す。
- ログには「受信者がいない」ことと、既存エラー分類である `ファイル読み込みエラー` または `ファイル検証エラー` が含まれる。
- receiver=0 かつ正常更新の場合、送信もエラーログも発生しない。
- receiver=0 かつ非 UTF-8 ファイルの場合、本文読込を避けるためエラーログを出さない。
- 既存の receiver あり Error 送信テストは維持する。

テストには `tracing_test::traced_test` と一時ファイルを使う。必要な範囲では単一ファイルモードを優先し、ディレクトリモード固有の相対パス挙動は既存テストに委ねる。

## セキュリティ考慮

変更後も WebSocket 配送内容や Host/Origin 検証には触れない。追加されるのはローカル warn ログのみである。

ログには既存のエラー文言を使い、Markdown 本文や HTML は出さない。パス表示は `build_change_broadcast_message` の既存 sanitization と user-facing label の経路を再利用し、生の外部入力パスを新規に展開しない。

receiver=0 時に追加で検証と読込前検査を試みるため、TOCTOU による metadata/open 失敗は Error として分類される。成功した場合でも内容は読まず、ログに残さない。

## 受け入れ条件

- `notify_update` は receiver=0 の正常更新では broadcast を送らず、本文読込・描画を行わず、エラーログも出さない。
- `notify_update` は receiver=0 のファイル削除・metadata/open 失敗・サイズ超過・検証失敗を warn ログに残す。
- `notify_update` は receiver=0 の非 UTF-8 ファイルでは本文読込を避け、エラーログを出さない。
- receiver がいる場合の Error broadcast は既存通り動作する。
- `cargo test --all-targets --all-features` が通る。
- `./verify.sh` が通る。

## 影響範囲

- 主な変更対象: `src/server/broadcast.rs`
- 依存する既存経路: `src/server/files/content.rs` の `build_change_broadcast_message`、`src/server/files/resolve.rs` の `resolve_change_target`
- 軽量検査 helper 追加対象: `src/server/files/content.rs`
- 影響しない想定: WebSocket JSON 形式、HTTP route、renderer、template、watcher strategy

## ロールバック

`notify_update` の receiver=0 分岐で追加した補助関数呼び出し、`src/server/files/content.rs` の軽量検査 helper、追加テストを戻す。既存の送信経路やメッセージ型には変更を入れないため、ロールバック範囲は `src/server/broadcast.rs` と `src/server/files/content.rs` に閉じる。
