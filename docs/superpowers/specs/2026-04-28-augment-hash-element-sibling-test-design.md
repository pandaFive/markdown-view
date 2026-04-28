# augmentHashWithTrailingLineHint ELEMENT_NODE sibling 回帰テスト設計

**作成日**: 2026-04-28
**対象ファイル**: `tests/e2e/memo_jump.spec.ts`

## 目的

`docs/todo/BACKLOG.md` の P2 から、次の2項目を同時に処理する。

- `augmentHashWithTrailingLineHint` が `ELEMENT_NODE` sibling の `textContent` を読む仕様を直接テストで固定する。
- `memo_jump.spec.ts` 内の Codex review ID 参照コメントを、外部IDではなく保護したい false-positive 仕様の説明へ置き換える。

`src/template/assets/js/content.js` の `augmentHashWithTrailingLineHint` は、旧形式メモの `出典: [link](url#heading) L15` を救済するため、リンク直後 sibling の `textContent` から `L<n>` または `L<n>-L<m>` を読み、hash に行範囲を補完する。ドックコメント上は `TEXT_NODE` と `ELEMENT_NODE` の双方を対象にしているが、現行テストは `document.createTextNode(...)` 経路を中心に固定している。今回の追加テストで `span` sibling 経路を直接固定し、将来のリファクタで `ELEMENT_NODE` 分岐が落ちる退行を検出できるようにする。

## 非ゴール

- プロダクションコードは変更しない。
- Playwright 設定は変更しない。
- `tests/e2e/helpers.ts` への共通 helper 抽出は行わない。
- `BACKLOG.md` の完了反映はこの実装スコープに含めない。
- 実レンダリングされた旧形式メモのクリック挙動を新規 E2E として増やさない。

## 実装方針

`tests/e2e/memo_jump.spec.ts` の `augmentHashWithTrailingLineHint` 直接テスト群に、`ELEMENT_NODE` sibling 専用の positive test を1件追加する。

テスト内では `#memo-preview` に `a` 要素と `span` 要素を順に追加する。`span.textContent = ' L15'` とし、`link.nextSibling` が `ELEMENT_NODE` になる状態で `augmentHashWithTrailingLineHint(link, '#section-b')` を直接呼ぶ。期待値は `#section-b:L15` とする。

cleanup は既存の明示削除方針に合わせ、`finally` で `link.remove()` と `lineHint.remove()` を実行する。末尾ノード推測や `innerHTML` 復元は使わない。

既存の `L10 onwards` false-positive 回帰テストでは、外部 review system の ID 参照を削除する。代わりに、旧 regex が先頭の `L10` だけを拾うと散文を行番号として誤認するため、sibling 全体が行番号トークンだけで構成されることを固定する、という仕様説明へ置き換える。

## 受け入れ基準

- `tests/e2e/memo_jump.spec.ts` に `ELEMENT_NODE` sibling の直接テストが追加されている。
- 追加テストは `document.createElement('span')` を使い、`augmentHashWithTrailingLineHint(link, '#section-b')` が `#section-b:L15` を返すことを検証している。
- 追加テストは `#memo-preview` 配下で実行され、関数のスコープガードを迂回していない。
- 追加テストの cleanup は追加した `link` と `span` を明示的に削除している。
- Codex review ID のような外部IDコメントが `memo_jump.spec.ts` から消え、保護対象の仕様説明に置き換わっている。
- プロダクションコード、Playwright 設定、共通 helper には差分がない。

## 検証

実装後、対象 E2E を実行する。

```bash
npm run test:e2e -- memo_jump.spec.ts
```

必要に応じて通常検証も実行する。

```bash
./verify.sh
```

docs-only の本設計書は、次で構造確認する。

```bash
rg -n "T(BD|ODO)|#[0-9]{6,}" docs/superpowers/specs/2026-04-28-augment-hash-element-sibling-test-design.md
```

## セキュリティ考慮

今回の実装は test-only で、直接のセキュリティ境界変更ではない。

ただし `augmentHashWithTrailingLineHint` は URL hash を生成する補助経路である。テストでは、旧形式メモ救済を `#memo-preview` 配下に限定する前提、既存 hash に行範囲がある場合は上書きしない前提、散文を行番号として拾わない前提を維持する。これにより、本文リンクやユーザー自然文を誤ってジャンプ対象にする退行を検出しやすくする。

## 影響範囲

- 変更対象: `tests/e2e/memo_jump.spec.ts`
- 参照対象: `src/template/assets/js/content.js`
- 実装コードへの影響: なし
- テストコードへの影響: `memo_jump.spec.ts` の直接テストが1件増える
- ドキュメントへの影響: この設計書のみ

## ロールバック

実装差分は `tests/e2e/memo_jump.spec.ts` のテスト追加とコメント置換に閉じる。問題があれば、当該テスト追加とコメント差分だけを revert する。

この設計書は docs-only なので、設計コミットを revert すれば元に戻せる。
