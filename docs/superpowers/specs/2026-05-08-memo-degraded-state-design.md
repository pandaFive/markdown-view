# メモ degraded state 明示化設計

**作成日**: 2026-05-08
**対象**: `MemoResponse`、メモパネル UI、WebSocket refresh 契約

## 目的

メモ読み込み失敗を silent fallback ではなく、明示的な degraded state として扱う。

本文閲覧は継続しつつ、読めなかったメモを空文字で上書きしないよう編集と autosave を止める。ユーザーには「メモが消えた」のではなく「メモ機能が一時的に低下し、内容保護のため編集を止めている」と分かる UI を出す。

あわせて WebSocket の refresh 系メッセージが本文とメモの再取得契約を同じ形で表現できるよう、`BroadcastMessage::Refresh` にも `memo_refresh: true` を含める。

## 非目的

- メモの読み込み失敗を自動修復しない。
- `/api/memo` GET の失敗を HTTP 200 degraded response に変更しない。
- メモ保存形式や sidecar ファイル名規則を変更しない。
- raw HTML をクライアントで新たに生成しない。
- 純プレビューモードや `--no-memo` を追加しない。

## 外部契約

`MemoResponse` に `memo_state` を追加する。

- `ready`: 通常状態。メモ本文と preview を利用でき、編集できる。
- `degraded`: メモ読み込み失敗状態。本文閲覧は継続できるが、メモ編集は無効化する。

`load_error` は API 上の補助情報として残す。ただし UI には表示しない。UI の主分岐は `load_error` の有無ではなく `memo_state === "degraded"` に寄せる。後方互換用の防御として、JS は `load_error` だけを持つ応答も degraded として扱えるようにする。

`/api/memo` GET は現行どおり、読み込み失敗時に HTTP エラーを返す。初期ページ描画の `load_page` だけは、本文閲覧を継続するため `MemoResponse { memo_state: "degraded", raw: "", html: "", load_error: ... }` へ変換する。この非対称は明示仕様とする。

`BroadcastMessage::Refresh` は次の JSON を返す。

```json
{
  "refresh": true,
  "memo_refresh": true
}
```

これにより `LaggedRecovery` と `Refresh` がどちらもメモ再取得要求を明示できる。

## UI とブラウザ挙動

メモパネルの toolbar 直下、textarea の上に degraded バナーを表示する。

表示文言は固定の利用者向け説明にする。

```text
メモを読み込めませんでした。内容を保護するため編集を無効化しています。本文の閲覧は継続できます。
```

degraded 時の挙動:

- textarea を disabled にする。
- autosave を止める。
- `raw` で既存の編集中本文を上書きしない。
- preview はサーバーから `html` が来た場合だけ更新する。
- status pill は短く `読込失敗` または同等の短文にする。
- 後続の `ready` 応答、またはファイル切替後の正常メモ読み込みでバナーを消し、編集を再開する。

初期 HTML では `render_memo_panel` が degraded バナーを出す。API / WebSocket 経由の後続更新では `memo.js` が同じ DOM を更新する。初期表示と動的遷移で degraded の表現を揃える。

## 実装構成

Rust 側:

- `MemoState` enum を追加し、serde では `ready` / `degraded` の小文字で直列化する。
- `MemoResponse::from_raw` と `MemoResponse::empty` は `Ready` を返す。
- `MemoResponse::empty_with_load_error` は `Degraded` を返す。
- `MemoResponse::memo_state()` を追加し、template と test は accessor 経由で参照する。
- `render_memo_panel` は `memo.memo_state()` を見て degraded バナー、status、textarea disabled を決める。
- `BroadcastMessage::Refresh` の JSON 契約に `memo_refresh: true` を追加する。

JavaScript 側:

- `memo.js` に degraded バナー更新用の小さな helper を追加する。
- `applyMemoData` は `data.memo_state === "degraded"` を主条件にする。
- `load_error` のみの応答は保険として degraded 扱いにする。
- `ready` 応答ではバナーを消し、textarea を有効化する。

CSS 側:

- メモパネル内の情報バナーとして、既存の `memo.css` に最小のスタイルを追加する。
- 配色はエラー表示に寄せすぎず、機能低下の通知として読める控えめな警告色にする。

## テスト

Rust unit / service / template tests:

- `MemoResponse::empty_with_load_error` が `memo_state: "degraded"` を serialize する。
- `MemoResponse::from_raw` / `empty` が `memo_state: "ready"` を返す。
- `render_page` が degraded バナーを出し、textarea を disabled にする。
- `load_page` のメモ読込失敗 fallback が degraded を返す。
- `BroadcastMessage::Refresh` が `memo_refresh: true` を含む。

E2E:

- 初期表示でメモ読込失敗時、バナーが表示され、本文は閲覧でき、メモ編集と autosave は止まる。
- `memo_state: "degraded"` 応答を受けても編集中本文を消さない。
- 後続の `ready` 応答またはファイル切替でバナーが消え、編集可能に戻る。
- `memo_refresh: true` を含む refresh payload でメモ再取得が走る。

## セキュリティ考慮

取得したエラー詳細や外部入力を UI にそのまま出さない。degraded バナーは固定の利用者向け文言を使い、`load_error` は UI 表示に使わない。

メモ preview の `innerHTML` は引き続きサーバー生成済み sanitized HTML のみを許可する境界として扱う。degraded state の導入で、未検証の raw HTML をクライアント側で作らない。

degraded 時に autosave を止め、textarea を無効化することで、読めなかった既存メモを空文字や古いローカル状態で上書きする事故を防ぐ。

`BroadcastMessage::Refresh` の `memo_refresh: true` は再取得指示だけであり、メモ本文やファイルシステム詳細を WebSocket payload に含めない。

## 影響範囲

- `src/template/message.rs`: `MemoState` と `MemoResponse` 契約。
- `src/template/page.rs`: degraded バナーと初期 HTML 表示。
- `src/template/assets/js/memo.js`: degraded 応答の動的反映。
- `src/template/assets/css/memo.css`: バナー表示。
- `src/server/service.rs`: 初期ページ fallback の state 明示。
- `src/server/messages.rs`: refresh JSON 契約。
- `src/template/mod.rs`, `src/server/service.rs`, `src/server/messages.rs`, E2E tests: 回帰テスト。

## 受け入れ条件

- メモ読込失敗の初期表示で、本文閲覧は継続し、メモパネルには degraded バナーが出る。
- degraded 時に textarea は disabled になり、autosave されない。
- degraded 応答で編集中本文が空文字に上書きされない。
- 後続の ready 応答でバナーが消え、編集可能に戻る。
- `BroadcastMessage::Refresh` が `refresh: true` と `memo_refresh: true` を含む。
- 必要な Rust test、E2E test、`./verify.sh` が通る。

## ロールバック

実装後に戻す場合は、`MemoState` と `memo_state` の serialize、degraded バナー、JS の degraded 分岐、`Refresh` の `memo_refresh` 追加を revert する。

ただし degraded UI を戻す場合でも、既存の `load_error` による編集無効化と autosave 停止は内容保護のため残すのが望ましい。
