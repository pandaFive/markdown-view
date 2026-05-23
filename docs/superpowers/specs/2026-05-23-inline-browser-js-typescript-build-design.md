# インラインブラウザ JS TypeScript ビルド化設計

## 背景

`docs/todo/BACKLOG.md` の P3 には、Rust の `include_str!` でコンパイル時に埋め込まれるブラウザ側 JavaScript を TypeScript 化する長期改善項目が残っている。

現在の E2E テストは TypeScript 化済みで、`package.json`、`package-lock.json`、`tsconfig.json`、`npm run typecheck` が存在する。一方、`src/template/assets/js/*.js` は無型のまま、`src/template/assets/inline_script.rs` が固定順序で `include_str!` し、最後に `startMarkdownViewApp();` を呼ぶ IIFE として結合している。

今回の設計では、インラインブラウザ JS も TypeScript を正のソースにする。生成 `.js` はリポジトリへコミットせず、Cargo build 経路で `build.rs` が生成する。

## ゴール

- `src/template/assets/ts/*.ts` をインラインブラウザスクリプトの唯一の編集対象にする。
- 既存の `src/template/assets/js/*.js` は移行後に削除する。
- 生成 `.js` はリポジトリへコミットせず、`OUT_DIR` 配下に生成する。
- `build.rs` が `npm run build:inline-js` を呼び、`cargo build`、`cargo test`、`cargo run` で必要な JS が自動生成されるようにする。
- 既存のインラインスクリプト結合順序、`__MAX_FILE_SIZE_MB__` sentinel 置換、CSP hash、E2E hook 露出条件を維持する。
- TypeScript の strict 型検査で、DOM nullability、fetch payload、WebSocket payload、memo/search/content update payload の shape を明示する。

## 非ゴール

- ブラウザ UI、文言、表示仕様の変更。
- ES module 化や bundler 導入。
- 生成 `.js` のコミット。
- E2E ヘルパーの DRY 化。
- Host/Origin 検証、path validation、HTML sanitize、CSP、検索上限、ファイルサイズ上限の仕様変更。
- `innerHTML` sink の許可範囲拡大。
- Rust 単体ビルドを Node なしで維持すること。

## 採用方針

採用する構成は、TypeScript source、`build.rs` 自動生成、`OUT_DIR` 生成物参照の組み合わせにする。

`src/template/assets/ts/*.ts` を正とし、`build.rs` が Cargo の `OUT_DIR` 配下、たとえば `$OUT_DIR/inline-js/*.js` に生成する。`build.rs` はさらに `$OUT_DIR/inline_script_manifest.rs` のような小さな Rust 断片を生成し、`src/template/assets/inline_script.rs` は `include!(concat!(env!("OUT_DIR"), "/inline_script_manifest.rs"))` で生成済み JS を固定順序で参照する。

この構成により、生成物はソースツリーを汚さず、Cargo build ごとの環境固有出力として扱える。`inline_script.rs` は既存どおり、JS 断片の結合、`__MAX_FILE_SIZE_MB__` 置換、tree-sitter による sink 検査を担う。

検討した代替案は次のとおり。

- `src/template/assets/js/*.ts` から同階層へ `.js` を生成する案: 構造は単純だが、未追跡生成物がソースツリーに残りやすく、生成 `.js` をコミットしない方針と相性が悪い。
- npm 側で Rust 断片や結合済み文字列まで生成する案: 既存の Rust 側 scanner と sentinel 置換の責務まで Node 側へ寄り、変更範囲が広すぎる。

## ビルド設計

`build.rs` を新規追加する。`build.rs` は次を行う。

1. `OUT_DIR/inline-js` を生成先として用意する。
2. `MV_INLINE_JS_OUT_DIR=$OUT_DIR/inline-js` を環境変数として渡し、`npm run build:inline-js` を実行する。
3. 生成される JS ファイルが期待する 12 ファイル分そろっていることを確認する。
4. 固定順序で `include_str!` する manifest を `OUT_DIR/inline_script_manifest.rs` に書き出す。
5. Cargo の再実行条件を出力する。

再実行条件は少なくとも次を含める。

```text
cargo:rerun-if-changed=src/template/assets/ts
cargo:rerun-if-changed=tsconfig.inline-js.json
cargo:rerun-if-changed=package.json
cargo:rerun-if-changed=package-lock.json
cargo:rerun-if-changed=scripts/build-inline-js.mjs
```

`package.json` には次の script を追加する。

```json
{
  "scripts": {
    "build:inline-js": "node scripts/build-inline-js.mjs",
    "typecheck:inline-js": "tsc -p tsconfig.inline-js.json --noEmit",
    "typecheck:e2e": "tsc --noEmit",
    "typecheck": "npm run typecheck:e2e && npm run typecheck:inline-js"
  }
}
```

`scripts/build-inline-js.mjs` は `MV_INLINE_JS_OUT_DIR` がない場合に失敗し、`npx --no-install tsc -p tsconfig.inline-js.json --outDir "$MV_INLINE_JS_OUT_DIR"` 相当を実行する。TypeScript API や bundler は導入せず、既存 dev dependency の `typescript` を使う。

`npm`、`node_modules`、`tsc` がない場合は、`build.rs` が Cargo のエラーログに `npm ci` を先に実行する必要があることを明示する。今回の方針では Rust 単体ビルドを Node なしで維持しないため、この失敗は想定された開発環境エラーとして扱う。

## TypeScript 設計

`tsconfig.inline-js.json` を新規追加し、E2E 用の `tsconfig.json` と分ける。E2E は Node + Playwright 環境だが、インライン JS はブラウザ DOM 環境で動くため、型環境を分離する。

想定する主要設定は次のとおり。

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["DOM", "ES2022"],
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "exactOptionalPropertyTypes": true,
    "declaration": false,
    "sourceMap": false,
    "removeComments": false
  },
  "include": ["src/template/assets/ts/**/*.ts"]
}
```

既存コードは、複数ファイルが同一 IIFE スコープ内で順番に評価され、関数同士がグローバル script として参照できる前提になっている。初回移行では ES module 化しない。`module` は入れないか、TypeScript が global script として扱う設定に寄せる。ファイル間共有の型は `.d.ts` または `types.ts` 相当へ集約するが、runtime import/export は導入しない。

型定義は `src/template/assets/ts/types.d.ts` に集約する。主な対象は次。

- `MarkdownViewAppContext`
- `ContentUpdatePayload`
- `MemoResponse`
- `SearchResponse`
- `SearchResult`
- `WebSocketRefreshMessage`
- `MarkdownViewTestHooks`
- E2E 専用の `window.__MV_E2E__` と production 非公開 hook

DOM 要素取得は、既存挙動に合わせて nullability を明示する。必須要素は helper または局所的な throw で失敗を明確にし、任意要素は `null` 分岐を維持する。型検査のためだけに production 挙動を広く変えない。

## インラインスクリプト結合

生成 JS の結合順序は現行の `TEMPLATE` と同じにする。

1. `bootstrap`
2. `selection`
3. `content-renderer`
4. `content-enhancements`
5. `content-navigation`
6. `document-search`
7. `directory-search`
8. `content-controller`
9. `memo`
10. `fetch`
11. `websocket`
12. `sidebar`
13. `startMarkdownViewApp();`

この順序は明示的な契約として `build.rs` と `inline_script.rs` の両方から読める形にする。順序を変える場合は別設計で扱う。

`__MAX_FILE_SIZE_MB__` sentinel は `bootstrap.ts` 側に残し、`inline_script.rs` の `inline_js(max_file_size)` が生成 JS 全体に対して置換する既存契約を維持する。sentinel が期待箇所以外に混入していないこと、生成済み JS に sentinel が残らないことを既存テストで固定する。

## セキュリティ設計

TS 型はセキュリティ保証そのものではない。未信頼入力は従来どおり未信頼として扱い、サーバー側で sanitize 済みの HTML だけを既存の許可 sink に渡す境界を維持する。

`innerHTML` sink の許可範囲は増やさない。既存の tree-sitter scanner を生成 JS に対して走らせ、許可済み sink を維持する。生成後 JS の書式差により scanner の期待文字列更新が必要になっても、許可対象の意味は変えない。

`build.rs` は固定の npm script だけを `Command` で実行し、shell 文字列合成は使わない。外部から取得したテキスト、検索結果、Issue、LLM 出力を script 名、path、policy として実行しない。`MV_INLINE_JS_OUT_DIR` は Cargo の `OUT_DIR` 配下だけにし、任意 path への生成を許さない。

Host/Origin 検証、`127.0.0.1` binding、path validation、HTML sanitize、CSP hash、検索上限、ファイルサイズ上限、E2E hook の production 非公開契約は変更しない。

## テスト方針

この作業はコード変更を伴うため、既存挙動を固定してから移行する。移行前に不足している契約があれば小さなテストを追加する。

重点的に固定する契約は次。

- 生成 JS が JavaScript grammar として parse できる。
- `__MAX_FILE_SIZE_MB__` sentinel が期待箇所だけに存在し、置換後に残らない。
- `maxFileSizeMb` が数値 literal として出力される。
- `innerHTML` sink allowlist が増えない。
- `window.__MV_E2E__ === true` の場合だけ `window.markdownViewTestHooks` を公開する。
- production では E2E hook が公開されない。
- content update、memo、document search、directory search、sidebar、WebSocket の E2E が従来どおり動く。

実装後の検証コマンドは次を想定する。

```bash
npm run build:inline-js
npm run typecheck
cargo test --lib template::assets::inline_script
cargo test --test update_content_exposure
./verify.sh
./verify.sh --e2e
```

一括移行ではあるが、実装作業はファイル単位に小さく進める。`selection.ts`、`content-renderer.ts` のような小さいファイルで型付けパターンを確立し、その後に `memo.ts`、`document-search.ts`、`sidebar.ts` の大きいファイルへ適用する。

## 受け入れ基準

- `src/template/assets/ts/*.ts` がインラインブラウザ JS の正ソースになっている。
- `src/template/assets/js/*.js` が削除され、生成 `.js` がリポジトリに追加されていない。
- `cargo build`、`cargo test`、`cargo run` 経路で `build.rs` が TypeScript から JS を生成する。
- `inline_script.rs` が `OUT_DIR` 生成物を使い、既存の結合順序と `startMarkdownViewApp();` 呼び出しを維持する。
- `npm run typecheck` が E2E とインライン JS の両方を検査する。
- `./verify.sh` が TypeScript 生成と Rust 検証を通す。
- `./verify.sh --e2e` が主要ブラウザ挙動の回帰を検出できる。
- Host/Origin 検証、path validation、HTML sanitize、CSP、検索上限、ファイルサイズ上限、E2E hook production 非公開契約が弱まっていない。
- 生成 JS の `innerHTML` sink allowlist が増えていない。

## 影響範囲

- `build.rs`
  - TypeScript 生成と manifest 生成を追加する。
- `package.json`
  - `build:inline-js`、`typecheck:inline-js`、`typecheck:e2e`、統合 `typecheck` script を追加または更新する。
- `package-lock.json`
  - script 変更に伴う更新は原則不要だが、npm が metadata を更新する場合は差分を確認する。
- `tsconfig.inline-js.json`
  - ブラウザ用 TypeScript 設定を追加する。
- `scripts/build-inline-js.mjs`
  - `MV_INLINE_JS_OUT_DIR` を受けて `tsc` を実行する。
- `src/template/assets/ts/*.ts`
  - 既存 JS 12 ファイルの移行先。
- `src/template/assets/js/*.js`
  - 移行完了後に削除する。
- `src/template/assets/inline_script.rs`
  - `OUT_DIR` manifest 参照へ変更する。
- `tests/e2e/globals.d.ts`
  - E2E hook 型と実装側型の整合確認対象。
- `verify.sh`
  - script 名変更に追従する可能性がある。

依存的に、template asset tests、content update exposure E2E、document search E2E、memo E2E、sidebar / TOC E2E、WebSocket E2E が回帰検知の対象になる。

## ロールバック

移行コミットを revert すれば、`src/template/assets/js/*.js` と `include_str!` 方式へ戻せる。生成物は `OUT_DIR` 配下にあるため、リポジトリの cleanup は不要。

`build.rs` 追加により Cargo build が Node/npm に依存するため、問題が出た場合は `build.rs`、`tsconfig.inline-js.json`、`scripts/build-inline-js.mjs`、`src/template/assets/ts/`、`inline_script.rs` の変更をまとめて revert する。

## 残余リスク

- Cargo build が Node/npm/node_modules に依存するため、Rust だけで clone して build する体験は失われる。
- 3,555 行の JS 一括移行になるため、レビュー負荷と局所的な typo リスクが高い。
- TypeScript が生成する JS の書式差により、tree-sitter scanner の期待文字列調整が必要になる可能性がある。
- global script として TS を扱うため、ES module 化や bundling に比べて名前衝突の検出力は限定的。
- 型定義を既存実装から写す過程で、実 runtime shape と型がずれる可能性がある。

## 見積もり

- 人間作業: 4-7 時間。
- Codex/AI 支援: 90-180 分。

変動要因は、大きい `memo.js`、`document-search.js`、`sidebar.js` の nullability 修正量、生成 JS と既存 scanner の差分、E2E 実行時間。

## 検証

この設計書自体は docs-only なので、TDD ではなく文書検証で確認する。実装計画フェーズでは、上記のテスト方針に沿ってコード変更前の契約固定と移行後検証を行う。

設計書の自己検証では次を確認する。

```bash
placeholder_matches="$(rg -n "T[B]D|TO[D]O|未[定]" docs/superpowers/specs/2026-05-23-inline-browser-js-typescript-build-design.md || :)"
test -z "$placeholder_matches" || { printf '%s\n' "$placeholder_matches"; exit 1; }
rg -n "ゴール|非ゴール|採用方針|ビルド設計|TypeScript 設計|セキュリティ設計|受け入れ基準|影響範囲|ロールバック|見積もり" docs/superpowers/specs/2026-05-23-inline-browser-js-typescript-build-design.md
git diff -- docs/superpowers/specs/2026-05-23-inline-browser-js-typescript-build-design.md
```
