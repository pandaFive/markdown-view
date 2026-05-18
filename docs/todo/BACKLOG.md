# Backlog (Low Priority)

低優先度で蓄積している項目。High/Medium は [`TODO.md`](./TODO.md) に置き、ここには低優先・長期改善の未完了候補を置く。
未完了項目は重要度と将来影響度を基準に P1/P2/P3 へ分類する。各項目末尾の「由来」は TODO.md 再編時（2026-04-21）以降の発見コンテキスト。

最終整理: 2026-05-09。セキュリティ境界、データ安全性、silent failure、監視不能に直接響く項目は `TODO.md` へ昇格した。ここには昇格しないが文脈を残すべき候補を置く。
完了済み履歴は [`DONE-2026-05.md`](../done/DONE-2026-05.md) に移動した。
レビュー由来の `現状` は作業候補として扱い、実装前に対象ファイル・行番号・現象を現行コードで再確認する。

## P1: リスク低減・契約明文化

## P2: 保守性・局所回帰検知

- [ ] ディレクトリ検索の allocation 削減を計測結果に基づいて検討する
  - ファイル: `src/server/files/search.rs`
  - 現状: ディレクトリ検索は `spawn_blocking` に隔離され、結果数・ファイル数・総読込 byte 数・query 長の打ち切りが明示されている。連続検索時の古い検索処理はサーバ側の検索世代と `SearchCancellation` により、ファイル単位の安全な区切りで協調的に早期終了できる
  - 対応: `SearchResultItem` の `before/current/after` がマッチごとに `String` を確保する点、検索ブロック抽出時の allocation、正規化処理のコストを計測したうえで、`Cow<str>` 化や処理単位の見直しを検討する
  - 判断: キャンセル境界は実装済み。残件は効率化であり、計測なしのマイクロ最適化を避けるため BACKLOG P2 に残す
  - 由来: ディレクトリ検索 blocking 隔離の残余リスク (2026-05-04)、ディレクトリ検索キャンセル境界実装 (2026-05-18)

- [ ] `AppMode` 構築時の `is_file()`/`is_dir()` 判定の TOCTOU を緩和する
  - ファイル: `src/server/state.rs` L18-24/L112/L132
  - 現状: `CanonicalPath::try_from_path` で `canonicalize` した直後に `is_file()`/`is_dir()` で判定するが、両者の間に rename/unlink される race window がある。実害は起動時の `AppMode::new_*` のみで影響は小さい
  - 対応: `metadata` を一度取得してから `is_file`/`is_dir` を判定し、race window を縮める。`AppModeBuildError` のメッセージも metadata 起点に整理
  - 判断: path safety に関係するが起動時限定で影響が小さいため BACKLOG P2 に残す
  - 由来: アーキテクチャレビュー (2026-04-30)

## P3: 長期改善・低緊急

- [ ] WS Host middleware bypass 兆候のメトリクス化を必要性ベースで検討する
  - ファイル: `src/server/guards.rs`
  - 現状: PR #123 で Host middleware 後段に到達した Host 系 `WsOriginRejection` を `error!` ログとして観測できるようにした。個人向け localhost ツールとしてはログで十分だが、本格運用や継続監視を想定するなら、発生回数をメトリクスやカウンタとして扱う余地がある
  - 対応: 実運用で bypass 兆候を集計する必要が出た場合のみ、軽量なカウンタや structured logging 連携を検討する。現時点では依存追加やメトリクス基盤導入は YAGNI とする
  - 判断: 既に error ログがあり、メトリクス基盤は実運用要求が出てからでよいため BACKLOG P3 に残す
  - 由来: PR #123 レビュー follow-up (2026-05-04)

- [ ] サイドバーの "Documents" 文字列を i18n または日本語化
  - ファイル: `src/server/routes.rs` L33-41 (`sidebar_directory_name`)
  - 現状: `unwrap_or("Documents")` で英語固定。日本語 UI でも同名が出る
  - 対応: 日本語デフォルト（"ドキュメント"）にするか、ディレクトリ名取得失敗時のフォールバック挙動をコメントで明示
  - 判断: UI 文言の局所改善であり、安全性や後続設計への影響は小さいため BACKLOG P3 に残す
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] インラインブラウザJS の TS 化
  - ファイル: `src/template/assets/js/{bootstrap,content,fetch,memo,selection,sidebar,websocket}.js`
  - 内容: Rust の `include_str!` でコンパイル時に埋め込まれる JS を TS で記述し、事前 tsc でビルドして `.js` 出力を `include_str!` 対象にする
  - 理由: ブラウザ側 JS は現在無型。ただし Rust ビルドパイプラインへの Node 依存追加が必要で、「Rust 単体ビルド」の明快さが崩れる
  - 判断: 型安全性の長期改善だが、Node 依存追加の設計判断が必要なため BACKLOG P3 に残す
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [ ] `catalog.rs` のパス構築での Vec アロケーション削減
  - ファイル: `src/server/files/catalog.rs`
  - 現状: 相対パス構築で `components().map(...).collect::<Vec<_>>().join("/")` を使っている。上限 1000 件だが呼出あたり Vec アロケーションが発生する
  - 対応: 計測または必要性確認のうえ、イテレータ駆動で直接 String を構築する（`itertools::Itertools::join()` もしくは手書き fold）
  - 判断: マイクロ最適化であり、実装前に効果確認が必要なため BACKLOG P3 に残す
  - 由来: PR #59 探索 (2026-04-18)

## Done

完了済み履歴は [`DONE-2026-05.md`](../done/DONE-2026-05.md) へ移動した。
