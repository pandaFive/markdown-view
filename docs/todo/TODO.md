# TODO Issues

## E2E テスト失敗（2026-04-20 発見）

`npx playwright test` 全体実行で 5 件の失敗を確認。develop baseline でも 3 件が再現するため PR #76 の変更起因ではなく pre-existing な不具合または flake。`./verify.sh` には E2E が含まれないため CI で検知されていない。

### High Priority

- [ ] `text_selection_defer.spec.js:333` 目次クリック直後の逆方向スクロール判定が誤動作
  - ファイル: `tests/e2e/text_selection_defer.spec.js`
  - 行番号: L333-L345（テスト本体）、L344 でアサーション失敗
  - 症状: `expect.poll(() => activeTocLabel(page)).toBe('Alpha')` が `'Beta'` のまま timeout (5000ms)
  - 再現: `npx playwright test tests/e2e/text_selection_defer.spec.js:333`
  - 再現性: develop でも再現（stable）
  - 理由: TOC ナビゲーションの主要動線。クリック直後に逆方向スクロールで通常判定に戻る仕様の回帰
  - 優先度: High

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
