# 検索 RSS allocator profile 切り分け設計書

## 目的

`docs/todo/TODO.md` の「ディレクトリ検索 many-match の RSS plateau」を完了判断できる水準まで追加切り分けする。副目的として、次の実装タスクを決めるための比較材料も集める。ただし測定が重くなる場合は、原因分類を優先する。

現時点の測定では、単一ファイル prefix many-match 経路で response 完了後に `RssAnon` と `smaps_rollup Anonymous` が高く残ることが分かっている。一方で、glibc allocator arena の保持、WSL2 の RSS/accounting 特性、または検索経路の live allocation のどれが支配的かはまだ確定していない。

この設計では、production Rust code を変更せず、既存の `scripts/measure-search-rss-plateau.mjs` を allocator profile 比較に対応させる。

## 非ゴール

- production Rust code の変更。
- `/api/search` の JSON shape や検索結果の意味変更。
- UI、TypeScript、generated JS の変更。
- 恒久 telemetry の追加。
- Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約の変更。
- native Linux 環境をこの作業内で必須準備すること。

native Linux での再測定は、実行環境が用意できる場合の任意追加手順として扱う。

## 測定アーキテクチャ

`scripts/measure-search-rss-plateau.mjs` に allocator profile の測定軸を追加する。CLI には次を追加する。

```bash
node scripts/measure-search-rss-plateau.mjs \
  --modes release \
  --fixtures prefix \
  --runs cold,warm \
  --fixture-scale full \
  --allocator-profiles default,arena1,arena2
```

想定 profile は次の通り。

- `default`: 環境変数を変えない現状基準。
- `arena1`: server process 起動時に `MALLOC_ARENA_MAX=1` を追加する。
- `arena2`: 必要時に `MALLOC_ARENA_MAX=2` を追加する。

`--smoke` は従来どおり短い dev/prefix/cold/default のみとし、重い matrix を暗黙に走らせない。`--allocator-profiles` を `--smoke` と併用した場合は、既存 matrix option と同じくエラーにする。

測定出力には、scenario ごとに次を記録する。

- allocator profile 名。
- 許可された allocator 環境変数名と値。
- build mode、fixture kind、run kind、fixture scale。
- HTTP status と検索 API 契約値。
- elapsed。
- request 中 peak RSS。
- response 完了直後の after RSS。
- 5 秒後 settled RSS。
- `RssAnon`。
- `smaps_rollup Anonymous`。
- file-backed RSS の目安。
- `/proc` 取得 completeness と scenario error。

環境変数は profile 定義の allowlist だけを report する。親 process の環境全体は出力しない。

## データフロー

1. fixture を `/tmp/markdown-view-search-rss-plateau.***` 配下に生成する。
2. 対象 binary を dev/release から選ぶ。
3. allocator profile ごとに server process を新規起動する。
4. HTTP `/api/search?q=needle` を送る。
5. request 中に `/proc/<pid>/status`、`smaps_rollup`、必要最小限の maps 集計を sampling する。
6. response 完了直後と 5 秒後の snapshot を取る。
7. sanitized JSON にまとめる。
8. `docs/todo/TODO.md` に判断、残件、セキュリティ境界維持を追記する。

cold run は profile ごとに server process を立て直す。warm run は同じ server process に複数回 request を送る。これにより allocator profile 差と process 初期化差を混ぜにくくする。

## 判定方針

- `arena1` で settled `RssAnon` が大きく下がる場合、glibc arena retained memory を強い主因候補とする。
- `arena1` でも下がらず、native Linux で下がる場合、WSL2 RSS/accounting 特性を強い主因候補とする。
- allocator profile で下がらず、peak と settled が近い場合、live allocation または parser/response 経路の保持を疑う。
- multifile が低いままで prefix 単一だけ高い場合、単一ファイル prefix 経路に限定した問題として扱う。
- HTTP 契約が壊れた場合、RSS 判定より検索契約不一致を優先して失敗扱いにする。

絶対 RSS のしきい値は CI test に入れない。RSS は OS、allocator、CPU、ディスクキャッシュ、同時実行 process の影響を受けるため、測定結果として docs に記録する。

## エラー処理

scenario ごとに失敗を記録し、他 scenario は可能な限り継続する。

記録対象の失敗は次を含む。

- port 衝突。
- server 起動失敗。
- HTTP timeout。
- JSON 契約不一致。
- `/proc` 欠落。
- `smaps_rollup` 権限不足。
- fixture 生成失敗。

sandbox の loopback bind 制限で server 起動が失敗する場合は、承認付き実行が必要になる。承認されない場合は、self-test と文書検証までを完了範囲とし、RSS 実測は残リスクとして報告する。

## テストと検証

docs/config/script 変更なので、ceremonial TDD ではなく validation を使う。ただしスクリプトの契約は self-test で固定する。

実装後に確認する項目は次の通り。

```bash
node scripts/measure-search-rss-plateau.mjs --self-test-sanitization
node scripts/measure-search-rss-plateau.mjs --help
node scripts/measure-search-rss-plateau.mjs --smoke
./verify.sh
```

`--smoke` が sandbox の loopback bind で失敗した場合は、承認付きで再実行する。承認されない場合は、未実行または失敗理由を completion report に残す。

self-test では次を固定する。

- `--allocator-profiles` の parse。
- 不正 profile の拒否。
- `--smoke` と allocator profile matrix option の併用拒否。
- profile ごとの env allowlist。
- report sanitization が実パス、ユーザー名、本文断片、raw maps 行を出さないこと。

## セキュリティ

検索 query、fixture 本文、HTTP response、`/proc` 出力、外部環境変数は未信頼入力として扱う。fixture は固定文字列で生成し、取得済みテキスト、検索結果、issue、LLM 出力を shell、SQL、policy、コードとして実行しない。

出力 sanitization は維持し、docs と JSON report に次を残さない。

- 実パス。
- ローカルユーザー名。
- full process args。
- Markdown 本文断片。
- raw `/proc/maps` 行。
- 親 process の環境変数一覧。

server process に追加する環境変数は、profile 定義で許可された `MALLOC_ARENA_MAX` のみとする。検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は弱めない。

## 影響範囲

主な変更対象は次の 2 ファイル。

- `scripts/measure-search-rss-plateau.mjs`: allocator profile CLI と実行 matrix。
- `docs/todo/TODO.md`: 追加測定結果、判断、残件。

設計書としてこのファイルを追加する。

確認対象は次の通り。

- `src/server/files/search.rs`: production code を変更しないことを確認する。
- `docs/superpowers/plans/2026-06-04-search-rss-plateau.md`: 既存測定計画との整合確認。

## ロールバック

スクリプト拡張、`TODO.md` 追記、この設計書 commit を revert すれば production behavior は元に戻る。fixture は `/tmp/markdown-view-search-rss-plateau.***` 配下に生成されるため、削除してもリポジトリには影響しない。

## 受け入れ基準

- `scripts/measure-search-rss-plateau.mjs` が allocator profile matrix を実行できる。
- self-test が allocator profile parsing と sanitization を固定している。
- 少なくとも release/prefix/full/cold、または短縮 smoke の allocator 比較結果を得る。
- `docs/todo/TODO.md` に、どの profile で RSS が下がったか、または下がらなかったか、主因候補、次にやるべきことが追記される。
- 検索 API 契約、Host/Origin/path validation、HTML sanitize、CSP、検索上限契約を弱めていないことが記録される。
- `./verify.sh` が pass する。未実行または失敗がある場合は、理由と残リスクを報告する。

## 見積もり

- 人間作業: 1.5-3 時間。
- Codex/AI 支援: 40-90 分。

loopback bind 承認、release build、full fixture 測定、native Linux 追加測定の有無で変動する。

## 文書検証

設計書自体は docs-only なので、次で検証する。

```bash
rg -n "目的|非ゴール|測定アーキテクチャ|データフロー|判定方針|エラー処理|テストと検証|セキュリティ|影響範囲|ロールバック|受け入れ基準|見積もり" docs/superpowers/specs/2026-06-05-search-rss-allocator-profile-design.md
placeholder_matches="$(rg -n -P 'T[B]D|TO[D]O(?!\\.md| Issues)|未[定]' docs/superpowers/specs/2026-06-05-search-rss-allocator-profile-design.md | rg -v 'T\\[B\\]D|TO\\[D\\]O|未\\[定\\]' || :)"
test -z "$placeholder_matches" || { printf '%s\n' "$placeholder_matches"; exit 1; }
git diff --check
```
