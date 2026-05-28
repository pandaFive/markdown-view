# ディレクトリ検索 byte-limit RSS plateau 設計書

## 背景

`docs/todo/BACKLOG.md` の P2 には、ディレクトリ検索 64MiB byte-limit 反復時の RSS plateau を測定方法改善込みで再確認する項目が残っている。

前回の上限近傍計測では、64MiB 近傍 byte-limit fixture に対する HTTP `q=missingneedle` が `truncated_reasons=["byte_limit"]` を返すことは確認できた。一方で、server RSS は request 後 snapshot だけでは plateau と判断できず、request 中 peak RSS と request 完了後の settled RSS も分離できていない。

今回の目的は、測定方法を改善して RSS の peak / after / settled を分け、必要な場合だけ byte-limit 超過候補ファイルの本文読込を避ける最小実装へ進めることである。

## ゴール

- 64MiB byte-limit 近傍検索の request 中 peak RSS、request 完了直後の after RSS、短い待機後の settled RSS を分けて記録する。
- dev / release、cold / warm の差を分けて記録する。
- `SearchResponse` の `searched_files`、`searched_bytes`、`truncated`、`truncated_reasons` を確認し、検索 API 契約が維持されていることを記録する。
- 測定だけで plateau を許容できる場合は、コード変更せず根拠を `BACKLOG.md` に残す。
- plateau しない、または byte-limit 超過候補ファイル読込の peak RSS が明確に大きい場合は、本文読込前 byte-budget 判定を最小実装として行う。

## 非ゴール

- 検索 API の JSON shape 変更。
- `SearchResponse` への `budgeted_bytes` / `consumed_bytes` 追加。
- 検索結果の意味変更。
- 検索インデックス導入。
- UI 変更。
- メトリクス基盤の追加。ファイル open 境界を capability-based API に寄せるための最小依存追加は許容する。
- `pulldown-cmark` parser の全面逐次化。
- Host/Origin 検証、HTML sanitization、CSP の変更。
- path validation の public contract 変更。内部実装を base identity 検証と capability-based access に寄せることは許容するが、base 外、hidden、非 Markdown、symlink 差し替え拒否は弱めない。

## 測定設計

測定 fixture は `/tmp/markdown-view-search-byte-limit-plateau.***` 配下に生成し、リポジトリには追加しない。fixture 内容は固定文字列で生成し、外部文書、Issue、検索結果、LLM 出力をそのまま使わない。

### Fixture

1. `near-64m-many-files`
   - 1.5MiB 前後の Markdown を 70 件程度生成する。
   - 64MiB 総量に近づいた後、次ファイルで byte-limit に到達する経路を作る。
   - no-match query で full scan に近い処理を走らせる。

2. `overshoot-file`
   - 検索済み bytes が 63MiB 前後になった後、数 MiB の次ファイルで byte-limit を超える構成にする。
   - 本文読込前判定を入れた場合に、読込回避の効果を比較しやすくする。

### RSS 記録

測定は単発の `/usr/bin/time -v curl` だけに頼らない。対象 server PID を確認し、HTTP request 中に短周期で RSS を sampling する。

記録する値は次に限定する。

- fixture 種別と規模。
- dev / release、cold / warm。
- HTTP status。
- `searched_files`、`searched_bytes`、`truncated`、`truncated_reasons`。
- elapsed。
- request 中 peak RSS。
- request 完了直後の after RSS。
- 短い待機後の settled RSS。

docs にはローカルユーザー名、絶対パス、本文断片、full process args を残さない。

### Plateau 判定

plateau 判定は、同一 server process に同一条件の request を複数回流し、settled RSS が増え続けるかで判断する。

1回目だけ大きく増え、その後は一定範囲に収束する場合は、allocator が確保領域を保持している可能性を考慮し、peak RSS と settled RSS を分けて評価する。反復ごとに settled RSS が上がり続ける場合は plateau 不十分として扱う。

## 実装候補

測定で必要と判断した場合の実装は、`src/server/files/search.rs` の `search_directory_with_limits_blocking()` で、base directory capability から検索対象を開き、open 済み handle の metadata size と残り byte 予算を本文 `String` 構築前に比較することである。

変更前の課題は次の通りである。

1. 対象ファイルの相対パス契約を検証する。
2. 検証済みの ambient path を `read_markdown_with_limit_blocking()` 相当で再度 open する。
3. `read_markdown_with_limit_blocking()` 相当の本文読込で本文を `String` に読み切る。
4. `stats.searched_bytes + markdown.len() > limits.max_bytes` の場合、`byte_limit` で終了する。

改善候補では、ambient path を再度 open せず、base directory capability から対象を開く。対象ファイルが `MAX_FILE_SIZE` を超える場合、open / metadata に失敗する場合は従来どおり skip する。`stats.searched_bytes + metadata.len()` が `limits.max_bytes` を超える場合は、不正 UTF-8 かどうかを判定するための本文 I/O も行わず、本文 `String` を構築せずに `SearchTruncationReason::Byte` を付けて終了する。

本文読込後の既存 `markdown.len()` ベース確認は残す。metadata と本文読込の間の TOCTOU、UTF-8 decode 後の実 byte 長、特殊ファイルシステム差異に備えるためである。

この実装では、byte-limit に含めるのは実際に検索した bytes という既存契約を維持する。超過して読まなかったファイルは `searched_files` と `searched_bytes` に加えず、`truncated=true`、`truncated_reasons=["byte_limit"]` を返す。内部で metadata byte 予算を管理しても public JSON には出さず、`searched_bytes` は本文を読み検索処理へ進めた UTF-8 Markdown bytes のみを表す。

検索対象列挙の安全性を保つため、catalog / search / resolve のファイル open は base directory capability から nofollow で行う。canonicalize 後に symlink や base directory が差し替えられても、除外 path や base 外 path は列挙・読込しない。Markdown 以外の大量 entry による走査 DoS は catalog の entry / directory 予算で部分結果にし、検索 API では既存の `file_limit` truncation reason へ写像する。

## テスト方針

コード変更する場合は TDD で進める。

- `SearchLimits` を小さくした unit test で、次ファイルが byte-limit を超える場合に本文読込へ進まないことを固定する。
- 読込回避の検証は既存 test hook を使うか、必要最小限の test-only hook を追加する。
- `searched_bytes` は読んだファイル分だけで、超過候補ファイル分を含まないことを確認する。
- `truncated_reasons` が `byte_limit` になることを確認する。
- 既存の result-limit、file-limit、stale cancellation、large block search の契約を壊さない。

性能値や RSS の絶対閾値は CI テストに入れない。RSS は環境依存が大きいため、測定結果として docs に記録する。

## 受け入れ基準

- 測定結果として、dev / release、cold / warm、peak / after / settled RSS、HTTP JSON 契約を記録する。
- 測定だけで plateau が許容できる場合は、コード変更せず、その根拠と残余リスクを `BACKLOG.md` に残す。
- plateau しない、または byte-limit 超過候補ファイル読込の peak が明確に大きい場合は、本文読込前 byte-budget 判定を実装する。
- 実装する場合は、「byte-limit 超過候補を本文 `String` 化しない」「byte-limit 超過候補は不正 UTF-8 でも skip より `byte_limit` 到達を優先する」「`searched_bytes` は検索したファイル分だけ」「`truncated_reasons` は `byte_limit`」をテストで固定する。
- `./verify.sh` を通す。
- 必要に応じて検索 targeted test と release / dev の手動測定結果を記録する。

## 影響範囲

- 測定のみの場合
  - `docs/todo/BACKLOG.md`
  - `docs/superpowers/plans/` の後続 implementation plan
  - `/tmp/markdown-view-search-byte-limit-plateau.***`

- 実装する場合
  - `Cargo.toml`
  - `Cargo.lock`
  - `src/server/state.rs`
  - `src/server/files/catalog.rs`
  - `src/server/files/content.rs`
  - `src/server/files/mod.rs`
  - `src/server/files/resolve.rs`
  - `src/server/files/search.rs`
  - `src/server/service.rs`
  - `src/server/log_path.rs`
  - 検索、ファイル一覧、ファイル解決関連 unit test
  - `/api/search` integration test
  - 必要なら `docs/todo/BACKLOG.md`

`SearchResponse` JSON、HTTP route、UI、CSP、Host/Origin 検証、path validation の public contract、HTML sanitization、ブラウザ assets は変更しない。path validation の内部実装は base identity 検証と capability-based access へ寄せる。

## エラー処理

測定中に HTTP 400、403、429、500、curl 失敗、JSON field 欠落、対象 server PID の判別不能が起きた場合は、検索実装を変更する前に計測条件を確認する。

- Host header が `127.0.0.1:<port>` になっているか。
- port が既存 server と衝突していないか。
- fixture path が今回生成した `/tmp` ディレクトリを指しているか。
- query が no-match / byte-limit 経路に対応しているか。
- 対象 PID が今回起動した server process であるか。

原因を切り分けられない場合は、測定不十分として `BACKLOG.md` に残し、実装判断へ進まない。

## セキュリティ

検索クエリ、Host/Origin、path、fixture 本文は未信頼入力として扱う。fixture は固定文字列で生成し、取得済みテキストや LLM 出力を shell、SQL、policy、コードとして実行しない。

今回の実装候補は検索対象ファイルの読み込み量を減らす方向であり、既存の path validation、localhost binding、Host/Origin 検証、CSP、HTML sanitization を弱めない。本文読込は base directory capability から開いた handle に寄せ、metadata と本文読込の対象差し替え、および capability 外 symlink への差し替えに備える。本文読込後の既存 byte-limit 再確認も残し、特殊ファイルシステム差異に備える。

ログや docs には、ローカルユーザー名、絶対パス、本文断片、full `ps args` を残さない。必要な情報は fixture 規模、検索統計、elapsed、RSS 系列に限定する。

## ロールバック

測定のみの場合、docs 更新 commit を revert すれば元に戻せる。fixture は `/tmp` 配下の対象ディレクトリを確認して削除する。

実装した場合は、`search_directory_with_limits_blocking()` の本文読込前 metadata 判定、catalog の走査予算、resolve/content の解決済み handle 読込、nofollow open helper、関連テストを同じ commit 単位で revert すれば、従来の読込後 byte-limit 判定へ戻せる。rollback 後は symlink race と大量非 Markdown entry の防御も戻るため、再適用する場合は個別に切り出す。

## 残余リスク

- RSS と elapsed は OS、CPU、ディスクキャッシュ、ビルド種別、同時実行中の別 process の影響を受ける。
- allocator がメモリを OS に返さないだけの場合、settled RSS は高止まりしても leak とは限らない。
- metadata による事前判定は通常ファイルを前提にした近似であり、最終契約は本文読込後の既存チェックで守る。
- byte-limit 読込回避で peak RSS が下がっても、Markdown parser や JSON 直列化由来の allocation は別問題として残る。
- 将来検索上限や UI 要件を変える場合は再測定が必要になる。

## 見積もり

- 人間作業: 2-4 時間。
- Codex/AI 支援: 45-90 分。

測定 fixture 生成、server 起動、RSS sampling、dev / release 比較、必要時の最小実装と検証が主な変動要因である。

## 検証

設計書自体は docs-only なので、TDD ではなく文書検証で確認する。

```bash
rg -n "背景|ゴール|非ゴール|測定設計|実装候補|テスト方針|受け入れ基準|セキュリティ|ロールバック|見積もり" docs/superpowers/specs/2026-05-26-directory-search-byte-limit-plateau-design.md
placeholder_matches="$(rg -n -P 'T[B]D|TO[D]O(?!\.md| Issues)|未[定]' docs/superpowers/specs/2026-05-26-directory-search-byte-limit-plateau-design.md | rg -v 'T\[B\]D|TO\[D\]O|未\[定\]' || :)"
test -z "$placeholder_matches" || { printf '%s\n' "$placeholder_matches"; exit 1; }
git diff --check
```
