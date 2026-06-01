# Backlog (Low Priority)

低優先度で蓄積している項目。High/Medium は [`TODO.md`](./TODO.md) に置き、ここには低優先・長期改善の未完了候補を置く。
未完了項目は重要度と将来影響度を基準に P1/P2/P3 へ分類する。各項目末尾の「由来」は TODO.md 再編時（2026-04-21）以降の発見コンテキスト。

最終整理: 2026-05-09。セキュリティ境界、データ安全性、silent failure、監視不能に直接響く項目は `TODO.md` へ昇格した。ここには昇格しないが文脈を残すべき候補を置く。
過去の完了済み履歴は [`DONE-2026-05.md`](../done/DONE-2026-05.md) に移動した。
レビュー由来の `現状` は作業候補として扱い、実装前に対象ファイル・行番号・現象を現行コードで再確認する。

## P1: リスク低減・契約明文化

## P2: 保守性・局所回帰検知

- [ ] ディレクトリ検索 64MiB byte-limit 反復時の RSS plateau を測定方法改善込みで再確認する
  - ファイル: `src/server/files/search.rs`
  - 現状: ディレクトリ検索は `spawn_blocking` に隔離され、結果数・ファイル数・総読込 byte 数・query 長の打ち切りが明示されている。連続検索時の古い検索処理はクライアント単位のサーバ側検索世代と `SearchCancellation` により、ファイル単位の安全な区切りで協調的に早期終了できる。検索クライアント ID がない互換呼び出しも、全体同時実行上限により blocking 検索の増加を抑える
  - 計測済み: 2026-05-23 に依存追加なしで追加計測した。環境は `rustc 1.93.1 (01f6ddf75 2026-02-11)`, `cargo 1.93.1 (083ac5135 2025-12-15)`, Linux WSL2 `5.15.133.1-microsoft-standard-WSL2`。検索系 targeted tests は sandbox 内 warm run が pass、`/usr/bin/time -v` 付き計測は loopback bind のため sandbox 外で 3 回実行し pass、elapsed は 0.50-0.60s、最大 RSS は 74528-74868 KiB。複数ファイル分散 result-limit fixture は 120 files / 484 KiB、HTTP `q=needle` で `searched_files=100`, `searched_bytes=13200`, `truncated=true`, `truncated_reasons=["result_limit"]`, elapsed 0.01s x5, response 20526 bytes, server RSS は pre-search 16148 KiB、同一 server process の run1-run5 後も 16148 KiB。64 MiB 近傍 byte-limit fixture は fixture 自体が 70 files / 102 MiB で、既存 byte-limit により約 64 MiB で打ち切られた。HTTP `q=missingneedle` は `searched_files=44`, `searched_bytes=66924836`, `truncated=true`, `truncated_reasons=["byte_limit"]`, elapsed 8.96-11.36s, response 221 bytes。server RSS は 1 回目の server process で pre-search 18120 KiB → run1 147844 → run2 206976 → run3 201132 → run4 210640 → run5 226696 KiB。追加 8 回は server restart 後の別 process で pre-search 16196 KiB → run1 107568 → run2 196004 → run3 210024 → run4 232952 → run5 213636 → run6 225812 → run7 215236 → run8 249120 KiB となり plateau 判定には不足した。10 MiB 近傍 many-match 経路は可用性リスクとして `TODO.md` Medium へ昇格した。fixture は `/tmp/markdown-view-search-allocation-upper-limit.***` に生成し、repo へ追加していない
  - 追加計測: 2026-05-26 に `/tmp/markdown-view-search-byte-limit-plateau.***` の near-64m-many-files fixture (70 files / 111M) と overshoot-file fixture (43 files / 67M) で、dev/release、cold/warm、request 中 peak RSS、request 後 after RSS、5秒待機後 settled RSS を分けて測定した。環境は `rustc 1.93.1 (01f6ddf75 2026-02-11)`, `cargo 1.93.1 (083ac5135 2025-12-15)`, Linux WSL2 `5.15.133.1-microsoft-standard-WSL2`。loopback bind が必要なため server 起動と HTTP 測定は sandbox 外で実行した。HTTP response は `searched_files`、`searched_bytes`、`truncated=true`、`truncated_reasons=["byte_limit"]` を維持し、実パス、full process args、本文断片は記録していない
  - 追加計測値: 実装前 near-debug は `searched_files=40`, `searched_bytes=66000760`, elapsed 15.49-19.37s, peak RSS 174564→220724→241564→232640→258996 KiB, after RSS 137688→199380→198980→215732→228252 KiB, settled RSS 171272→219532→202148→214660→254952 KiB。実装前 overshoot-debug は `searched_files=42`, `searched_bytes=63001008`, elapsed 14.45-17.17s, peak RSS 172648→218424→231324→233752→235612 KiB, after RSS 151208→184540→215996→209724→222144 KiB, settled RSS 206232→204300→208864→221272→214904 KiB。実装前 near-release は elapsed 3.52-3.79s, peak RSS 199420→238648→238852→273248→258872 KiB, after RSS 181968→214004→198036→244596→244684 KiB, settled RSS 196652→232880→236620→246316→224384 KiB。実装前 overshoot-release は elapsed 3.34-3.52s, peak RSS 175664→264732→298568→304836→293792 KiB, after RSS 169304→232388→298568→302092→292232 KiB, settled RSS 236500→254512→268748→261748→292076 KiB
  - 対応: 測定で byte-limit 超過候補ファイルの本文読込が peak RSS に寄与し得ることを確認したため、`src/server/files/search.rs` で base directory capability から検索対象を開き、open 済み handle の metadata size と残り byte 予算を本文 `String` 構築前に比較する構成にした。本文読込後の既存 `markdown.len()` check は残し、capability 外 symlink 差し替え、TOCTOU、特殊ファイルシステム差異に備えている。追加レビューで base directory 自体のパス実体差し替えが残ることを確認したため、`CanonicalPath` に生成時の filesystem identity を保持し、検索開始時に open した base directory handle の identity と一致しない場合は `PermissionDenied` で拒否する構成にした。検索対象の列挙と本文読込は同じ検証済み base directory capability を起点にし、identity 取得不能時も fail closed とする。byte-limit 超過候補は、不正 UTF-8 かどうかを判定するための本文 I/O も行わず `byte_limit` で打ち切る契約として test で固定した
  - 再計測: 実装後 overshoot-debug は `searched_files=42`, `searched_bytes=63001008`, elapsed 14.82-16.93s, peak RSS 166500→214856→231612→234364→235084 KiB, after RSS 138764→190656→231612→234364→229908 KiB, settled RSS 182184→183004→220444→232708→224004 KiB。実装後 overshoot-release は elapsed 3.28-5.00s, peak RSS 217644→240060→253124→248976→288792 KiB, after RSS 187412→214284→220912→245664→271148 KiB, settled RSS 206368→228852→248976→213804→267668 KiB。HTTP response は `truncated=true`, `truncated_reasons=["byte_limit"]`, `searched_bytes=63001008` のままで、超過候補ファイルを検索済み byte に含めないことを確認した
  - 検証: `cargo test server::files::search::tests::test_search_directory_総読込バイト上限到達を明示し超過ファイルは検索しない -- --nocapture`、`cargo test server::files::search::tests::test_search_directory_byte予算超過候補は本文string構築前に打ち切る -- --nocapture`、`cargo test search --all-targets --all-features`、`./verify.sh` が通過した。`./verify.sh` は初回 `node_modules` 不在で停止したため、`npm ci` で Node 依存を準備して再実行した
  - 残件: byte-limit 超過候補の不要な本文読込は解消したが、実装後も settled RSS は完全な低 plateau と断定できない。allocator 保持、Markdown parser、JSON 直列化、watcher 混雑 WARN の測定ノイズを分ける必要がある場合は、別タスクで allocator/parser/serialization の追加切り分けを行う。検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は弱めていない
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
