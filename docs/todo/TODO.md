# TODO Issues

## E2E テスト失敗（2026-04-20 発見）

`npx playwright test` 全体実行で 5 件の失敗を確認。develop baseline でも 3 件が再現するため PR #76 の変更起因ではなく pre-existing な不具合または flake。`./verify.sh` には E2E が含まれないため CI で検知されていない。

### High Priority

- [ ] `markdown_links.spec.js:124` 壊れたフラグメントリンクの hash クリア + 警告動作
  - ファイル: `tests/e2e/markdown_links.spec.js`
  - 行番号: L124（テスト開始行）
  - 症状: 同一ファイル内の存在しないフラグメントリンクで URL hash クリアと警告が期待通りに動作しない
  - 再現: `npx playwright test tests/e2e/markdown_links.spec.js:124`
  - 再現性: develop でも再現（stable）
  - 理由: markdown 内リンクの基本 UX、壊れたアンカーの fallback 挙動
  - 優先度: High

- [ ] `document_search.spec.js:389` ディレクトリモード検索 API 結果の一覧表示
  - ファイル: `tests/e2e/document_search.spec.js`
  - 行番号: L389（テスト開始行）
  - 症状: ディレクトリモードで `/api/search` 結果の一覧描画が期待通りにならない
  - 再現: `npx playwright test tests/e2e/document_search.spec.js:389`
  - 再現性: develop でも再現（stable）
  - 理由: 検索機能の E2E 検証、ディレクトリモード固有の描画経路
  - 優先度: High

### Medium Priority

- [ ] Flaky E2E テスト 2 件の安定化
  - ファイル:
    - `tests/e2e/markdown_links.spec.js:41` ディレクトリモード相対リンクフラグメント遷移
    - `tests/e2e/memo_sync.spec.js:48` 別ページへのメモ更新同期
  - 症状: 同じコマンドを連続実行すると pass/fail が揺れる
  - 再現: `npx playwright test tests/e2e/markdown_links.spec.js:41` を複数回実行
  - 方針: `test.retry(2)` で許容せず、待機条件（selector の安定化 / broadcast 到達確認）を特定して根治
  - 理由: `./verify.sh` に E2E を組み込む前提で flake は許容しない
  - 優先度: Medium

## TODO Issues (レビュー日: 2026-04-20, PR #76 レビュー)

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

- [ ] 日本語 id の TOC クリックで popstate ガードが動作する検証
  - ファイル: `tests/e2e/text_selection_defer.spec.js`
  - 内容: `markdown_links.spec.js` L253 の日本語見出しテストは cross-file 分岐のみ。同一ファイル内 TOC クリックで `pendingTocNavigationId` と `location.hash` の URL エンコーディングが一致することの直接検証がない
  - 理由: ブラウザ/Playwright の hash エンコーディング変化や、将来 id encoding 変更時のリグレッション検知
  - 優先度: Low（criticality 5。Chromium 現行版では encoded 統一）

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
