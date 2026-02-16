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
