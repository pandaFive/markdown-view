# Backlog (Low Priority)

低優先度で蓄積している項目。High/Medium は [`TODO.md`](./TODO.md) に置き、ここには低優先・長期改善の未完了候補を置く。
未完了項目は重要度と将来影響度を基準に P1/P2/P3 へ分類する。各項目末尾の「由来」は TODO.md 再編時（2026-04-21）以降の発見コンテキスト。

最終整理: 2026-05-09。セキュリティ境界、データ安全性、silent failure、監視不能に直接響く項目は `TODO.md` へ昇格した。ここには昇格しないが文脈を残すべき候補を置く。
過去の完了済み履歴は [`DONE-2026-05.md`](../done/DONE-2026-05.md) に移動した。
レビュー由来の `現状` は作業候補として扱い、実装前に対象ファイル・行番号・現象を現行コードで再確認する。

## P1: リスク低減・契約明文化

## P2: 保守性・局所回帰検知

- [ ] ディレクトリ検索の allocation 削減を追加計測に基づいて再判断する
  - ファイル: `src/server/files/search.rs`
  - 現状: ディレクトリ検索は `spawn_blocking` に隔離され、結果数・ファイル数・総読込 byte 数・query 長の打ち切りが明示されている。連続検索時の古い検索処理はクライアント単位のサーバ側検索世代と `SearchCancellation` により、ファイル単位の安全な区切りで協調的に早期終了できる。検索クライアント ID がない互換呼び出しも、全体同時実行上限により blocking 検索の増加を抑える
  - 計測済み: 依存追加なしで検索系 targeted tests と HTTP `/api/search` 経由の代表 fixture 計測を行った。環境は `rustc 1.93.1 (01f6ddf75 2026-02-11)`, `cargo 1.93.1 (083ac5135 2025-12-15)`, Linux WSL2 `5.15.133.1-microsoft-standard-WSL2`。検索系 test elapsed は 0.52-0.68s、最大 RSS は 74620-74776 KiB。HTTP result-limit 経路（`q=needle`）は `searched_files=1`, `searched_bytes=158411`, `truncated=true`, `truncated_reasons=["result_limit"]`, elapsed 0.03s x3, response 47778 bytes, 対象 server RSS 47988 KiB。HTTP full-scan 経路（`q=absentneedle`）は `searched_files=260`, `searched_bytes=7604305`, `truncated=false`, `truncated_reasons=[]`, elapsed 1.18-1.21s, response 209 bytes。対象 server RSS は起動直後 16156 KiB、初回 full-scan 後 33764 KiB、複数回 full-scan 後 51324 KiB。fixture は `/tmp` に生成し、repo へ追加していない。別の既存 `markdown-view .` process は計測対象外として触らなかった
  - 残件: 今回の 260 files / 7.8M 代表 fixture は追加計測範囲を絞る材料に留まり、対象 server RSS の plateau 未確認を含むため、不要判断も最適化判断も保留する。複数ファイルに分散した 100 件 result-limit、64 MiB 近傍 full-scan または byte-limit 近傍、10 MiB 単一ファイル、RSS plateau の追加確認を行ったうえで、`Cow<str>` 化や検索ブロック処理単位変更の要否を再判断する
  - 判断: 検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は変更していない。現時点では完了扱いにせず、追加計測が必要な P2 残件として残す
  - 由来: ディレクトリ検索 blocking 隔離の残余リスク (2026-05-04)、ディレクトリ検索キャンセル境界実装 (2026-05-18)、ディレクトリ検索 allocation 代表 fixture 計測 (2026-05-19)

## P3: 長期改善・低緊急

- [ ] WS Host middleware bypass 兆候のメトリクス化を必要性ベースで検討する
  - ファイル: `src/server/guards.rs`
  - 現状: PR #123 で Host middleware 後段に到達した Host 系 `WsOriginRejection` を `error!` ログとして観測できるようにした。個人向け localhost ツールとしてはログで十分だが、本格運用や継続監視を想定するなら、発生回数をメトリクスやカウンタとして扱う余地がある
  - 対応: 実運用で bypass 兆候を継続集計する必要が出た場合のみ、軽量なカウンタや structured logging 連携を検討する。現時点では既存の `error!` ログ、`ws_rejection_class`、`host_recheck_anomaly` field で異常兆候を確認でき、依存追加やメトリクス基盤導入は YAGNI とする
  - 判断: 既に error ログがあり、メトリクス基盤は実運用要求が出てからでよいため BACKLOG P3 に残す
  - 由来: PR #123 レビュー follow-up (2026-05-04)

## Done

- [x] インラインブラウザJS の TS 化
  - 完了根拠: `src/template/assets/ts/*.ts` を正ソースにし、`build.rs` が `npm run build:inline-js` 経由で Cargo `OUT_DIR` 配下へ生成した JS を `inline_script.rs` へ埋め込む構成にした。生成 `.js` はリポジトリに保持せず、`npm run typecheck` で E2E とインライン JS の両方を検査する。既存の結合順序、`__MAX_FILE_SIZE_MB__` sentinel 置換、CSP hash、`innerHTML` sink allowlist、E2E hook production 非公開契約は維持している。検証では `npm run build:inline-js`、`npm run typecheck`、`cargo test --lib template::assets::inline_script` は通過したが、指定の `cargo test --test update_content_exposure` は該当 Cargo test target が存在せず失敗した。`./verify.sh` と `./verify.sh --e2e` は `template::page` の既存 HTML 内 JS 文字列アサーション 4 件で停止し、E2E 実行段階までは到達していない

- [x] `AppMode` 構築時の `is_file()`/`is_dir()` 判定の TOCTOU を緩和する
  - 完了根拠: `src/server/state.rs` は `CanonicalPath::try_from_path()` で canonicalize した後、`ensure_canonical_file()` / `ensure_canonical_directory()` が `metadata_for_mode()` 経由で取得した `Metadata` の `file_type()` から file / directory を判定している。canonicalize 後に対象が消えた場合も `NotFile` / `NotDirectory` へ集約することを unit test で固定済み。`.md` 拡張子チェック、canonical path 保持、base_dir / single_file / directory の公開契約、Host/Origin 検証、HTML sanitize、CSP、path validation は変更していない

- [x] サイドバーの "Documents" fallback を日本語化
  - 完了根拠: `src/server/service.rs` の `sidebar_directory_name()` fallback を private 定数 `SIDEBAR_DIRECTORY_FALLBACK_NAME` 経由の `"ドキュメント"` に変更した。通常のディレクトリ名が取得できる場合は従来どおり実ディレクトリ名を使うことを unit test で固定した。i18n 基盤、UI 全体の文言、HTML sanitize、path validation は変更していない

- [x] `catalog.rs` の相対パス構築で中間 Vec allocation を避ける
  - 完了根拠: `src/server/files/catalog.rs` に `relative_path_to_slash_string()` を追加し、catalog.rs の相対パス構築で `components().map(...).collect::<Vec<_>>().join("/")` を使わずに `/` 区切り文字列を構築するようにした。ファイル列挙の sort、件数上限、除外ルール、canonicalize 再検証、symlink handling は変更していない。helper の単一 component とネスト path の出力を unit test で固定した

過去の完了済み履歴は [`DONE-2026-05.md`](../done/DONE-2026-05.md) へ移動した。
