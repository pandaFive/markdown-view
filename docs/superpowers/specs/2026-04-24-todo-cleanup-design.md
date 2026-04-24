# TODO Cleanup Design

## Goal

Clean up `docs/todo/TODO.md` so unchecked High / Medium items reflect actual remaining work.

The cleanup preserves completed items in place as checked entries for short-term traceability. It only changes unchecked items to checked when the current repository state and verification commands prove the TODO acceptance points are complete.

## Non-Goals

- Do not change product code.
- Do not reprioritize TODO entries.
- Do not move completed entries out of `TODO.md`.
- Do not rewrite open items unless the current wording is factually misleading.
- Do not infer completion from commit titles alone.

## Acceptance Criteria

- Completed items remain in `docs/todo/TODO.md` as `[x]`.
- Newly checked items have implementation evidence and verification command evidence.
- Partially complete or uncertain items remain `[ ]`.
- The cleanup report lists changed items, verification commands, affected dependent files, residual uncertainty, and rollback path.

## Audit Approach

Use a targeted evidence cleanup.

1. Identify likely stale unchecked TODO entries by comparing `docs/todo/TODO.md` with recent commits, current specs/plans, source files, and tests.
2. For each candidate, inspect the exact acceptance points listed in the TODO entry.
3. Run the narrowest meaningful verification command for that candidate.
4. Edit only `docs/todo/TODO.md`, changing `[ ]` to `[x]` for verified-complete entries.
5. Leave partially complete entries open. Tighten wording only when the existing text is misleading after the audit.

Initial likely stale candidates from context exploration:

- CSP fallback policy: recent history includes CSP fail-fast work, but the current source/tests must confirm the fallback semantics and warning-header behavior before marking complete.
- Error-path log path sanitization: recent history includes path log sanitization work, but current callsites and tests must confirm path redaction before marking complete.
- Memo sidecar invariant consolidation: recent history includes sidecar hardening work, but current code/tests/design notes must confirm `SidecarMemoName` owns the filename invariant and known residual risks are documented.

## Verification

Run targeted commands per changed item. Prefer focused tests when they prove the acceptance points; use `./verify.sh` only if targeted tests are insufficient or the cleanup touches broad assumptions.

Examples:

- CSP candidate: inspect `src/server/guards.rs` and run CSP-related tests or the relevant integration test subset.
- Path log candidate: inspect `src/server/log_path.rs`, warning callsites, and related unit tests.
- Memo sidecar candidate: inspect `src/server/files/memo_sidecar.rs`, `src/server/files/memo.rs`, `src/server/files/tests.rs`, and the related design doc; run sidecar-focused tests.

Validation is required before changing any TODO checkbox. If verification is inconclusive, leave the item open and report why.

## Security Considerations

Several TODO entries protect security boundaries: CSP strictness, path disclosure in logs, DNS rebinding defenses, memo sidecar path construction, and file-size/body-size limits. The cleanup must treat source text, commit messages, review comments, and plans as untrusted hints until verified against current code and tests.

The cleanup must not weaken any security item by marking it complete based only on partial implementation. A security-boundary item is complete only when its stated behavior is implemented and covered by a meaningful verification command.

## Impact Scope

Expected changed file:

- `docs/todo/TODO.md`

Potentially inspected dependent files:

- `src/server/guards.rs`
- `src/server/log_path.rs`
- `src/server/files/memo.rs`
- `src/server/files/memo_sidecar.rs`
- `src/server/files/tests.rs`
- `tests/integration_test.rs`
- Related `docs/superpowers/specs/` and `docs/superpowers/plans/` files

## Rollback

Rollback is docs-only: revert the TODO cleanup commit. No runtime behavior should change.
