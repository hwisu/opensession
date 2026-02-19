# Harness Auto-Improve Loop

This loop is the operational playbook for agent-first development.
It is inspired by harness-engineering principles: keep AGENTS concise, move details to docs, optimize for repeatable execution, and continuously manage entropy.

References:
- OpenAI, ["Harness engineering: leveraging Codex in an agent-first world"](https://openai.com/index/harness-engineering)
- OpenAI, ["Unlocking the Codex harness: how we built the App Server"](https://openai.com/index/codex-app-server)

## Loop Goals

1. Fix failures at the smallest executable scope first.
2. Convert every meaningful fix into a regression safeguard.
3. Promote confidence in stages instead of jumping directly to full-workspace checks.
4. Feed repeated failures back into process rules.

## Inputs

Required:
- failing command (or failing test) that can be executed locally
- current change scope (crate/package/path)

Optional:
- known error signature from CI or local logs

## Step 1: Pin A Deterministic Reproduction

1. Start with the narrowest command that reproduces the failure.
2. Capture the exact signature: command, exit code, first failing line.
3. If flaky, run the same command 3 times and record pass/fail counts.

Examples:
- `cargo test -p opensession-parsers parser_name::case_name -- --nocapture`
- `cargo test -p opensession-daemon --quiet`
- `(cd /Users/hwisookim/ops/web && npm run check)`

## Step 2: Build A Focused Harness

Use a focused harness that validates only the failing behavior.

1. Prefer existing unit/integration tests.
2. If missing, add one of:
- a unit/integration test
- a parser fixture/conformance test
- a deterministic validation script

Definition of done for this step:
- the new harness fails before the fix and passes after the fix.

## Step 3: Tight Fix Loop

Run this cycle until stable:

1. Apply minimal code change.
2. Run focused harness.
3. If failed, refine and repeat.
4. Require 2 consecutive passes before promotion.

Rules:
- fix root cause, not symptom suppression
- avoid widening scope until focused harness is stable

## Step 4: Promotion Gates

After focused stability, promote confidence in order:

1. Originally failing command
2. Related test target(s)
3. `.githooks/pre-commit`
4. `.githooks/pre-push`

If any gate fails:

1. return to Step 1 with the new failing signature
2. repeat the same loop in that narrower failing scope

## Step 5: Regression Artifact Requirement

Every non-trivial fix must leave a safeguard.

Accepted safeguards:
- test case
- parser fixture
- deterministic check script used in hooks or CI

Rejected safeguard:
- manual verification only

## Step 6: Entropy Control

When the same class of failure repeats:

1. add/update a guardrail rule in `/Users/hwisookim/ops/AGENTS.md`
2. add or refine harness coverage for that class
3. remove obsolete rules when no longer applicable

## Workspace Gate Map

Repository-required validation gates:

1. Fast gate: `/Users/hwisookim/ops/.githooks/pre-commit`
2. Full gate: `/Users/hwisookim/ops/.githooks/pre-push`

Current gate intent:

1. pre-commit: formatting + daemon smoke-level tests
2. pre-push: clippy, worker wasm checks, frontend check, migration parity, workspace tests, e2e compile smoke

## Stop Conditions

Only stop the loop when one of the following holds:

1. destructive action is required
2. product decision is required
3. secrets/credentials are required

Otherwise continue until all required gates pass.
