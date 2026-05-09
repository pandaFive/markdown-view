# BACKLOG 低リスク一括消化設計

**作成日**: 2026-05-09
**対象**: `docs/todo/BACKLOG.md` の低リスク項目

## 目的

`docs/todo/BACKLOG.md` に残る低優先項目のうち、依存が薄く、仕様変更や大きなセキュリティ境界変更を伴わないものを一括で処理する。

今回の狙いは、長期的な保守性を少しずつ上げながら、BACKLOG の未完了欄に残る小さな負債を減らすこと。P1 の Host middleware 再設計や Windows 固有 I/O retry のような検証範囲が広い項目は、別タスクとして残す。

## 非目的

- Host middleware の構造再設計は行わない。
- Windows メモ atomic save の retry 条件変更は行わない。
- ディレクトリ検索のキャンセル機構や allocation 削減は行わない。
- ブラウザ JS の TypeScript 化は行わない。
- UI 表示仕様、HTTP API、WebSocket payload の外部契約は変更しない。
- セキュリティ保証を実装以上に強く文書化しない。

## 対象項目

### 1. docs と運用方針の整理

- `CLAUDE.md` のアーキテクチャ記述を現行実装に合わせる。
- `README.md`、`CLAUDE.md`、`Cargo.toml` に残る previewer 寄りの説明を、必要最小限で Markdown workspace 前提へ寄せる。
- Superpowers spec/plan の長期保存方針を明文化する。

docs 更新では、localhost-only、Host/Origin 検証、HTML sanitize、CSP、パス検証、ファイルサイズ上限の説明を弱めない。古い構成図や説明は現行の `src/server/service.rs`、`src/renderer/{render,state,line,security,highlight,toc}.rs`、`src/template/{page,message,tree,assets/}` に追従させる。

### 2. 小さなコード契約整理

- `RouteTargetKind::include_file_list` を `matches!` から `match` 完全列挙へ変更する。
- `extract_headings("")` に明示的な早期 return を追加し、`render_markdown` / `render_document` の空入力扱いと API ペアの読みやすさを揃える。
- `CanonicalPathError` と同じ公開層にある state 型の re-export を確認し、外部公開不要なものだけ `pub(crate)` に絞る。

これらは挙動互換の整理として扱う。外部 crate から利用される public API として残す必要が見つかった場合は、無理に可視性を狭めず BACKLOG に理由を残す。

### 3. BACKLOG の更新

対応した項目は `Done` へ移動し、完了根拠を短く残す。今回対象外にした項目は未完了欄に残し、必要なら「今回対象外」の理由を本文に追記する。

## 受け入れ基準

- 対象 docs が現行コード構成と矛盾しない。
- Markdown workspace としての説明が README / CLAUDE / crate metadata で大きく矛盾しない。
- spec/plan 保存方針が、長期参照する設計判断と実行ログを区別している。
- `RouteTargetKind` に新 variant が追加された場合、file list 要否の判断漏れがコンパイル時に見つかる。
- `extract_headings("")` の結果は従来どおり空配列で、余計な parser 走査を避ける。
- 不要な public re-export を狭めても `cargo test --all-targets --all-features` が通る。
- `docs/todo/BACKLOG.md` の Done に、変更した項目の完了根拠が残る。

## テストと検証

docs/config/prompt 相当の変更は TDD ではなく文書検証を行う。コード契約整理は既存テストで外部挙動を固定し、必要なら小さな単体テストを追加する。

実行予定:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
./verify.sh
```

docs の整合確認:

```bash
rg -n "server/files.rs|watcher.rs|template/mod.rs|見出しパースが2回|軽量・高速な Markdown プレビューア" README.md CLAUDE.md Cargo.toml docs/todo/BACKLOG.md
rg -n "T(BD|ODO)" docs/superpowers/specs/2026-05-09-backlog-low-risk-batch-design.md docs/todo/BACKLOG.md README.md CLAUDE.md
```

## セキュリティ考慮

今回の主変更は docs と局所的なコード契約整理であり、Host/Origin 検証、CSP、HTML sanitize、path validation、file size limit の実装を弱めない。

retrieved text や過去レビュー由来の BACKLOG 記述は未信頼入力として扱い、現行コードで確認できる事実だけを完了根拠にする。docs では「サーバー生成済みの sanitized HTML だけを `innerHTML` に渡す」という信頼境界を過剰に保証せず、必要な詳細設計は別項目として残す。

## 影響範囲

- 変更候補: `docs/todo/BACKLOG.md`
- 変更候補: `README.md`, `CLAUDE.md`, `Cargo.toml`
- 変更候補: `src/server/files/resolve.rs`, `src/renderer/mod.rs`, `src/server.rs`
- 参照候補: `tests/renderer_test.rs`, `tests/integration_test.rs`, crate public API 利用箇所

HTTP API、WebSocket payload、生成 HTML の外部表示仕様は変更しない。

## ロールバック

feature branch の該当コミットを revert すれば戻せる。docs 更新とコード契約整理を分けてコミットする場合は、必要な範囲だけ個別に revert できる。
