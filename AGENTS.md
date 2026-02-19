# Principles

1. If a problem occurs, it must be resolved.
2. After resolving a problem, add test cases to prevent regression.
3. Everything is testable. If testing is not possible in the current setup, add tools to make it possible.
4. Prefer execution over suggestions: if an additional actionable fix is found in the same scope, continue and implement it immediately instead of proposing it first.

## Auto-Continue Policy
After completing the requested change, immediately continue fixing any build/test/runtime errors discovered during verification in the same scope.

Do not ask for confirmation for these follow-up fixes.

Stop only when:
1. A destructive action is required.
2. A product decision is required.
3. Secrets/credentials are required.

Completion criteria:
1. The failing command used for verification passes.
2. Related tests pass.

## Test Hook Policy
For test validation gates, use `pre-commit` and `pre-push`.
Treat both hook stages as required checks when available.

## Codex Frequent Failure Guardrails (from recent sessions)
1. In `zsh`, always quote or escape paths containing brackets or glob-like tokens (for example `"[id]"`, `"[...path]"`) before `cat/sed/rg` commands.
2. Do not assume file locations. Verify with `rg --files` (or `ls`) first, then read/edit the confirmed path.
3. Use repository hook scripts at `.githooks/pre-commit` and `.githooks/pre-push` for validation gates, not `.git/hooks/*`.
4. For GitHub Actions inspection, use the run database ID from `gh run list` and valid `gh run view` flags only.
5. Use `write_stdin` only for sessions started with `tty=true` and still interactive; otherwise rerun the command non-interactively.
6. Run `node/npm/pnpm` commands in the package directory that owns `package.json` and dependencies.

## Auto-Improvement Routine
Trigger this routine automatically whenever a command, test, hook, or runtime check fails.

1. Reproduce the failure with the smallest deterministic command and capture the exact error signature.
2. Classify the root cause (`path/quoting`, `tool-usage`, `dependency/context`, `logic/test`, `infra/flaky`) and fix the root cause, not only the symptom.
3. Add a regression safeguard in the same scope: a test case when feasible, otherwise a validation script/check that fails fast.
4. Re-run in this order until clean: originally failing command, related test target, `.githooks/pre-commit`, then `.githooks/pre-push`.
5. If the same failure signature appears repeatedly, update this `AGENTS.md` with a new guardrail in the same change.

## Harness Auto-Improve Loop
Follow the canonical loop in `docs/harness-auto-improve-loop.md`.

1. Start from a concrete failing harness command (not a broad workspace run).
2. Iterate in a tight fix loop on the smallest scope until stable.
3. Promote only after local stability to `pre-commit` and `pre-push`.
4. Merge only with a regression artifact (test/fixture/check) that would fail without the fix.
5. Record repeated failure classes back into this `AGENTS.md` to reduce future entropy.
