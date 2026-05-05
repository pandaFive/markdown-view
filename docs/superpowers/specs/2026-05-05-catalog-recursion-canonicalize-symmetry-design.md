# catalog 再帰判定 canonicalize 対称化設計

## 目的

`src/server/files/catalog.rs` の Markdown ファイル一覧列挙で、通常ディレクトリとシンボリックリンクディレクトリの再帰可否判定を対称化する。

現状は両分岐とも `canonicalize_dir_for_cycle` を通すが、シンボリックリンク分岐だけが追加で `resolved.is_dir()` を呼び、通常ディレクトリ分岐と判断材料が分かれている。今回の目的は、正規化、base 配下確認、ディレクトリ判定、visited 登録を一箇所に集め、`canonicalize` 失敗時に visited を経由せず再帰しない契約を明確にすることである。

## 非目的

- ファイル一覧 UI、検索 UI、API レスポンス形式は変更しない。
- `MAX_FILE_LIST`、`MAX_DIR_DEPTH`、隠しファイル除外ルールは変更しない。
- base 外シンボリックリンクを許可しない。
- Markdown 以外のファイルを列挙対象にしない。
- ディレクトリ走査全体の非同期化や performance tuning は行わない。
- `catalog.rs` の相対パス構築アロケーション削減は別 backlog として扱う。

## 設計

`list_markdown_files_recursive` のディレクトリ再帰判定を、小さな private helper へ切り出す。

候補名は `resolve_recursable_directory` とする。この helper は、対象 `path`、`file_type`、`canonical_base_dir`、`visited_dirs`、ログ用 base を受け取り、再帰してよい場合だけ `Some(canonical_path)` を返す。再帰不可の場合は既存方針どおり warn/debug ログを出して `None` を返す。

helper 内の判定順は次の通りにする。

1. 対象パスを `canonicalize_dir_for_cycle` で正規化する。
2. `canonicalize` に失敗した場合は安全側でスキップする。
3. シンボリックリンクの場合は、正規化先が `canonical_base_dir` 配下か確認する。
4. 正規化先の metadata からディレクトリかどうかを判断し、通常ファイルならスキップする。
5. `visited_dirs` に正規化パスを登録し、既訪問ならサイクルとしてスキップする。
6. 再帰可能な場合だけ呼び出し元へ返す。

呼び出し元は `file_type.is_dir() || file_type.is_symlink()` の大枠を維持し、helper が `Some` を返した場合だけ `list_markdown_files_recursive` を呼ぶ。再帰先の `current_dir` は既存の相対パス表示を維持するため `entry.path()` のままとし、サイクル検出と base 判定には正規化パスを使う。

通常ディレクトリにも metadata 確認を通すことで、通常ディレクトリとシンボリックリンクの再帰条件を同じ関数で読めるようにする。ただし `read_dir` 直前の TOCTOU を完全に消すものではないため、`read_dir` エラー時の既存スキップ挙動は維持する。

## 受け入れ条件

- 通常ディレクトリとシンボリックリンクディレクトリの再帰可否が同じ helper で判断されている。
- `canonicalize` 失敗時は、通常ディレクトリとシンボリックリンクのどちらでも再帰しない。
- シンボリックリンクが base 外を指す場合は引き続き除外される。
- シンボリックリンクが通常ファイルを指す場合は引き続き除外される。
- シンボリックリンクサイクルと自己参照シンボリックリンクでハングしない。
- 既存の Markdown ファイル一覧、ソート、最大件数、隠しファイル除外の挙動が変わらない。

## テスト方針

TDD で進める。最初に `src/server/files/tests.rs` の catalog 周辺テストへ、helper 契約を表すテストを追加または既存テスト名を調整する。

主な確認対象は次の通り。

- `canonicalize` 失敗時はスキップ扱いになる。
- base 外シンボリックリンクディレクトリは除外される。
- base 内シンボリックリンクディレクトリは辿られる。
- シンボリックリンクが通常ファイルを指す場合は除外される。
- シンボリックリンクサイクルと自己参照シンボリックリンクでハングしない。

実装中は catalog 関連の局所テストを優先して実行し、完了前に `./verify.sh` を実行する。

## セキュリティ考慮

ファイル一覧はユーザー指定ディレクトリ配下のパスを扱うため、シンボリックリンクと TOCTOU を未信頼入力として扱う。

base 外シンボリックリンク拒否、隠しパス除外、最大深度、最大件数、サイクル検出は維持する。正規化できないパスは安全側でスキップし、正規化結果が base 配下であることを確認してから再帰する。今回の変更で raw HTML、CSP、Host/Origin 検証、memo 保存境界には触れない。

## 影響範囲

- 主変更対象: `src/server/files/catalog.rs`
- テスト対象: `src/server/files/tests.rs`
- 間接影響: `src/server/files/search.rs`, `src/server/files/resolve.rs`

`search.rs` と `resolve.rs` は `list_markdown_files_from_canonical_base` を使うため、ファイル一覧の列挙結果に依存する。ただし API 型や呼び出し契約は変更しない。

## ロールバック

変更範囲は `catalog.rs` と catalog 周辺テストに閉じる。問題が出た場合は、helper 抽出と追加テストのコミットを revert すれば元の分岐構造へ戻せる。

ロールバック後も既存の base 外シンボリックリンク拒否、サイクル検出、最大深度、最大件数の挙動は維持される。

## 工数見積もり

- 人間作業: 45-75 分
- Codex/AI 支援作業: 20-40 分
