# TOC level=0 ガード設計

## Goal

`HeadingInfo.level` に `0` が渡っても `toc::build_toc_html` が panic しないことを回帰テストで固定する。現在の `level.max(1)` 正規化を仕様として明示し、将来のTOC生成ロジック変更でインデックスOOBが再導入されることを防ぐ。

## Non-Goals

- `HeadingInfo.level` を `NonZeroU8` へ型変更しない。
- TOC HTML の既存構造、改行、ネスト方針を変更しない。
- renderer 全体の見出し抽出フローや slug 生成には触れない。
- watcher変更ターゲット再検証プランの対象ファイルには触れない。

## Impact Scope

- 主対象: `src/renderer/toc.rs`
- 追加テスト: `toc.rs` 内の `#[cfg(test)] mod tests`
- 既存の外部テスト `tests/toc_test.rs` は必要がなければ変更しない。

## Design

`build_toc_html` は非公開関数であり、通常の Markdown パース経路からは `HeadingInfo.level=0` が生成されない。今回のリスクは現行バグではなく、`HeadingInfo.level: u8` が型として `0` を許しているため、将来の内部利用やリファクタで `level=0` を含む `HeadingInfo` が渡ったときに `open_li_at_level[(current_level - 1) as usize]` のようなインデックス操作が panic し得る点にある。

対応は `toc.rs` 内に境界テストを追加し、`HeadingInfo { level: 0, ... }` を直接 `build_toc_html` へ渡す。期待値は「panicしない」「`level=0` を h1 相当に正規化して `<ul>` と `<li><a href="#...">...</a>` を生成する」「id と text は既存どおり HTML escape される」とする。

実装コードは原則変更しない。必要な場合のみ、`level.min(current_level.saturating_add(1)).max(1)` の直前に短い日本語コメントを追加し、`0` は防御的に h1 扱いへ正規化することを明示する。

## Error Handling

`level=0` はエラー扱いにせず、既存挙動どおり安全側で `1` に正規化する。ユーザー入力由来の Markdown では到達不能な内部境界のため、エラー表示やログ追加は行わない。

## Security Considerations

TOC HTML は `SanitizedHtml` として扱われるため、境界テストでも `heading.id` と `heading.text` の escape 維持を確認する。`level=0` の防御は可用性面の hardening であり、不正な内部データでレンダリング処理が panic してプレビューや更新通知を落とすことを防ぐ。

## Testing

- `cargo test --all-targets --all-features toc`
- 最終検証: `./verify.sh`

追加するテストは `toc.rs` の private 関数へ直接アクセスできるユニットテストに置く。テスト名は日本語で、`level=0` が panic せず h1 相当になること、escape が維持されることを明示する。

## Acceptance Criteria

- `HeadingInfo.level=0` を含む入力で `build_toc_html` が panic しない。
- 生成TOCは既存の `<ul>` / `<li>` 構造を保つ。
- `heading.id` と `heading.text` の HTML escape が維持される。
- watcher変更ターゲット再検証プランの対象ファイルに変更を加えない。
- `cargo test --all-targets --all-features toc` と `./verify.sh` が成功する。

## Rollback Path

追加した `toc.rs` のテストと、もし追加した場合は正規化コメントだけを revert する。production behavior を変えない設計のため、rollback はテスト追加分の削除で完了する。
