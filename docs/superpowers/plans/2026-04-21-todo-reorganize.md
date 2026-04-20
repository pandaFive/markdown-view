# TODO.md 再編 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** TODO.md を High 4件 + Medium 5件 にフラット化し、Low 18件を新規 BACKLOG.md へ分離する。重複3件を統合、criticality ≤ 3 の冗長項目5件を削除する。

**Architecture:** 純粋な docs refactor。2ファイル（TODO.md を overwrite、BACKLOG.md を新規作成）を編集し、grep ベースで項目数と削除候補不在を検証。コード・テスト影響なし。

**Tech Stack:** Bash (grep 検証), git (commits)

**Design reference:** `docs/superpowers/specs/2026-04-21-todo-reorganize-design.md`

---

## 前提

- 作業ブランチ: `docs/reorganize-todo-md`（既に切ってある）
- 起点コミット: `4440593` (spec コミット済み)
- 現行 `docs/todo/TODO.md` は 255 行、ソースとして参照する

## タスク構成

- **Task 1**: BACKLOG.md を新規作成（Low 18 件）し、検証・コミット
- **Task 2**: TODO.md を全面書き換え（High 4 + Medium 5）し、検証・コミット
- **Task 3**: 最終クロスチェック
- **Task 4**: PR 作成

各項目の**フォーマット変換規則**（Task 1 / Task 2 共通）:
- 原文のタイトル行（`- [ ]` チェックボックス付き）と `- ファイル: ...` / `- 現状: ...` / `- 対応: ...` / `- 理由: ...` 等のサブ項目は**原文のまま保持**
- Low 項目では `- 優先度: Low（criticality X。...）` 行は**削除**（親セクションが優先度を示すため冗長）
- Low 項目の末尾に `- 由来: PR #XX レビュー (YYYY-MM-DD)` を**追加**（再編時の発見コンテキスト保持用）
- High / Medium 項目は 由来 行を追加**しない**（今後も更新される生きたリストのため）

---

### Task 1: BACKLOG.md を新規作成（Low 18件を移植）

**Files:**
- Create: `docs/todo/BACKLOG.md`
- Read (source): `docs/todo/TODO.md`

**Low 18 件の内訳と原文ソース行番号:**

| # | タイトル | 原文 L | 由来 |
|---|---------|-------|------|
| 1 | `augmentHashWithTrailingLineHint` ELEMENT_NODE sibling のユニットテスト | L29-33 | PR #76 レビュー (2026-04-20) |
| 2 | E2E テストの DOM クリーンアップ戦略見直し | L48-53 | PR #76 レビュー (2026-04-20) |
| 3 | 猶予期間中の連続 TOC クリックでの挙動検証 | L59-63 | PR #77 レビュー (2026-04-20) |
| 4 | `TOC_NAVIGATION_SLACK_PX` 境界の回帰テスト | L65-69 | PR #77 レビュー (2026-04-20) |
| 5 | L347 を active 遷移フラッシュ厳密検証に強化 | L71-75 | PR #77 レビュー (2026-04-20) |
| 6 | `window.updateContent` を E2E モード限定 expose に変更 | L98-102 | PR #80 レビュー (2026-04-20) |
| 7 | E2E の `declare global` ブロックを `tests/e2e/globals.d.ts` に集約 | L120-124 | E2E TypeScript 移行 PR レビュー (2026-04-20) |
| 8 | `updateContent` 型宣言の統一 | L126-130 | E2E TypeScript 移行 PR レビュー (2026-04-20) |
| 9 | `as unknown as` double-cast の説明コメント追加 | L137-141 | E2E TypeScript 移行 PR レビュー (2026-04-20) |
| 10 | `memo_jump.spec.ts:303` の Codex review ID `#4136142343` 削除 | L143-146 | E2E TypeScript 移行 PR レビュー (2026-04-20) |
| 11 | `tsconfig.json` に `noUncheckedIndexedAccess` / `exactOptionalPropertyTypes` を追加 | L148-153 | E2E TypeScript 移行 PR レビュー (2026-04-20) |
| 12 | E2E 共通ヘルパー (`resetFixtures`, `selectParagraphText` 等) を `tests/e2e/helpers.ts` に抽出 | L155-159 | E2E TypeScript 移行 PR レビュー (2026-04-20) |
| 13 | `TestWebSocket` を `tests/e2e/browser/test-websocket.ts` に抽出 | L161-165 | E2E TypeScript 移行 PR レビュー (2026-04-20) |
| 14 | E2E を `verify.sh` に統合するか検討 | L167-171 | E2E TypeScript 移行 PR レビュー (2026-04-20) |
| 15 | インラインブラウザJS (`src/template/assets/js/*.js`) の TS 化 | L173-177 | E2E TypeScript 移行 PR レビュー (2026-04-20) |
| 16 | `render_markdown` の責務分割（大規模） | L237-242 | PR #59 探索 (2026-04-18) |
| 17 | `catalog.rs` のパス構築での Vec アロケーション削減 | L244-248 | PR #59 探索 (2026-04-18) |
| 18 | README のアーキテクチャ図を実装構成に揃える | L250-254 | PR #59 探索 (2026-04-18) |

**削除候補（BACKLOG.md に含めない）:**
- 範囲形式 precedence テスト（現 L35-40）
- `!sibling` 早期 return テスト（現 L42-46）
- `contentEl` HTML 代入時の例外可視化（現 L104-108）
- 初回 broadcast 中クラス消失 edge case（現 L110-114）
- `export {};` 削除（現 L132-135）

- [ ] **Step 1: BACKLOG.md のヘッダ部のみを作成**

Write `docs/todo/BACKLOG.md`:

```markdown
# Backlog (Low Priority)

低優先度で蓄積している項目。High/Medium が TODO.md から捌けてから着手する候補。各項目末尾の「由来」は TODO.md 再編時（2026-04-21）の発見コンテキスト。

## Low Priority

```

- [ ] **Step 2: PR #76 残 Low 2件を追記**

原文 L29-33, L48-53 を BACKLOG.md に append。フォーマット変換規則を適用（`- 優先度: Low（...）` を削除、`- 由来: PR #76 レビュー (2026-04-20)` を追加）。

変換後の Item 1 の例（執筆者向けの期待フォーマット）:

```markdown
- [ ] `augmentHashWithTrailingLineHint` ELEMENT_NODE sibling のユニットテスト
  - ファイル: `tests/e2e/memo_jump.spec.js`
  - 行番号: L359 付近（既存 augmentHashWithTrailingLineHint テスト群と併設）
  - 理由: `src/template/assets/js/content.js` L181-L182 のドックコメント『renderer がソース行トラッキング用に text を `<span>` でラップするケースに対応』という設計意図を固定する直接テストが欠如している。現状は L205 の「旧形式メモ」E2E で実レンダ経由の TEXT_NODE パスのみカバー。`document.createElement('span')` で `L15` を内包したノードを sibling に置いて `textContent` 経路が生きることを明示的に検証する
  - 由来: PR #76 レビュー (2026-04-20)

```

(Item 2 の「E2E テストの DOM クリーンアップ戦略見直し」も同様に変換)

- [ ] **Step 3: PR #77 Low 3件を追記**

原文 L59-63, L65-69, L71-75 を BACKLOG.md に append。変換規則同じ。`- 由来: PR #77 レビュー (2026-04-20)` を追加。

- [ ] **Step 4: PR #80 残 Low 1件を追記**

原文 L98-102 を BACKLOG.md に append。変換規則同じ。`- 由来: PR #80 レビュー (2026-04-20)` を追加。

- [ ] **Step 5: E2E TS 移行 Low 9件を追記**

原文 L120-130, L137-177 を BACKLOG.md に append（L132-135 の `export {};` 削除項目はスキップ）。変換規則同じ。`- 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)` を追加。

**注意:** Item 10（Codex review ID 削除、原文 L143-146）は原文の「優先度: Low（既存 PR #76 で持ち込まれた既存課題、E2E TS 移行とは独立）」行を削除し、由来は `E2E TypeScript 移行 PR レビュー (2026-04-20、既存 PR #76 持ち込み課題)` と記載（原文の文脈注記を由来側に寄せる）。

- [ ] **Step 6: PR #59 探索 Low 3件を追記**

原文 L237-254 を BACKLOG.md に append。変換規則同じ。`- 由来: PR #59 探索 (2026-04-18)` を追加。

- [ ] **Step 7: BACKLOG.md 項目数の検証**

Run:
```bash
grep -c "^- \[ \]" docs/todo/BACKLOG.md
```

Expected output: `18`

FAIL した場合は項目の追記漏れ or 誤削除。Step 2-6 を確認。

- [ ] **Step 8: 削除候補5件が BACKLOG.md に含まれていないことを検証**

Run:
```bash
! grep -Fq "範囲形式 hash + sibling L の precedence" docs/todo/BACKLOG.md && \
! grep -Fq '!sibling` 早期 return' docs/todo/BACKLOG.md && \
! grep -Fq "contentEl への HTML 代入時の例外可視化" docs/todo/BACKLOG.md && \
! grep -Fq "初回 broadcast 中に付与済みクラスが消失" docs/todo/BACKLOG.md && \
! grep -Fq "export {};` 削除" docs/todo/BACKLOG.md && \
echo "OK: 削除候補5件すべて BACKLOG.md に含まれていない"
```

Expected output: `OK: 削除候補5件すべて BACKLOG.md に含まれていない`

FAIL（途中で no output）なら該当項目を BACKLOG.md から除去。

- [ ] **Step 9: 由来行が 18 件分存在することを検証**

Run:
```bash
grep -c "^  - 由来:" docs/todo/BACKLOG.md
```

Expected output: `18`

FAIL なら追記漏れ。

- [ ] **Step 10: BACKLOG.md の目視確認**

Run:
```bash
cat docs/todo/BACKLOG.md | head -30
wc -l docs/todo/BACKLOG.md
```

Expected: 18 件 × 平均 5-6 行 + ヘッダ 6 行 ≈ 100-120 行前後。

- [ ] **Step 11: BACKLOG.md をコミット**

コミットメッセージを `/tmp/commit-msg-backlog.txt` に書き出してから commit:

```bash
cat > /tmp/commit-msg-backlog.txt <<'EOF'
docs: BACKLOG.md を新規作成し Low 優先度 TODO 18 件を退避

変更内容:
- docs/todo/BACKLOG.md を新規作成
- TODO.md から Low 優先度 18 件を移植（削除候補5件を除外、各項目に由来注記を追加）

変更理由:
- TODO.md が4つのレビュー日ブロックに分散し Low 20件超で視界ノイズ化していたため、High/Medium を TODO.md に集中させるための前処理

影響範囲:
- docs/todo/BACKLOG.md のみ新規作成（TODO.md は次コミットで書き換え）
- コード・テスト影響なし

テスト結果: grep 検証（18件 + 削除候補不在）Pass
EOF

git add docs/todo/BACKLOG.md
git commit -F /tmp/commit-msg-backlog.txt
rm /tmp/commit-msg-backlog.txt
```

---

### Task 2: TODO.md を全面書き換え（High 4 + Medium 5）

**Files:**
- Modify (overwrite): `docs/todo/TODO.md`
- Read (source): 現行 `docs/todo/TODO.md`（git history から参照可）

**High 4 件の原文ソース行番号（すべて PR #59 探索由来）:**

| 順 | タイトル | 原文 L |
|----|---------|-------|
| 1 | `is_trusted_host` / `normalize_authority` の IPv6 網羅テストを追加 | L185-189 |
| 2 | メモ sidecar fallback 経路の超長ファイル名＋特殊文字テストを追加 | L191-195 |
| 3 | メモ API のボディ制限値を意図明文化し、境界テストを追加 | L197-201 |
| 4 | WebSocket close_code マッピングの統合テストを追加 | L203-207 |

順序根拠: セキュリティ境界のリスク順（DNS Rebinding → パストラバーサル → DoS境界 → プロトコル境界）。原文と同順。

**Medium 5 件の原文ソース行番号:**

| 順 | タイトル | 原文 L | 備考 |
|----|---------|-------|------|
| 1 | CSP フォールバック時の方針整理（fail-fast vs 現状運用） | L7-11 | PR #76 側を採用、PR #59 側 L213-217 は削除 |
| 2 | エラー経路ログのパス情報を base 相対化 | L13-17 | PR #76 側を採用、PR #59 側 L219-223 は削除 |
| 3 | `is_hidden_relative` のネスト深度を 3 → 2 階層に削減 | L21-25 | PR #76 側を採用、PR #59 側 L227-231 は削除 |
| 4 | `updateContent` の inverse case (file-switch / data.content 変更時) の再描画検証 | L82-87 | PR #80 由来 |
| 5 | `updateContent` で `data.content === undefined` を契約違反として明示ログ | L89-94 | PR #80 由来 |

順序根拠: サーバ側 infra (1-3) → フロントエンド content.js (4-5) の自然なファイル粒度。

- [ ] **Step 1: TODO.md のヘッダと High セクション骨格を作成**

Overwrite `docs/todo/TODO.md` with:

```markdown
# TODO Issues

レビュー指摘・コードベース探索で検出した改善項目のうち **High / Medium のみ**を優先度順に掲載。Low 項目は [`BACKLOG.md`](./BACKLOG.md) を参照。

## High Priority

```

- [ ] **Step 2: High 4件を原文のまま追記**

原文 L185-189, L191-195, L197-201, L203-207 をそのまま append（フォーマット変換なし、順序も原文通り）。

High 項目には `- 由来: ...` を追加**しない**（今後も使われる生きた項目のため）。

**期待される Item 1 の形式:**

```markdown
- [ ] `is_trusted_host` / `normalize_authority` の IPv6 網羅テストを追加
  - ファイル: `src/server/guards.rs`
  - 現状: L171-173 の `test_trusted_host_loopback_ipv6` が `[::1]` のみを検証
  - 追加観点: `[::1]:3000`（port 付き bracketed）、`::1`（非 bracketed）、`[fe80::1]`（非 loopback）、`[::1]:abc`（非数値 port）の 4 パターン
  - 理由: DNS Rebinding 対策の核。正規化エッジケースで想定外に通過するとセキュリティ境界が崩れる
```

- [ ] **Step 3: Medium セクション骨格を追記**

```markdown

## Medium Priority

```

- [ ] **Step 4: Medium 5件を原文のまま追記**

原文 L7-11, L13-17, L21-25, L82-87, L89-94 をそのまま append（順序は上表通り）。

Medium 項目にも `- 由来: ...` を追加**しない**。

**統合方針の確認**: PR #76 側（L7-11 等）と PR #59 側（L213-217 等）は本文がほぼ同一のため、PR #76 側をそのまま採用すれば内容的に統合完了。

- [ ] **Step 5: TODO.md 項目数の検証**

Run:
```bash
grep -c "^- \[ \]" docs/todo/TODO.md
```

Expected output: `9`（High 4 + Medium 5）

FAIL なら追記漏れ or 重複。

- [ ] **Step 6: BACKLOG.md の項目が TODO.md に漏れていないことを検証**

Run:
```bash
! grep -Fq "augmentHashWithTrailingLineHint ELEMENT_NODE sibling" docs/todo/TODO.md && \
! grep -Fq "DOM クリーンアップ戦略見直し" docs/todo/TODO.md && \
! grep -Fq "猶予期間中の連続 TOC クリック" docs/todo/TODO.md && \
! grep -Fq "render_markdown` の責務分割" docs/todo/TODO.md && \
echo "OK: BACKLOG 側の代表項目が TODO.md に残存していない"
```

Expected output: `OK: BACKLOG 側の代表項目が TODO.md に残存していない`

- [ ] **Step 7: 削除候補5件が TODO.md にも残っていないことを検証**

Run:
```bash
! grep -Fq "範囲形式 hash + sibling L の precedence" docs/todo/TODO.md && \
! grep -Fq '!sibling` 早期 return' docs/todo/TODO.md && \
! grep -Fq "contentEl への HTML 代入時の例外可視化" docs/todo/TODO.md && \
! grep -Fq "初回 broadcast 中に付与済みクラスが消失" docs/todo/TODO.md && \
! grep -Fq "export {};` 削除" docs/todo/TODO.md && \
echo "OK: 削除候補5件すべて TODO.md に含まれていない"
```

Expected: `OK: 削除候補5件すべて TODO.md に含まれていない`

- [ ] **Step 8: PR #59 側の重複記述3件が削除されていることを検証**

PR #59 側は PR #76 側と同じ本文だが、L213-217 の CSP 節には「PR #76 と PR #59 で同内容」の識別用語句がない。代わりに**重複しないテキスト**で検証する：PR #59 側には節見出し `#### セキュリティ・堅牢性`（L211）がある構造で、PR #76 側には同節見出し（L5）はあるが本文は同じ。

→ 項目数 9 件を担保している Step 5 で実質的に検証済み（重複があれば項目数が 12 件になる）。追加検証不要。

- [ ] **Step 9: TODO.md の目視確認**

Run:
```bash
cat docs/todo/TODO.md
wc -l docs/todo/TODO.md
```

Expected: 100 行前後（現 255 行から約 60% 削減）。

- [ ] **Step 10: TODO.md をコミット**

```bash
cat > /tmp/commit-msg-todo.txt <<'EOF'
docs: TODO.md を High/Medium のみにフラット化して再編

変更内容:
- docs/todo/TODO.md を全面書き換え（9件: High 4 + Medium 5）
- レビュー日ベースの4ブロック構造を廃止し優先度順フラット1本化
- PR #76 と PR #59 探索の重複3件（CSP, エラーログ base 相対化, is_hidden_relative ネスト削減）を統合
- Low 18件は前コミットで BACKLOG.md に退避済み
- 冗長項目5件（criticality ≤ 3 / cosmetic 自己申告）を削除

変更理由:
- Low 20件超で視界ノイズ化していた現状を解消し「次に何をやるかをすぐ判断できる」状態にする

影響範囲:
- docs/todo/TODO.md のみ（コード・テスト影響なし）
- /ex-todo コマンドの対象は引き続き TODO.md のみで BACKLOG.md は対象外

テスト結果: grep 検証（9件、BACKLOG 側不在、削除候補不在）Pass
EOF

git add docs/todo/TODO.md
git commit -F /tmp/commit-msg-todo.txt
rm /tmp/commit-msg-todo.txt
```

---

### Task 3: 最終クロスチェック

**Files:** なし（検証のみ）

- [ ] **Step 1: 両ファイルの総項目数を確認**

Run:
```bash
echo "TODO.md: $(grep -c '^- \[ \]' docs/todo/TODO.md) items"
echo "BACKLOG.md: $(grep -c '^- \[ \]' docs/todo/BACKLOG.md) items"
echo "合計: $(($(grep -c '^- \[ \]' docs/todo/TODO.md) + $(grep -c '^- \[ \]' docs/todo/BACKLOG.md))) items"
```

Expected:
```
TODO.md: 9 items
BACKLOG.md: 18 items
合計: 27 items
```

根拠: 原 TODO.md の項目数 = High 4 + Medium 2 (PR #80) + Low 20 + 重複 Medium 3 (PR #59) + 可読性改善 3 (L7-25, PR #76) = 32 件。うち削除 5 件、重複統合で 3 件減 → 32 - 5 - 3 = 24 件... あれ？

改めて原 TODO.md を数える:
- PR #76: 3 (CSP + エラーログ + is_hidden_relative) + 4 (Low) = 7
- PR #77: 3 (Low)
- PR #80: 2 (Medium) + 3 (Low) = 5
- E2E TS: 10 (Low)
- PR #59: 4 (High) + 3 (Medium、うち3件は PR #76 と重複) + 3 (Low) = 10

計: 7 + 3 + 5 + 10 + 10 = 35 件

内訳再計算:
- 高優先度: 4 (PR #59)
- Medium 相当: 3 (PR #76 = PR #59 重複) + 2 (PR #80) = 5（重複除去済み）
- Low: 4 + 3 + 3 + 10 + 3 = 23 件

重複分: PR #59 側の Medium 3 件を削除 → 35 - 3 = 32 件
削除候補: Low から 5 件削除 → 32 - 5 = 27 件

内訳: High 4 + Medium 5 + Low 18 = 27 件 ✓

上記 Expected が正しい。

- [ ] **Step 2: TODO.md に High/Medium のみが存在することを確認**

Run:
```bash
grep "^##" docs/todo/TODO.md
```

Expected output:
```
## High Priority
## Medium Priority
```

「Low Priority」見出しが現れないこと。

- [ ] **Step 3: BACKLOG.md に Low のみが存在することを確認**

Run:
```bash
grep "^##" docs/todo/BACKLOG.md
```

Expected output:
```
## Low Priority
```

- [ ] **Step 4: verify.sh を実行して回帰なしを確認**

Run:
```bash
./verify.sh
```

Expected: Pass（docs のみの変更なので Rust ビルド・E2E 型チェック共に影響なし）。

- [ ] **Step 5: git log でコミット構成を確認**

Run:
```bash
git log --oneline -5
```

Expected:
```
<hash> docs: TODO.md を High/Medium のみにフラット化して再編
<hash> docs: BACKLOG.md を新規作成し Low 優先度 TODO 18 件を退避
4440593 docs: TODO.md 再編の設計ドキュメントを追加
...
```

3 コミットがこのブランチで積まれていること。

---

### Task 4: PR 作成

**Files:** なし

- [ ] **Step 1: リモートに push**

```bash
git push -u origin docs/reorganize-todo-md
```

- [ ] **Step 2: PR を作成**

```bash
cat > /tmp/pr-body.txt <<'EOF'
## Summary
- TODO.md を High 4件 + Medium 5件 にフラット化し、Low 18件を新規 BACKLOG.md へ分離
- PR #76 と PR #59 探索の重複 3件（CSP フォールバック、エラーログ base 相対化、`is_hidden_relative` ネスト削減）を統合
- criticality ≤ 3 または cosmetic 自己申告の冗長項目 5件を削除

**再編後の状態**: TODO.md 9件 / BACKLOG.md 18件（合計 27件）

## 設計ドキュメント
- `docs/superpowers/specs/2026-04-21-todo-reorganize-design.md`
- `docs/superpowers/plans/2026-04-21-todo-reorganize.md`

## Test plan
- [ ] `./verify.sh` Pass（docs のみの変更、Rust/E2E 影響なし）
- [ ] `docs/todo/TODO.md` の目視確認（High 4件 + Medium 5件が優先度順に並ぶ）
- [ ] `docs/todo/BACKLOG.md` の目視確認（Low 18件 + 由来注記）
- [ ] `/ex-todo` コマンドが TODO.md から High 項目を拾うことを手動確認
EOF

gh pr create --title "docs: TODO.md を High/Medium にフラット化し Low を BACKLOG.md に分離" --body-file /tmp/pr-body.txt --base develop
rm /tmp/pr-body.txt
```

- [ ] **Step 3: PR URL を返す**

PR 作成コマンドの出力から URL を報告。

---

## Self-Review チェックリスト（プラン作成者が最後に確認）

- [x] **Spec coverage:** spec の目的・スコープ・削除・統合・最終構成・成功基準すべてが Task 1-3 の検証 step で担保されている
- [x] **Placeholder scan:** "TBD" / "TODO" / "implement later" 等の placeholder なし
- [x] **Type consistency:** ファイル名・パスはすべて `docs/todo/TODO.md` / `docs/todo/BACKLOG.md` で統一
- [x] **削除候補5件**: Task 1 Step 8 と Task 2 Step 7 の両方で grep 検証
- [x] **重複3件の統合**: Task 2 Step 5（項目数 = 9 件）で実質検証
- [x] **PR 手順**: CLAUDE.md のブランチ運用（develop ターゲット、feature ブランチ、squash merge 想定）に準拠

## リスク・注意事項

1. **grep パターンのエスケープ**: 削除候補検証で使う `export {};` や `!sibling` はシェル特殊文字を含む。Task 1 Step 8 / Task 2 Step 7 の grep コマンドは single quote で囲んであるが、実行時に clippy / shellcheck 的な警告は無視して可。
2. **段階コミット**: BACKLOG.md 先 → TODO.md 後の順序は意図的。逆順にすると TODO.md から Low が消えて BACKLOG.md にまだ入っていない中間状態が生まれ、「ファイルをまたいだ人の任意のタイミングで全項目が把握可能」を崩す。
3. **/ex-todo コマンドへの影響**: リスク節（spec）で記述した通り、BACKLOG.md は `/ex-todo` の対象外のままとする。Low 項目は意図的に「拾われない」。
