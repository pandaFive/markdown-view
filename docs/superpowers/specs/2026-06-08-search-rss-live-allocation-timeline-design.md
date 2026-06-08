# 検索 RSS live allocation timeline 設計書

## 目的

`docs/todo/BACKLOG.md` の「ディレクトリ検索 prefix many-match RSS plateau の環境差・live allocation 追加診断」を、Rust production code を変更せずに追加分類する。

今回の主目的は、prefix many-match 検索中の RSS / anonymous memory の時系列を外部から観測し、次のどれに近いかを判断できる材料を増やすことである。

- request 中だけ大きく増える一時 live allocation。
- response 完了後も残る allocator retained memory。
- JSON 直列化または response buffering の寄与。
- prefix 経路固有の挙動。

この作業は RSS plateau の解消ではなく、次に Rust 側 probe や allocation 削減実装へ進む価値があるかを判断するための診断である。

## 非ゴール

- production Rust code の変更。
- Rust 側 instrumentation、`#[cfg(test)]` probe、診断ログの追加。
- 検索アルゴリズム、allocator、Tokio runtime 構成の改善実装。
- `MALLOC_ARENA_MAX=1` の常用化判断。
- native Linux や別 allocator build での環境差検証を必須にすること。
- `/api/search` の JSON shape や検索結果の意味変更。
- UI、TypeScript、generated JS の変更。
- Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約の変更。

## 測定アーキテクチャ

`scripts/measure-search-rss-plateau.mjs` に opt-in の timeline 測定モードを追加する。CLI は次を想定する。

```bash
node scripts/measure-search-rss-plateau.mjs \
  --timeline \
  --modes release \
  --fixtures prefix \
  --runs cold \
  --fixture-scale full \
  --allocator-profiles default,arena1
```

`--timeline` は重い診断モードとして扱い、`--smoke` とは併用不可にする。既存 `--smoke` は軽量な dev/prefix/cold/default の契約確認に限定し、timeline sampling を暗黙に走らせない。

timeline 測定では、scenario ごとに次の snapshot を記録する。

- `server_ready`: server 起動後、request 前。
- `request_started`: HTTP request 送信直後。
- `request_peak`: request 中 sampling で観測した最大 RSS snapshot。
- `headers_received`: HTTP response headers 受信時。
- `body_received`: response body 受信完了直後。
- `settled_1s`: body 受信完了から 1 秒後。
- `settled_5s`: body 受信完了から 5 秒後。
- `settled_15s`: 指定時のみ body 受信完了から 15 秒後。

各 snapshot には、取得できる範囲で次を含める。

- `VmRSS`、`RssAnon`、`RssFile`、`RssShmem`。
- `smaps_rollup Anonymous`。
- elapsed milliseconds。
- `/proc` 取得 completeness。
- scenario error。

HTTP response については、既存と同じ契約値を記録する。

- status。
- `searched_files`。
- `searched_bytes`。
- `truncated`。
- `truncated_reasons`。
- result count。

## 比較 fixture

比較対象は最小限に抑える。

- `prefix-full`: 既存の full prefix many-match fixture。
- `prefix-sparse`: full prefix と近い byte サイズで hit 密度を下げた fixture。
- `multifile` または short fallback: prefix 経路固有性を確認する対照。

`prefix-sparse` は、同程度の読み込み量でも hit 密度と result-limit 到達位置が異なる場合に peak / settled の差が変わるかを見るための補助 fixture である。測定 matrix が過剰にならないよう、既定 timeline run では `prefix-full` を主対象にし、`prefix-sparse` と対照 fixture は明示指定時だけ実行する。

allocator profile は `default` と `arena1` を基本にする。`arena2` は必要時の任意 profile とし、timeline の受け入れ基準には含めない。

## 判定方針

timeline の判定は絶対 RSS ではなく、scenario 内と profile 間の相対差で行う。

- `request_peak` が高く、`body_received` または `settled_1s` 以降で大きく下がる場合は、一時 live allocation の寄与が強い候補とする。
- `request_peak` と `settled_5s` が近い場合は、allocator retained memory または保持された構造の寄与が強い候補とする。
- `headers_received` から `body_received` までの差が大きい場合は、JSON 直列化または response buffering の寄与候補とする。
- `default` と `arena1` の `settled_5s` 差が大きい場合は、glibc allocator retained memory の寄与候補とする。
- `prefix-full` だけが高く、`prefix-sparse` や multifile 対照が低い場合は、prefix many-match 経路固有の問題として扱う。
- `/proc` 取得が不完全な scenario は判定から除外し、部分測定として記録する。

RSS は OS、allocator、CPU、ディスクキャッシュ、同時実行 process の影響を受けるため、CI test に絶対しきい値は入れない。

## エラー処理

scenario ごとに失敗を記録し、他 scenario は可能な限り継続する。記録対象の失敗は次を含める。

- port 衝突。
- server 起動失敗。
- HTTP timeout。
- response body parse failure。
- JSON 契約不一致。
- `/proc` 欠落。
- `smaps_rollup` 権限不足。
- fixture 生成失敗。

sandbox の loopback bind 制限で server 起動が失敗する場合は、承認付き実行が必要になる。承認されない場合は、self-test と文書検証までを完了範囲とし、timeline 実測は残リスクとして報告する。

## データ更新

実装時の主な変更対象は次の 2 ファイルである。

- `scripts/measure-search-rss-plateau.mjs`: `--timeline`、snapshot 名、sampling 間隔、timeline report、self-test。
- `docs/todo/BACKLOG.md`: 測定結果、判断、残件、セキュリティ境界維持。

設計書としてこのファイルを追加する。

production Rust code、UI、TypeScript、generated JS は変更しない。

## テストと検証

docs/config/script 変更なので、ceremonial TDD ではなく validation を使う。ただしスクリプトの契約は self-test で固定する。

実装後に確認する項目は次の通り。

```bash
node scripts/measure-search-rss-plateau.mjs --self-test-sanitization
node scripts/measure-search-rss-plateau.mjs --help
node scripts/measure-search-rss-plateau.mjs --timeline --modes release --fixtures prefix --runs cold --fixture-scale full --allocator-profiles default,arena1
git diff --check
./verify.sh
```

`--timeline` が sandbox の loopback bind で失敗した場合は、承認付きで再実行する。承認されない場合は、未実行または失敗理由を completion report に残す。

self-test では次を固定する。

- `--timeline` の parse。
- `--timeline` と `--smoke` の併用拒否。
- timeline snapshot 名の安定性。
- timeline report sanitization が実パス、ユーザー名、本文断片、raw maps 行を出さないこと。
- `prefix-sparse` fixture 指定が既存 fixture matrix と矛盾しないこと。

## セキュリティ

検索 query、fixture 本文、HTTP response、`/proc` 出力、外部環境変数は未信頼入力として扱う。取得済みテキスト、検索結果、issue、LLM 出力を shell、SQL、policy、コードとして実行しない。

出力 sanitization は維持し、docs と JSON report に次を残さない。

- 実パス。
- ローカルユーザー名。
- full process args。
- Markdown 本文断片。
- raw `/proc/maps` 行。
- 親 process の環境変数一覧または値。

server process に追加する環境変数は、allocator profile 定義で許可された値に限定する。検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は弱めない。

## 影響範囲

直接影響するのは診断スクリプトとバックログ文書である。通常の server 起動、検索 API、WebSocket、renderer、watcher、browser UI には影響しない。

確認対象は次の通り。

- `scripts/measure-search-rss-plateau.mjs`
- `docs/todo/BACKLOG.md`
- `src/server/files/search.rs`

`src/server/files/search.rs` は production code を変更しないことの確認対象であり、編集対象ではない。

## ロールバック

スクリプト拡張、`BACKLOG.md` 追記、この設計書 commit を revert すればよい。production Rust code は変更しないため、HTTP API、WebSocket、検索アルゴリズム、UI の追加 rollback は不要である。fixture は `/tmp/markdown-view-search-rss-plateau.***` 配下に生成されるため、削除してもリポジトリには影響しない。

## 受け入れ基準

- `scripts/measure-search-rss-plateau.mjs` が opt-in の timeline 測定を実行できる。
- timeline report に request 前、request 中 peak、headers 受信時、body 受信完了後、settled snapshot が含まれる。
- release/prefix/full/cold で `default` と `arena1` の timeline 比較結果を得る。
- `docs/todo/BACKLOG.md` に、live allocation 候補、allocator retained memory 候補、response buffering 候補、次に必要な作業が記録される。
- 検索 API 契約、Host/Origin/path validation、HTML sanitize、CSP、検索上限契約を弱めていないことが記録される。
- self-test と文書 validation が成功する。
- `./verify.sh` が pass する。未実行または失敗がある場合は、理由と残リスクを報告する。

## 見積もり

- 人間作業: 1.5-3 時間。
- Codex / AI 支援: 45-90 分。

loopback bind 承認、release build、full fixture timeline 測定、15 秒 settled snapshot の有無で変動する。

## 文書検証

設計書自体は docs-only なので、次で検証する。

```bash
rg -n "目的|非ゴール|測定アーキテクチャ|比較 fixture|判定方針|エラー処理|データ更新|テストと検証|セキュリティ|影響範囲|ロールバック|受け入れ基準|見積もり" docs/superpowers/specs/2026-06-08-search-rss-live-allocation-timeline-design.md
placeholder_matches="$(rg -n -P 'T[B]D|TO[D]O(?!\\.md| Issues)|未[定]' docs/superpowers/specs/2026-06-08-search-rss-live-allocation-timeline-design.md | rg -v 'T\\[B\\]D|TO\\[D\\]O|未\\[定\\]' || :)"
test -z "$placeholder_matches" || { printf '%s\n' "$placeholder_matches"; exit 1; }
git diff --check
```
