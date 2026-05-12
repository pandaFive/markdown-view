# innerHTML AST scanner review fix plan

## Scope

`src/template/assets/inline_script.rs` の test-only scanner を強化し、`innerHTML` sink allowlist の static verification を AST ベースで固定する。runtime JS、sanitizer、CSP、HTTP/WebSocket 契約は変更しない。

## Tasks

1. RED tests を追加する。
   - parse error: 壊れた JS で `inner_html_sinks` が失敗する。
   - escaped identifier: `target.\u0069nnerHTML = unsafeHtml;`
   - augmented assignment: `target.innerHTML += unsafeHtml;`
   - computed object key: `Object.assign(target, { ["innerHTML"]: unsafeHtml });`
   - escaped computed object key: `Object.assign(target, { ["inner\\x48TML"]: unsafeHtml });`
   - defineProperties: `Object["defineProperties"](target, { innerHTML: { value: unsafeHtml } });`
   - legacy octal string: `target["inner\\110TML"] = unsafeHtml;`
2. Tree-sitter JavaScript parser を dev-dependency として追加する。
3. scanner を実装する。
   - parse tree に error recovery があれば失敗させる。
   - assignment / augmented assignment LHS subtree を走査する。
   - identifier、string/template literal、computed property name の property 名を正規化する。
   - Unicode escape と legacy octal escape を decode する。
   - `Object.assign`、`Reflect.set`、`Object.defineProperty`、`Object.defineProperties` の computed member call を dot member call と同じ名前へ正規化する。
4. bundled inline JS の `innerHTML` sink allowlist をテストで固定する。
5. docs に検出範囲、security note、dynamic alias 残リスクを明記する。

## Verification

- `cargo fmt --all -- --check`
- `cargo test --all-targets --all-features inline_script::tests`
- `cargo test --all-targets --all-features`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `./verify.sh`
- `git diff --check`

## Rollback

この変更は test-only scanner と docs に閉じる。問題があれば、`Cargo.toml` / `Cargo.lock` の dev-dependency 追加、`inline_script.rs` の test module、追加 docs 2 件を revert する。
