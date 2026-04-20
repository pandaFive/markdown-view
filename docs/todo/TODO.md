# TODO Issues

## TODO Issues (レビュー日: 2026-04-20, PR #76 レビュー)

#### セキュリティ・堅牢性

- [ ] CSP フォールバック時の方針整理（fail-fast vs 現状運用）
  - ファイル: `src/server/guards.rs` L30-36
  - 現状: `HeaderValue::from_str(&csp)` 失敗時のフォールバック CSP は `default-src 'self'; object-src 'none'; frame-ancestors 'none'`。inline は暗黙拒否されるが、sha256 ハッシュベースの厳格制約は失われる
  - 対応候補: (a) CSP 生成失敗をサーバー起動中止扱いにする、(b) フォールバック CSP に `script-src 'none'; style-src 'none'` を明示する、(c) 現状維持で運用ログ監視に任せる
  - 理由: フォールバック発生時の動作セマンティクスが不明瞭。個人使用前提だが、意図ある設計として明文化したい

- [ ] エラー経路ログのパス情報を base 相対化
  - ファイル: `src/server/files/resolve.rs` ほか `tracing::warn!` でパスを出す箇所
  - 現状: パス正規化失敗時にユーザー指定パス・サーバー実ディレクトリ構造をそのまま warn ログに出力
  - 対応: base_dir 基準での相対化ヘルパー `sanitize_path_for_logging(path, base)` を抽出し、絶対パスや base 外パスを丸めて出力
  - 理由: 個人使用前提でもディレクトリ構造の漏出は望ましくない

#### 可読性改善

- [ ] `is_hidden_relative` のネスト深度を 3 → 2 階層に削減
  - ファイル: `src/watcher/strategy.rs` L162-195
  - 現状: `match strip_prefix → match canonicalize(path) → match canonicalize(base)` の 3 段ネストで、canonicalize 失敗時のフォールバックログが 2 回重複
  - 対応: `try_relative_components(path, base) -> Option<impl Iterator<Component>>` 風のヘルパーを抽出し、呼び出し側は 1 回 match
  - 理由: 直前の watcher リファクタで隠し判定のロジックだけが旧形状のまま残っている

### Low Priority

- [ ] `augmentHashWithTrailingLineHint` ELEMENT_NODE sibling のユニットテスト
  - ファイル: `tests/e2e/memo_jump.spec.js`
  - 行番号: L359 付近（既存 augmentHashWithTrailingLineHint テスト群と併設）
  - 理由: `src/template/assets/js/content.js` L181-L182 のドックコメント『renderer がソース行トラッキング用に text を `<span>` でラップするケースに対応』という設計意図を固定する直接テストが欠如している。現状は L205 の「旧形式メモ」E2E で実レンダ経由の TEXT_NODE パスのみカバー。`document.createElement('span')` で `L15` を内包したノードを sibling に置いて `textContent` 経路が生きることを明示的に検証する
  - 優先度: Low（criticality 4-5。間接カバーあり）

- [ ] `augmentHashWithTrailingLineHint` 範囲形式 hash + sibling L の precedence テスト
  - ファイル: `tests/e2e/memo_jump.spec.js`
  - 行番号: L359 追加テストに隣接
  - 内容: `hash === '#section-b:L15-L17'` + sibling `L20` でも早期 return する（結果 `'#section-b:L15-L17'` 不変）ことを明示検証
  - 理由: 単一行版 (PR #76 で追加) と `parseLineHash.lineRange` 経由で同分岐に入るため動作上は冗長だが、将来 `parseLineHash` の範囲パースを改変したとき回帰を検出できる
  - 優先度: Low（criticality 3。単一行版で分岐は既にカバー済み）

- [ ] `augmentHashWithTrailingLineHint` `!sibling` 早期 return のユニットテスト
  - ファイル: `tests/e2e/memo_jump.spec.js`
  - 内容: リンクが末尾で `link.nextSibling === null` のときに hash が不変であることを検証
  - 理由: `src/template/assets/js/content.js` L186 の早期 return 分岐カバレッジ。挙動は自明だが、`memo-preview` 末尾 citation のガード確認として有効
  - 優先度: Low（criticality 2。挙動自明）

- [ ] E2E テストの DOM クリーンアップ戦略見直し
  - ファイル: `tests/e2e/memo_jump.spec.js`（全 augmentHashWithTrailingLineHint 系テスト）
  - 行番号: L245-L407 の `try/finally` ブロック
  - 内容: 現状は `container.lastChild && lastChild.nodeType === TEXT_NODE` で末尾を削除しているが、並列で別ノードが挿入された場合に想定外ノードを削除する脆さがある。`afterEach` で `memo-preview` innerHTML のスナップショット復元に寄せると安全
  - 理由: PR #76 レビュー（pr-test-analyzer）で指摘された全テスト共通の懸念。本 PR 単独の課題ではなくテスト基盤改善
  - 優先度: Low（現状は実害なし、将来のテスト拡張で顕在化する可能性あり）

## TODO Issues (レビュー日: 2026-04-20, PR #77 レビュー)

### Low Priority

- [ ] 猶予期間中の連続 TOC クリックでの挙動検証
  - ファイル: `tests/e2e/text_selection_defer.spec.js`
  - 内容: `markPendingTocNavigation` は無条件に id と時刻を上書きする（sidebar.js L180-186）。grace 400ms 以内に `clickTocLink('alpha')` → `clickTocLink('beta')` と連続クリックしたとき、最終 active と scrollY が 2 番目のリンク先に正しく収束することを検証するテストが欠落
  - 理由: pending 上書き仕様が壊れた場合（条件付き更新などに書き換え）の回帰検知
  - 優先度: Low（criticality 5。現実のユーザ操作としてまれ）

- [ ] `TOC_NAVIGATION_SLACK_PX` 境界の回帰テスト
  - ファイル: `tests/e2e/text_selection_defer.spec.js` L347-L359 近辺
  - 内容: 現 L347 の小揺らし検証は `+6px` ハードコード。`SLACK - 2 = 22px` で pending 維持、`SLACK + 2 = 26px` で通常判定復帰を 2 ポイントで検証すれば SLACK 定数縮小の回帰を検出できる
  - 理由: 定数変更時のテスト反映漏れ検知
  - 優先度: Low（criticality 4。定数変更頻度は低い）

- [ ] L347 を active 遷移フラッシュ厳密検証に強化
  - ファイル: `tests/e2e/text_selection_defer.spec.js` L347-L359
  - 内容: 現在の `expect.poll(...).toBe('Beta')` は「最終的に Beta なら通る」。一瞬 `Alpha` に遷移して戻るケースを見逃す。`MutationObserver` で `#toc a.active` の `class` 遷移を監視し、Beta 以外への切り替わりが 0 回であることを主張するように強化
  - 理由: フラッシュ系の視覚バグは poll で見逃されるため、より厳密な回帰検知を整備する
  - 優先度: Low（criticality 6。現実の視認性には影響するが現状 pass で安定）


## TODO Issues (レビュー日: 2026-04-20, PR #80 レビュー)

### Medium Priority

- [ ] `updateContent` の inverse case (file-switch / data.content 変更時) の再描画検証
  - ファイル: `tests/e2e/memo_jump.spec.js` (回帰テスト L429 周辺に Step 4 追加 or 別テスト)
  - 内容: 現状の回帰テストは「同一 data.content での 2 回目 no-op」のみ検証。**逆方向**である「data.content が変わったら必ず再描画される」を直接検証するテストが欠落
  - 想定実装: 既存 prime → highlight → 同一 no-op の後に Step 4 として、別の `data.content` 文字列 (例: ダミー HTML) を渡して `window.updateContent` を呼び、(a) `.jump-highlight` が消えている (= 再描画された) (b) その後同一の changed content で再度呼ぶと no-op (= cache が新値で更新された) の 2 点を検証
  - 理由: cache invariant が逆転した regression (条件が常に false 化する書き換え等) を現状の suite では検出できない
  - 優先度: Medium（criticality 7。修正は 5 行だが invariant の半分が未検証）

- [ ] `updateContent` で `data.content === undefined` を契約違反として明示ログ
  - ファイル: `src/template/assets/js/content.js` L1239 周辺
  - 内容: `UpdateMessage` (`src/template/message.rs`) は `content` / `toc` に `skip_serializing_if` を付けていないため `data.content` は **必ず** 存在するはずだが、現状は `undefined` を no-op で黙殺している。サーバ契約変更や中継プロキシ改変で content が欠落した場合「ファイル編集してもプレビュー更新されない」サイレント失敗になる
  - 想定実装: `data.content === undefined` の場合 `console.warn('[markdown-view] updateContent: data.content が欠落 (契約違反)', data);` を出し、TOC 更新等の副作用は継続
  - 理由: WS フレームを直接覗かないとデバッグ不能なサイレント失敗の予防
  - 優先度: Medium（criticality 6。現状の契約では発生しないが将来の regression 検出に有効）

### Low Priority

- [ ] `window.updateContent` を E2E モード限定 expose に変更
  - ファイル: `src/template/assets/js/content.js` L1303 (現状 `window.updateContent = updateContent;`)
  - 内容: Playwright 実行時のみ expose する形 (`if (window.__MV_E2E__) window.updateContent = updateContent;`) に変更。E2E 側は `page.addInitScript(() => { window.__MV_E2E__ = true; })` で有効化
  - 理由: 個人 markdown viewer (127.0.0.1 限定) なので実害はないが、テスト hook が production HTML に常時露出している。将来 OSS 化 / 公開ホスティングに転じた際にサニタイズ層をバイパスして任意 HTML payload を流す呼び出しが可能になる
  - 優先度: Low（criticality 4。コメントで「本番から呼ぶな」とは明示済み、用途上は許容）

- [ ] `contentEl` への HTML 代入時の例外可視化
  - ファイル: `src/template/assets/js/content.js` L1239-L1242
  - 内容: 現状は `try/catch` なし。CSP 違反 / 拡張機能が DOM mutation observer 経由で throw を投げ込んだ場合、例外が呼出元まで bubble up し `live-status` も曖昧に。想定実装: `try` で代入と cache 更新を囲み、`catch` で `console.error` + `showWsParseErrorBanner` + `setLiveStatus('error')` + early return
  - 理由: 失敗時に「ライブ更新が止まっている」と「変更がなかった」をユーザーが区別できない silent failure 化
  - 優先度: Low（criticality 3。本 PR 修正前から同じ挙動、本質的に既存問題）

- [ ] 初回 broadcast 中に付与済みクラスが消失する edge case の検証
  - ファイル: `tests/e2e/memo_jump.spec.js` 新規テスト
  - 内容: SSR 完了から WS 接続完了までの数十〜数百 ms にユーザーが目次クリック等で `.jump-highlight` を獲得した場合、A2 設計上の「初回 broadcast 1 回再描画」でクラスが消える可能性。`MutationObserver` で `#content` の childList 置換回数を監視し、初回 broadcast 後に 0 回追加置換されることを assert
  - 理由: A2 設計の「UI 影響なし」前提の境界条件検証
  - 優先度: Low（criticality 3。実用上ユーザーが SSR 直後 100ms 以内に目次クリックする可能性は低い）

## TODO Issues (レビュー日: 2026-04-20, E2E TypeScript移行 PR レビュー)

### Low Priority

- [ ] E2E の `declare global` ブロックを `tests/e2e/globals.d.ts` に集約
  - ファイル: `tests/e2e/{text_selection_defer,document_search}.spec.ts` に散在する `declare global { interface Window { ... } }` + ブラウザバンドル関数/変数の declare
  - 内容: 共通 ambient 宣言を `tests/e2e/globals.d.ts` に一本化。各 spec の `declare global` を削除。`tsconfig.json` の `include` で拾う
  - 理由: `Window.__lastWs` / `__realWsOnmessage` が text_selection_defer と document_search で byte 一致しているが、片方を変更すると TS2717 で破綻するリスクを根治。`selectFile` / `updateContent` 宣言の spec 間不整合も解消
  - 優先度: Low（現状は TS declaration merging で動作、実害は将来の drift リスクのみ）

- [ ] `updateContent` 型宣言の統一
  - ファイル: `tests/e2e/memo_jump.spec.ts` (Window.updateContent プロパティ型), `tests/e2e/document_search.spec.ts` (top-level function 型)
  - 内容: 同じランタイム binding に対し 2 通りの型宣言が存在。`opts` が memo_jump では required、document_search では optional と不整合。どちらかに統一
  - 理由: 同一 binding を 2 型で捕捉しているため、片方の型が誤っても検出不能。ペイロード union (`{ refresh: true }` / `file?: string` 等) も未表現
  - 優先度: Low（現行 test は両宣言で動作、実害なし）

- [ ] `document_search.spec.ts:18` の冗長な `export {};` 削除
  - 内容: `import { test, expect, type Page } from '@playwright/test';` で既に module 扱いのため `export {};` は不要。他 5 spec も import ベースで同等扱い
  - 理由: 他 spec と対称性を保ち、コメント（declare global の要件）の誤解を防ぐ
  - 優先度: Low（cosmetic）

- [ ] `as unknown as` double-cast の説明コメント追加
  - ファイル: `tests/e2e/{text_selection_defer,document_search}.spec.ts` の `stabilizeWebSocketHarness` 内
  - 内容: `window.__realWsOnmessage = window.__lastWs.onmessage! as unknown as (ev: { data: string }) => void;` の直前に、`__dispatchWsMessage` が MessageEvent を生成せず `{ data: string }` を直接渡すため契約を狭めている旨の日本語コメント
  - 理由: 2 箇所の strict エスケープハッチが無説明。`MessageEvent` contravariance の問題を説明しないと将来の保守者が削除しかねない
  - 優先度: Low（動作は正しい、理解補助のみ）

- [ ] `memo_jump.spec.ts:303` の Codex review ID `#4136142343` 削除
  - 内容: 外部 review system の ID 参照を除去し、回帰保護の対象である false-positive パターンの説明に置き換える
  - 理由: ID は Codex 側でアーカイブされると参照不能、典型的な rot-prone comment
  - 優先度: Low（既存 PR #76 で持ち込まれた既存課題、E2E TS 移行とは独立）

- [ ] `tsconfig.json` に `noUncheckedIndexedAccess` / `exactOptionalPropertyTypes` を追加
  - 内容: strict の上位に両 flag を有効化
  - 理由:
    - `noUncheckedIndexedAccess`: `__clickObservations[href]` 等の Record アクセスに `undefined` 可能性を強制 → missing key のバグを発見
    - `exactOptionalPropertyTypes`: `toc?: string` と `toc: undefined` の区別を厳格化 → `content.js` 側の `data.toc !== undefined` チェックと整合
  - 優先度: Low（現状の test は既存 flag で strict、追加強化は将来の保守リスク低減目的）

- [ ] E2E 共通ヘルパー (`resetFixtures`, `selectParagraphText` 等) を `tests/e2e/helpers.ts` に抽出
  - ファイル: `tests/e2e/{memo_quote,memo_sync,memo_jump,markdown_links,text_selection_defer}.spec.ts` に重複するヘルパー
  - 内容: 複数 spec で同一実装されているヘルパー関数を共通モジュールに抽出
  - 理由: DRY 違反、片方を修正して片方を忘れるリスク。TS 化の副産物として可視化されたが、E2E TS 移行スコープ外として延期
  - 優先度: Low（現状動作、保守性向上のみ）

- [ ] `TestWebSocket` を `tests/e2e/browser/test-websocket.ts` に抽出し `addInitScript` 経由でロード
  - ファイル: `tests/e2e/{text_selection_defer,document_search}.spec.ts` の `page.addInitScript` 内 TestWebSocket 定義
  - 内容: 共有ブラウザハーネスモジュールとして切り出し、`page.addInitScript(path)` で読み込む
  - 理由: 2 spec で TestWebSocket 定義が重複、片方に `setTimeout` override が付く等の drift が発生している
  - 優先度: Low（現状動作、将来の drift 防止）

- [ ] E2E を `verify.sh` に統合するか検討
  - ファイル: `verify.sh`
  - 内容: 現状 `tsc --noEmit` のみで `npm run test:e2e` は手動実行。verify.sh で Rust server 立ち上げ→ playwright 実行まで含めるか
  - 理由: E2E を CI で回していない現状、type check のみが SSoT。実行コストと速度のトレードオフ要検討
  - 優先度: Low（個人使用前提で現状維持可）

- [ ] インラインブラウザJS (`src/template/assets/js/*.js`) の TS 化
  - ファイル: 7 ファイル (bootstrap, content, fetch, memo, selection, sidebar, websocket)
  - 内容: Rust の `include_str!` でコンパイル時に埋め込まれる JS を TS で記述し、事前 tsc でビルドして `.js` 出力を `include_str!` 対象にする
  - 理由: ブラウザ側 JS は現在無型。ただし Rust ビルドパイプラインへの Node 依存追加が必要で、「Rust 単体ビルド」の明快さが崩れる
  - 優先度: Low（個人利用前提・staged migration 方針、ビルド複雑化コストに見合うか要検討）

## TODO Issues (レビュー日: 2026-04-18, コードベース探索 - PR #59)

### High Priority

#### セキュリティ境界

- [ ] `is_trusted_host` / `normalize_authority` の IPv6 網羅テストを追加
  - ファイル: `src/server/guards.rs`
  - 現状: L171-173 の `test_trusted_host_loopback_ipv6` が `[::1]` のみを検証
  - 追加観点: `[::1]:3000`（port 付き bracketed）、`::1`（非 bracketed）、`[fe80::1]`（非 loopback）、`[::1]:abc`（非数値 port）の 4 パターン
  - 理由: DNS Rebinding 対策の核。正規化エッジケースで想定外に通過するとセキュリティ境界が崩れる

- [ ] メモ sidecar fallback 経路の超長ファイル名＋特殊文字テストを追加
  - ファイル: `src/server/files/memo.rs`, `src/server/files/tests.rs`
  - 現状: `ensure_safe_memo_path` / `truncate_to_bytes` の基本テストと非utf8/拡張子大小テストはあるが、255 バイト超のファイル名と `../` や `\..\` の組み合わせが未検証
  - 追加観点: (a) 超長名＋特殊文字で sidecar 名が隔離され破損しないこと、(b) 異なる長い名前が同一 sidecar 名に衝突しないこと
  - 理由: パストラバーサル境界の回帰テスト

- [ ] メモ API のボディ制限値を意図明文化し、境界テストを追加
  - ファイル: `src/server/routes.rs` L27
  - 現状: `MEMO_JSON_BODY_LIMIT = (MAX_FILE_SIZE * 2) + 4096` が無説明で定義
  - 対応: JSON エスケープで最悪 2 倍になる前提を doc コメントで明示。`MAX_FILE_SIZE + 小さなマージン` に引き締める可否を再検討。境界テスト（10MB + 1 バイト、20MB 付近）を追加
  - 理由: 将来の保守時に「なぜ 2 倍か」が読めないと制限緩和や強化判断を誤る

- [ ] WebSocket close_code マッピングの統合テストを追加
  - ファイル: `tests/integration_test.rs`
  - 現状: `ReadMarkdownError::close_code()` のユニットテストは存在、`load_initial_socket_update` のエラー arm も Low 側で TODO 化済み。だが実際の WebSocket フレームまで透過確認する E2E はない
  - 追加観点: IO → 1011、TooLarge → 1009、NotUtf8 → 1003 の 3 シナリオを実サーバー + WebSocket クライアントで検証
  - 理由: WebSocket プロトコル境界。クライアント側の再接続ロジックが close_code に依存するため、中間層のどこかで書き換わると下流が壊れる

### Medium Priority

#### セキュリティ・堅牢性

- [ ] CSP フォールバック時の方針整理（fail-fast vs 現状運用）
  - ファイル: `src/server/guards.rs` L30-36
  - 現状: `HeaderValue::from_str(&csp)` 失敗時のフォールバック CSP は `default-src 'self'; object-src 'none'; frame-ancestors 'none'`。inline は暗黙拒否されるが、sha256 ハッシュベースの厳格制約は失われる
  - 対応候補: (a) CSP 生成失敗をサーバー起動中止扱いにする、(b) フォールバック CSP に `script-src 'none'; style-src 'none'` を明示する、(c) 現状維持で運用ログ監視に任せる
  - 理由: フォールバック発生時の動作セマンティクスが不明瞭。個人使用前提だが、意図ある設計として明文化したい

- [ ] エラー経路ログのパス情報を base 相対化
  - ファイル: `src/server/files/resolve.rs` ほか `tracing::warn!` でパスを出す箇所
  - 現状: パス正規化失敗時にユーザー指定パス・サーバー実ディレクトリ構造をそのまま warn ログに出力
  - 対応: base_dir 基準での相対化ヘルパー `sanitize_path_for_logging(path, base)` を抽出し、絶対パスや base 外パスを丸めて出力
  - 理由: 個人使用前提でもディレクトリ構造の漏出は望ましくない

#### 可読性改善

- [ ] `is_hidden_relative` のネスト深度を 3 → 2 階層に削減
  - ファイル: `src/watcher/strategy.rs` L162-200
  - 現状: `match strip_prefix → match canonicalize(path) → match canonicalize(base)` の 3 段ネストで、canonicalize 失敗時のフォールバックログが 2 回重複
  - 対応: `try_relative_components(path, base) -> Option<impl Iterator<Component>>` 風のヘルパーを抽出し、呼び出し側は 1 回 match
  - 理由: 直前の watcher リファクタで隠し判定のロジックだけが旧形状のまま残っている

### Low Priority

#### リファクタ・ドキュメント整合性

- [ ] `render_markdown` の責務分割（大規模）
  - ファイル: `src/renderer/mod.rs` L184-540（約 357 行）
  - 現状: pulldown-cmark の `Event` ループと状態管理（heading / code block / table / image / link の各フェーズ）が 1 関数に同居。ファイル全体 951 行
  - 対応方針: フェーズ別ハンドラを `RenderState` の impl メソッドとして抽出、メイン関数はイベントディスパッチのみにする
  - 注意: 大規模リファクタ。既存テスト（`renderer_test.rs`, `toc_test.rs`）が振る舞い等価性を担保するため、先にテストカバレッジを確認
  - 理由: CLAUDE.md にも「見出しパースが 2 回実行される既知トレードオフ」が記載されており、renderer の保守重心は既に認識済み

- [ ] `catalog.rs` のパス構築での Vec アロケーション削減
  - ファイル: `src/server/files/catalog.rs` L127-128
  - 現状: 相対パス構築で `collect::<Vec<_>>()` してから `join("/")`。上限 1000 件だが呼出あたり Vec アロケーションが発生
  - 対応: イテレータ駆動で直接 String を構築する（`itertools::Itertools::join()` もしくは手書き fold）
  - 理由: マイクロ最適化。計測前に効果確認推奨

- [ ] README のアーキテクチャ図を実装構成に揃える
  - ファイル: `README.md` L121-125 周辺
  - 現状: `websocket.rs` / `renderer.rs` / `template.rs` / `files.rs` が単一ファイル前提で記載。実装は `src/server/session.rs`、`src/renderer/`（ディレクトリ）、`src/template/`（ディレクトリ）、`src/server/files/`（サブモジュール分割）
  - 対応: CLAUDE.md の「アーキテクチャ」節と同じ粒度で README を更新
  - 理由: ドキュメント rot。新規コントリビュータが実装構造を誤解する
