# BACKLOG P1 契約整理バッチ設計

**作成日**: 2026-05-16
**対象ファイル**: `src/server/messages.rs`, `src/server/session.rs`, `src/server/files/memo.rs`, `src/server/files/memo_fs.rs`, `tests/`

## 目的

`docs/todo/BACKLOG.md` の P1 に残っている低リスクな契約整理を、1 つの小粒バッチとして処理する。

対象は次の 3 件に絞る。

- `BroadcastMessage::Update` 系の JSON 直列化失敗時 fallback を明示する。
- メモ読込のサイズ上限契約を `MemoFs::read_with_limit` へ寄せ、`read_memo_file_if_present` の重複チェックを削る。
- `tokio::select!` の cancel-safe 前提を `src/server/session.rs` に短く明記する。

いずれも現行のユーザー可視挙動を大きく変えず、将来改修時の判断材料をコード上に残すことが目的である。

## 非目的

- WebSocket payload shape は変更しない。
- メモ API の成功・失敗レスポンス形状は変更しない。
- HTML / JS 初期化契約は変更しない。
- `data-memo-file` の None 時属性出力は今回扱わない。
- `log_path::canonicalize_status` や catalog の性能最適化は今回扱わない。
- Host / Origin 検証、CSP、HTML sanitize、path validation の設計は変更しない。

## 方針

### `BroadcastMessage::Update` fallback

`BroadcastMessage::to_json()` の `Update` arm に、Update 専用の fallback 経路を追加する。

通常時は既存どおり `UpdateMessage` を `serde_json::to_string` し、`content`、`toc`、optional `file` の契約を維持する。直列化失敗時は `tracing::error!` で観測し、クライアントが既存 update として扱える最小 JSON を返す。

fallback payload は `content: ""` と `toc: ""` を含める。`file` は含めない。fallback 生成自体は `serde_json::json!` に依存せず、静的文字列または失敗しない小さな helper にして、fallback の fallback を作らない。

現状の `UpdateMessage` 構造では直列化失敗は実質不可能だが、`session.rs` や broadcast 経路が持つエラー処理との契約を `BroadcastMessage` 側へ寄せる。

### メモ読込サイズ契約

`MemoFs::read_with_limit` の doc コメントを、TOCTOU 対策として「実読み取り量が上限を超えた場合は `MemoReadError::TooLarge` を返す」契約として明確化する。

`read_memo_file_if_present` の `metadata.len() > MAX_FILE_SIZE` 早期拒否は残す。これは明らかな上限超過を本文読み込み前に拒否するための一段目チェックである。

読み込み後の `bytes.len() as u64 > MAX_FILE_SIZE` チェックは削除する。読み込み開始後にファイルが増える race は `read_with_limit` が `MAX_FILE_SIZE + 1` 相当までの読み込みで検出し、`MemoReadError::TooLarge` を返す責務にする。

### `tokio::select!` cancel-safe コメント

`src/server/session.rs` の WebSocket 送受信ループに、現在の `socket.recv()` と `rx.recv()` branch が cancel-safe である前提を短くコメントする。

コメントは実装根拠と将来 branch 追加時の注意に限定する。挙動変更や抽象化追加は行わない。

## セキュリティ

ファイルサイズ上限は弱めない。

メモ読込は、metadata 時点の上限超過と、metadata 後にファイルが増えたケースの両方を拒否する。後者は `MemoFs::read_with_limit` の責務として明文化し、テストで固定する。

Update fallback は空の `content` / `toc` だけを返し、未信頼入力を新たに混ぜない。既存の HTML sanitize 済み payload 契約や CSP には影響しない。

WebSocket の Host / Origin 検証、HTTP security headers、path traversal 防御、memo sidecar path 検証には触れない。

## テスト

追加または更新するテストは次を確認する。

- `BroadcastMessage::Update` の通常 JSON が既存契約を維持する。
- Update fallback helper が `content` と `toc` を持つ valid JSON を返す。
- メモ読込で metadata 時点の上限超過が既存どおり `413 Payload Too Large` 相当へ落ちる。
- `MemoFs::read_with_limit` が読み込み中の上限超過を `MemoReadError::TooLarge` として返す契約を持つ。
- `read_memo_file_if_present` から読み込み後の重複サイズチェックを削っても、上限超過が `MemoReadError::TooLarge` 経由で処理される。

`session.rs` の cancel-safe コメントは挙動変更を伴わないため、専用テストは追加しない。

## 受け入れ条件

- `BroadcastMessage::Update` の通常 JSON は `content` / `toc` / optional `file` を維持する。
- Update fallback は `content` と `toc` を持つ valid JSON である。
- Update 直列化失敗時に `tracing::error!` で観測できる設計になっている。
- メモ読込のサイズ上限は、metadata 早期拒否と `read_with_limit` の実読み取り制限の 2 段で維持される。
- `read_memo_file_if_present` の読み込み後重複チェックが削除されている。
- `tokio::select!` の cancel-safe 前提が、将来改修者に伝わる短いコメントとして残っている。
- ユーザー可視の WebSocket / HTTP / HTML 契約を変更しない。

## 検証

実装後は次を実行する。

```bash
cargo test --all-targets --all-features
./verify.sh
```

必要に応じて反復中は次の targeted test を使う。

```bash
cargo test server::messages
cargo test server::files
```

docs-only の設計書作成時点では、文書検証として次を確認する。

```bash
rg -n "目的|非目的|セキュリティ|受け入れ条件|ロールバック" docs/superpowers/specs/2026-05-16-backlog-p1-contract-cleanup-design.md
rg -n "T[B]D|T[O]DO|未[定]|あと[で]" docs/superpowers/specs/2026-05-16-backlog-p1-contract-cleanup-design.md
```

## 影響範囲

- `src/server/messages.rs`: Update JSON fallback 契約を追加する。
- `src/server/session.rs`: `tokio::select!` cancel-safe 前提コメントを追加する。
- `src/server/files/memo.rs`: 読み込み後サイズ重複チェックを削除する。
- `src/server/files/memo_fs.rs`: `read_with_limit` 契約コメントと必要なテストを補強する。
- `tests/`: 上記契約の回帰テストを追加または更新する。

影響を受ける dependent file は、WebSocket 送信経路、memo API 経路、memo filesystem test support である。外部設定、永続データ、ビルド設定への影響はない。

## ロールバック

対象ファイルの差分を revert すれば元に戻せる。

DB、設定、永続データ、生成物の migration はない。fallback payload は空 update のみで、ロールバック時に既存ファイルや memo sidecar の内容へ影響しない。

## 見積もり

- 人間作業: 30-60 分
- Codex/AI 支援: 15-30 分
