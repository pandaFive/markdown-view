# verify.sh E2E opt-in 統合設計

**作成日**: 2026-04-28
**対象 backlog**: `docs/todo/BACKLOG.md` の `E2E を verify.sh に統合するか検討`

## 目的

`verify.sh` を検証入口として拡張し、通常検証は現状の速度と依存条件を維持したまま、明示指定時だけ Playwright E2E まで実行できるようにする。

次タスクは `BACKLOG.md` の P1 項目 `E2E を verify.sh に統合するか検討` とする。直近の `E2E テストの DOM クリーンアップ戦略見直し` は実装済みコミットが存在するため、次のリスク低減対象として E2E 実行の入口統合に進む。

## 非ゴール

- E2E を `./verify.sh` の通常実行に常時含めない。
- Playwright 設定、fixture 分離、E2E 並列化は変更しない。
- CI 設定は追加しない。
- `npm run test:e2e` の script 内容は変更しない。
- Rust のプロダクションコードやブラウザ JS は変更しない。
- README など利用者向けドキュメント更新は、必要性を実装時に再評価する。

## 採用方針

`./verify.sh --e2e` の opt-in 方式を採用する。

通常の `./verify.sh` は、現在どおり次の順序で実行する。

1. `cargo fmt --all -- --check`
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo test --all-targets --all-features`
4. `npx --no-install tsc --noEmit`

`--e2e` が指定された場合だけ、最後に Playwright E2E を追加する。

```bash
./verify.sh --e2e
```

opt-in にする理由は、E2E が `cargo run` の webServer 起動、Playwright browser、固定 fixture、直列実行に依存し、通常の Rust 検証より重いためである。検証入口を増やさずに E2E まで回せる経路を作りつつ、日常の高速な検証は維持する。

## 実装設計

`verify.sh` に最小限の引数パースを追加する。

- `--e2e`: `run_e2e=true` にする。
- `-h` / `--help`: usage を表示して正常終了する。
- 未知引数: usage を表示して失敗する。

既存の `typecheck_e2e` は `node_modules` 存在確認を持っている。この確認を E2E 実行でも再利用できるよう、共通ヘルパーに分けるか、同じチェックを `run_e2e` に持たせる。

E2E 実行は、既存 script を尊重して `npm run test:e2e` を呼ぶ。`package.json` の script が Playwright 実行の single source of truth であり、将来オプションが増えても `verify.sh` 側を追随させやすい。

`set -Eeuo pipefail`、`run_step`、`ERR` trap の構造は維持する。E2E も `run_step "E2E実行 (Playwright)" run_e2e` の形で既存の失敗報告に乗せる。

## エラー処理

`node_modules` が存在しない場合は、既存 typecheck と同様に `npm ci` を先に実行するよう促して失敗する。

Playwright browser が未インストールの場合は、Playwright 側のエラーをそのまま表示する。実装時に独自検出を追加しない。Playwright の install 状態は環境依存であり、`verify.sh` が過剰に推測すると保守対象が増えるためである。

未知引数は明示的に失敗させる。`./verify.sh foo` が通常検証として成功すると、実行者が E2E を指定したつもりで見落とす可能性がある。

## 受け入れ基準

- `./verify.sh` の通常実行順序と実行内容が変わらない。
- `./verify.sh --e2e` が通常検証の後に `npm run test:e2e` を実行する。
- `node_modules` がない場合、typecheck または E2E 実行前に分かりやすいエラーで失敗する。
- 未知引数が usage を表示して非ゼロ終了する。
- `docs/todo/BACKLOG.md` の `E2E を verify.sh に統合するか検討` が完了済みになる。
- Playwright 設定、E2E fixture、プロダクションコードに不要な変更が入らない。

## 検証

実装フェーズでは次を確認する。

```bash
./verify.sh
./verify.sh --help
./verify.sh --unknown
./verify.sh --e2e
```

`./verify.sh --unknown` は失敗することを期待するため、終了コードと usage 表示を確認する。

`./verify.sh --e2e` は環境に Playwright browser が入っている場合に E2E まで pass することを確認する。browser 未インストールで失敗した場合は、失敗理由を検証結果として記録し、実装の成否と環境不足を分けて報告する。

## セキュリティ考慮

この変更は検証スクリプトの入口統合であり、アプリケーションのセキュリティ境界は変更しない。

E2E 実行時は Playwright の `webServer` が `cargo run -- tests/fixtures/e2e --port 4173 --no-open` を起動する。既存の localhost 前提、Host / Origin 検証、CSP、HTML sanitization、パス検証を弱めない。`verify.sh` から任意コマンド文字列を受け取る設計にはしない。

`BACKLOG.md` の記述は外部レビュー由来の内容を含むため、実装済み事実や現在の脆弱性として扱わない。実装時は現行の `verify.sh`、`package.json`、`playwright.config.ts` を根拠に再確認する。

## 影響範囲

- 変更予定: `verify.sh`
- 変更予定: `docs/todo/BACKLOG.md`
- 参照: `package.json`
- 参照: `playwright.config.ts`
- 実装コードへの影響: なし
- E2E テストコードへの影響: なし

## ロールバック

`verify.sh` の引数パースと `--e2e` 実行分岐を revert し、`docs/todo/BACKLOG.md` の対象項目を未完了に戻せば元の運用に戻せる。
