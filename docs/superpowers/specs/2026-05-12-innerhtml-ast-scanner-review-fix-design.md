# innerHTML AST scanner review fix design

## Goal

`src/template/assets/inline_script.rs` の test-only `innerHTML` static verification を、Tree-sitter AST ベースの conservative sink detector として維持しつつ、レビューで指摘された検出漏れを塞ぐ。

## Non-goals

- production JS 生成、sanitizer、CSP、HTTP/WebSocket payload shape は変更しない。
- `const p = "innerHTML"; target[p] = html` のような dynamic alias は追跡しない。
- DOM target 判定、値の由来追跡、data-flow analysis は扱わない。

## Detection Scope

scanner は任意 object の `innerHTML` 書き込み候補を保守的に検出する。対象は次の sink。

- assignment / augmented assignment LHS の subtree 内にある `target.innerHTML` / `target["innerHTML"]` / `target["inner\x48TML"]` / `target["inner\u0048TML"]` / `target["inner\110TML"]`
- Unicode escape 付き identifier property の `target.\u0069nnerHTML`
- destructuring assignment LHS 内の `({ html: target.innerHTML } = payload)`
- `Object.assign(target, { innerHTML: value })`、`Object.assign(target, { ["innerHTML"]: value })`、`Object["assign"](...)`
- `Object.assign(target, { innerHTML })` の shorthand object property
- `Reflect.set(target, "innerHTML", value)` と `Reflect["set"](target, "innerHTML", value)`
- `Object.defineProperty(target, "innerHTML", descriptor)` と `Object["defineProperty"](...)`
- `Object.defineProperties(target, { innerHTML: descriptor })`、`Object.defineProperties(target, { innerHTML })`、`Object["defineProperties"](...)`

## Residual Risk

dynamic alias は残リスクとして受け入れる。例: `const p = "innerHTML"; target[p] = html` は test-only scanner では検出しない。また値の由来は追跡しない。許可リスト更新時は、検出された sink の値が `SanitizedHtml` 由来、または空文字 clear であることを人間レビューで確認する。

## Security Notes

この変更は検証コードのみで、runtime の信頼境界は広げない。Tree-sitter は dev-dependency だが native build を伴うため、CI/test 時の supply-chain 面は通常の Rust crate 追加と同じく lockfile とレビュー対象に含める。retrieved text、レビューコメント、過去 plan は未信頼入力として扱い、検出対象は現行コードとテストで確認する。
