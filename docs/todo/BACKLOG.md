# Backlog (Low Priority)

低優先度で蓄積している項目。High/Medium は [`TODO.md`](./TODO.md) に置き、ここには低優先・長期改善の未完了候補を置く。
未完了項目は重要度と将来影響度を基準に P1/P2/P3 へ分類する。各項目末尾の「由来」は TODO.md 再編時（2026-04-21）以降の発見コンテキスト。

最終整理: 2026-05-09。セキュリティ境界、データ安全性、silent failure、監視不能に直接響く項目は `TODO.md` へ昇格した。ここには昇格しないが文脈を残すべき候補を置く。
過去の完了済み履歴は [`DONE-2026-05.md`](../done/DONE-2026-05.md) に移動した。
レビュー由来の `現状` は作業候補として扱い、実装前に対象ファイル・行番号・現象を現行コードで再確認する。

## P1: リスク低減・契約明文化

## P2: 保守性・局所回帰検知

- [ ] ディレクトリ検索の巨大単一ブロック many-match 経路を処理単位と早期打ち切り観点で見直す
  - ファイル: `src/server/files/search.rs`
  - 現状: ディレクトリ検索は `spawn_blocking` に隔離され、結果数・ファイル数・総読込 byte 数・query 長の打ち切りが明示されている。連続検索時の古い検索処理はクライアント単位のサーバ側検索世代と `SearchCancellation` により、ファイル単位の安全な区切りで協調的に早期終了できる。検索クライアント ID がない互換呼び出しも、全体同時実行上限により blocking 検索の増加を抑える
  - 計測済み: 2026-05-23 に依存追加なしで追加計測した。環境は `rustc 1.93.1 (01f6ddf75 2026-02-11)`, `cargo 1.93.1 (083ac5135 2025-12-15)`, Linux WSL2 `5.15.133.1-microsoft-standard-WSL2`。検索系 targeted tests は sandbox 内 warm run が pass、`/usr/bin/time -v` 付き計測は loopback bind のため sandbox 外で 3 回実行し pass、elapsed は 0.50-0.60s、最大 RSS は 74528-74868 KiB。複数ファイル分散 result-limit fixture は 120 files / 484 KiB、HTTP `q=needle` で `searched_files=100`, `searched_bytes=13200`, `truncated=true`, `truncated_reasons=["result_limit"]`, elapsed 0.01s x5, response 20526 bytes, server RSS は起動直後 16148 KiB から 5 回後も 16148 KiB。64 MiB 近傍 byte-limit fixture は 70 files / 102 MiB、HTTP `q=missingneedle` で `searched_files=44`, `searched_bytes=66924836`, `truncated=true`, `truncated_reasons=["byte_limit"]`, elapsed 8.96-11.36s, response 221 bytes。server RSS は 5 回計測で 18120 → 147844 → 206976 → 201132 → 210640 → 226696 KiB、追加 8 回計測でも 16196 → 107568 → 196004 → 210024 → 232952 → 213636 → 225812 → 215236 → 249120 KiB となり plateau 判定には不足した。10 MiB 単一ファイル fixture は 1 file / 10388017 bytes。many-match 経路の HTTP `q=needle` は `searched_files=1`, `searched_bytes=10388017`, `truncated=true`, `truncated_reasons=["result_limit"]`, response 39512 bytes, elapsed 36.73-38.37s, server RSS 887312 → 1329640 → 1186668 KiB。no-match 経路の HTTP `q=missingneedle` は `searched_files=1`, `searched_bytes=10388017`, `truncated=false`, `truncated_reasons=[]`, response 209 bytes, elapsed 1.39-1.47s, server RSS 1193316 → 1195364 → 1187484 KiB。fixture は `/tmp/markdown-view-search-allocation-upper-limit.***` に生成し、repo へ追加していない
  - 残件: 複数ファイル分散 result-limit は実用範囲だったが、64 MiB 近傍 byte-limit の server RSS は plateau と判断できず、10 MiB 単一ファイルの many-match 経路は result-limit 100 件にもかかわらず 36 秒超かつ server RSS 1 GiB 超になった。`Cow<str>` 化だけでなく、巨大単一ブロックで全 match / context を作る前に result-limit へ到達できる処理単位、検索ブロック分割、file 内 match 列挙の早期停止、RSS 回収・plateau の再計測を設計する
  - 判断: allocation 削減不要とは判断せず、Done 化しない。検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は変更していない。今回の計測結果から、最適化対象は一般的な小規模検索ではなく、10 MiB 近傍の巨大単一ブロック many-match と 64 MiB 近傍 byte-limit 反復時の RSS plateau に絞る
  - 由来: ディレクトリ検索 blocking 隔離の残余リスク (2026-05-04)、ディレクトリ検索キャンセル境界実装 (2026-05-18)、ディレクトリ検索 allocation 代表 fixture 計測 (2026-05-19)、ディレクトリ検索 allocation 上限近傍計測 (2026-05-23)

## P3: 長期改善・低緊急

- [ ] WS Host middleware bypass 兆候のメトリクス化を必要性ベースで検討する
  - ファイル: `src/server/guards.rs`
  - 現状: PR #123 で Host middleware 後段に到達した Host 系 `WsOriginRejection` を `error!` ログとして観測できるようにした。個人向け localhost ツールとしてはログで十分だが、本格運用や継続監視を想定するなら、発生回数をメトリクスやカウンタとして扱う余地がある
  - 対応: 実運用で bypass 兆候を継続集計する必要が出た場合のみ、軽量なカウンタや structured logging 連携を検討する。現時点では既存の `error!` ログ、`ws_rejection_class`、`host_recheck_anomaly` field で異常兆候を確認でき、依存追加やメトリクス基盤導入は YAGNI とする
  - 判断: 既に error ログがあり、メトリクス基盤は実運用要求が出てからでよいため BACKLOG P3 に残す
  - 由来: PR #123 レビュー follow-up (2026-05-04)

## Done

- [x] インラインブラウザJS の TS 化
  - 完了根拠: `src/template/assets/ts/*.ts` を正ソースにし、`build.rs` が `MV_INLINE_JS_OUT_DIR` 付きの `npm run build:inline-js` 経由で Cargo `OUT_DIR` 配下へ生成した JS を `inline_script.rs` へ埋め込む構成にした。生成 `.js` はリポジトリに保持せず、`npm run typecheck` で E2E とインライン JS の両方を検査する。既存の結合順序、`__MAX_FILE_SIZE_MB__` sentinel 置換、CSP hash、`innerHTML` sink allowlist、E2E hook production 非公開契約は維持している。検証は `MV_INLINE_JS_OUT_DIR="$(mktemp -d)" npm run build:inline-js`、`npm run typecheck`、`cargo test --lib template::assets::inline_script`、`npx playwright test tests/e2e/update_content_exposure.spec.ts`、`npx playwright test tests/e2e/document_search.spec.ts`、`./verify.sh`、`./verify.sh --e2e` が通過した。

- [x] `AppMode` 構築時の `is_file()`/`is_dir()` 判定の TOCTOU を緩和する
  - 完了根拠: `src/server/state.rs` は `CanonicalPath::try_from_path()` で canonicalize した後、`ensure_canonical_file()` / `ensure_canonical_directory()` が `metadata_for_mode()` 経由で取得した `Metadata` の `file_type()` から file / directory を判定している。canonicalize 後に対象が消えた場合も `NotFile` / `NotDirectory` へ集約することを unit test で固定済み。`.md` 拡張子チェック、canonical path 保持、base_dir / single_file / directory の公開契約、Host/Origin 検証、HTML sanitize、CSP、path validation は変更していない

- [x] サイドバーの "Documents" fallback を日本語化
  - 完了根拠: `src/server/service.rs` の `sidebar_directory_name()` fallback を private 定数 `SIDEBAR_DIRECTORY_FALLBACK_NAME` 経由の `"ドキュメント"` に変更した。通常のディレクトリ名が取得できる場合は従来どおり実ディレクトリ名を使うことを unit test で固定した。i18n 基盤、UI 全体の文言、HTML sanitize、path validation は変更していない

- [x] `catalog.rs` の相対パス構築で中間 Vec allocation を避ける
  - 完了根拠: `src/server/files/catalog.rs` に `relative_path_to_slash_string()` を追加し、catalog.rs の相対パス構築で `components().map(...).collect::<Vec<_>>().join("/")` を使わずに `/` 区切り文字列を構築するようにした。ファイル列挙の sort、件数上限、除外ルール、canonicalize 再検証、symlink handling は変更していない。helper の単一 component とネスト path の出力を unit test で固定した

過去の完了済み履歴は [`DONE-2026-05.md`](../done/DONE-2026-05.md) へ移動した。
