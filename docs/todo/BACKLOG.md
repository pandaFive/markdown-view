# Backlog (Low Priority)

低優先度で蓄積している項目。High/Medium は [`TODO.md`](./TODO.md) に置き、ここには低優先・長期改善の未完了候補を置く。
未完了項目は重要度と将来影響度を基準に P1/P2/P3 へ分類する。各項目末尾の「由来」は TODO.md 再編時（2026-04-21）以降の発見コンテキスト。

最終整理: 2026-06-09。セキュリティ境界、データ安全性、silent failure、監視不能に直接響く項目は `TODO.md` へ昇格した。ここには昇格しないが文脈を残すべき候補を置く。
過去の完了済み履歴は [`DONE-2026-05.md`](../done/DONE-2026-05.md) に移動した。
レビュー由来の `現状` は作業候補として扱い、実装前に対象ファイル・行番号・現象を現行コードで再確認する。

## P1: リスク低減・契約明文化

## P2: 保守性・局所回帰検知

- [ ] ディレクトリ検索 prefix many-match RSS plateau の環境差・live allocation 追加診断を必要時に行う
  - ファイル: `src/server/files/search.rs`, `scripts/measure-search-rss-plateau.mjs`
  - 現状: 2026-06-04 と 2026-06-05 の測定で、単一ファイル prefix many-match 経路の response 完了後に anonymous memory が高く残ること、`MALLOC_ARENA_MAX=1` で settled anonymous RSS が下がること、multifile result-limit や short fallback は同規模の plateau を示さないことを確認した。これにより Medium Priority の「切り分ける」目的は完了扱いにした。
  - 追加診断: 2026-06-09 に `scripts/measure-search-rss-plateau.mjs --timeline --strict --modes release --fixtures prefix --runs cold --fixture-scale full --fixture-density dense --settled-delays 1s,5s --allocator-profiles default,arena1` を実行した。primary measurement は sandbox EPERM 後、承認付き再実行で pass。acceptanceStatus=`full`, fullAcceptanceMet=`true`。raw response body、実パス、full process args、raw `/proc/maps` 行、親環境変数値は記録していない。
  - 診断結果: default settled anonymous memory は settled_1s=204700 KiB, settled_5s=598624 KiB。arena1 は settled_1s=150428 KiB, settled_5s=551256 KiB。default_vs_arena1_settled_delta_kb は settled_1s=54272, settled_5s=47368。default requestPeakAnon=`request_started`, bodyDrainPeakAnon=`headers_received`, body_peak_to_body_received_delta_kb=0。arena1 requestPeakAnon=`request_started`, bodyDrainPeakAnon=`headers_received`, body_peak_to_body_received_delta_kb=0。`requestPeakAnon=request_started` は request 中の live allocation peak ではなく request 開始時 baseline が最大だったことを示すため、peak-to-settled は live allocation decay と断定しない。`body_peak_to_body_received_delta_kb=0` は外部 HTTP response body drain 窓の観測であり、server 内部の JSON 構築完了時点や allocation 解放時点と同一視しない。
  - 検証: `node scripts/measure-search-rss-plateau.mjs --self-test`: pass。`git diff --check`: pass。`./verify.sh`: pass。
  - 次判断: timeline summary は取得でき、allocator profile 間の settled anonymous memory 差は正方向に出た。一方で primary run は prefix dense の default/arena1 比較に限定しており、prefix dense/sparse や multifile 対照を同一 JSON report で追加していないため、Rust 側 allocation 削減や追加 probe へ進むかは、必要性が再浮上した時点で path specificity 追加測定を行って判断する。現時点では Done に移さず、低優先の将来候補として残す。
  - 対応: RSS の絶対値改善や環境差検証が必要になった場合に限り、同一 prefix fixture を native Linux、別 allocator build、または prefix 経路の live allocation 観測で再確認する。現時点では個人向け localhost ツールの High / Medium 実行候補には戻さず、低優先の将来候補として扱う。
  - セキュリティ: 測定は未信頼入力として扱う。追加診断を行う場合も、測定出力には実パス、full process args、本文断片、raw maps 行、親環境の値を含めない。検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は変更していない。
  - 由来: ディレクトリ検索 many-match RSS plateau 完了判定のレビュー修正 (2026-06-06)

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

## Done

- [x] WS Host middleware bypass 兆候のメトリクス化を必要性ベースで検討する
  - 完了根拠: `src/server/guards.rs` の Host 系 `WsOriginRejection` (`MissingHost`, `HostMalformed`, `UntrustedHost`) は、Host middleware 後段では通常到達しない bypass / malformed probe 兆候として `ERROR`、`ws_rejection_class="WS Host 検証異常"`、`host_recheck_anomaly=true` の構造化ログ契約で固定した。Origin 系拒否は `WS Origin 拒否`、`host_recheck_anomaly=false` として分離し、Host 系異常説明文を混ぜないことを unit test で確認した。WS 拒否ログの Host は監査ログ用 helper 経由で記録し、userinfo 付き authority や不正 authority は実値ではなく sentinel 化する。WS Origin も userinfo 付き authority と非数値 port authority は実値ではなく sentinel 化する。
  - 判断: 個人向け localhost ツールとしては、既存の `error!` ログと structured field で異常兆候を確認できるため、metrics crate、counter state、HTTP endpoint、外部監視基盤は追加しない。継続集計が必要な本格運用要求が出た場合のみ、今回固定した `host_recheck_anomaly=true` ログを入力契約として counter 化を再検討する。
  - セキュリティ: Host / Origin は攻撃者制御の未信頼入力として扱い、ログ値は監査ログ用の正規化 helper 経由に限定する。WS Host は parse 可能な authority のみ通常値を記録し、userinfo 付き authority や path / query を含む不正 authority は実値を出さず sentinel 化する。Origin は parse 可能な場合も scheme + authority までを記録し、path / query / fragment は出さない。userinfo 付き Origin authority と非数値 port authority も実値を出さず sentinel 化する。Host/Origin 検証、DNS Rebinding 対策、CSP、security headers、WebSocket payload は変更しない。query string、Markdown 本文、ファイルパス、full process args、環境変数は新規出力しない。
  - 由来: PR #123 レビュー follow-up (2026-05-04)、WS Host bypass structured log 契約設計 (2026-06-07)

- [x] インラインブラウザJS の TS 化
  - 完了根拠: `src/template/assets/ts/*.ts` を正ソースにし、`build.rs` が `MV_INLINE_JS_OUT_DIR` 付きの `npm run build:inline-js` 経由で Cargo `OUT_DIR` 配下へ生成した JS を `inline_script.rs` へ埋め込む構成にした。生成 `.js` はリポジトリに保持せず、`npm run typecheck` で E2E とインライン JS の両方を検査する。既存の結合順序、`__MAX_FILE_SIZE_MB__` sentinel 置換、CSP hash、`innerHTML` sink allowlist、E2E hook production 非公開契約は維持している。検証は `MV_INLINE_JS_OUT_DIR="$(mktemp -d)" npm run build:inline-js`、`npm run typecheck`、`cargo test --lib template::assets::inline_script`、`npx playwright test tests/e2e/update_content_exposure.spec.ts`、`npx playwright test tests/e2e/document_search.spec.ts`、`./verify.sh`、`./verify.sh --e2e` が通過した。

- [x] `AppMode` 構築時の `is_file()`/`is_dir()` 判定の TOCTOU を緩和する
  - 完了根拠: `src/server/state.rs` は `CanonicalPath::try_from_path()` で canonicalize した後、`ensure_canonical_file()` / `ensure_canonical_directory()` が `metadata_for_mode()` 経由で取得した `Metadata` の `file_type()` から file / directory を判定している。canonicalize 後に対象が消えた場合も `NotFile` / `NotDirectory` へ集約することを unit test で固定済み。`.md` 拡張子チェック、canonical path 保持、base_dir / single_file / directory の公開契約、Host/Origin 検証、HTML sanitize、CSP、path validation は変更していない

- [x] サイドバーの "Documents" fallback を日本語化
  - 完了根拠: `src/server/service.rs` の `sidebar_directory_name()` fallback を private 定数 `SIDEBAR_DIRECTORY_FALLBACK_NAME` 経由の `"ドキュメント"` に変更した。通常のディレクトリ名が取得できる場合は従来どおり実ディレクトリ名を使うことを unit test で固定した。i18n 基盤、UI 全体の文言、HTML sanitize、path validation は変更していない

- [x] `catalog.rs` の相対パス構築で中間 Vec allocation を避ける
  - 完了根拠: `src/server/files/catalog.rs` に `relative_path_to_slash_string()` を追加し、catalog.rs の相対パス構築で `components().map(...).collect::<Vec<_>>().join("/")` を使わずに `/` 区切り文字列を構築するようにした。ファイル列挙の sort、件数上限、除外ルール、canonicalize 再検証、symlink handling は変更していない。helper の単一 component とネスト path の出力を unit test で固定した

過去の完了済み履歴は [`DONE-2026-05.md`](../done/DONE-2026-05.md) へ移動した。
