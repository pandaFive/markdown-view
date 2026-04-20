# TODO Issues

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
