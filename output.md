# Stage 9.1 — Integration Test verify

**Status**: DONE
**Skill**: `front-end-rust-verify-integration-loop`, `front-end-debug` (via `debug-fixer-rust` dispatch), `front-end-subagent-dispatch-rules`
**Repo**: `onchainos-skills/` — crate `cli/` (`tech-stack: rust-bin-only`, binary `onchainos`)
**Feature**: Unified `atomic_write` file-persistence & permissions refactor

## Skip check
`integration-plan.csv` (A-06a) is not present in this stage's CoW workspace — it was declared
input-only to Stage 7.1 and never propagated downstream. Stage 7.1's `output.md` (input A-09a)
already recorded the same skip check: 13 data rows (IT-001…IT-205), not header-only → NOT a
skip. Proceeded with the full verify workflow.

## Workflow executed
1. Read `project-context.md`: `build-command` = `cargo build` (debug) but the loaded
   `/front-end-rust-verify-integration-loop` skill's own Step 1 is `cargo build --release`
   (matches repo `CLAUDE.md`'s documented dev binary) — followed the skill's explicit
   instruction. `test-integration-command` = `cargo test --tests`. `app-home-env-var` =
   `ONCHAINOS_HOME`, exported to a fresh per-stage sandbox
   (`cli/target/test_tmp/integration/`). No `base-url-env-var` declared — none exported.
2. `cargo build --release` — PASS (4m 56s).
3. `cargo test --tests` — surfaced ~230 failures across the *entire* test suite (this command
   runs all `tests/*.rs`, not just this feature's).
4. **Scope triage**: built a disposable `git worktree` at the current commit's clean tree
   (pre-this-requirement's uncommitted diff) and ran the identical `cargo test --tests` there.
   14 test files (`cli_defi`, `cli_draft`, `cli_gateway`, `cli_leaderboard`, `cli_portfolio`,
   `cli_security`, `cli_signal`, `cli_social`, `cli_swap`, `cli_token`, `cli_trenches`,
   `cli_wallet`, `cli_workflows`, `mcp_server`) had byte-for-byte identical failure counts on
   the baseline — confirmed pre-existing, unrelated to this requirement (root cause: live
   tests need an authenticated wallet session not available in this sandbox; `cli_draft.rs`
   tests a `draft` subcommand removed by an unrelated already-merged refactor). Excluded from
   this stage's verdict, same scoping precedent Stage 8.1 used for pre-existing clippy errors.
5. This requirement's own 13 rows (in `cli_market.rs`, `cli_wallet_persistence.rs`,
   `cli_ws.rs`) showed 4 genuine rule failures (IT-004, IT-104, IT-201, IT-202) plus 1 infra
   failure (IT-003, live/auth) plus 2 documented mock-stub ignores (IT-002, IT-205); the rest
   passed.
6. Dispatched 3 parallel foreground `debug-fixer-rust` sub-agents (variables-only prompts) for
   the 4 rule failures:
   - `self_heal_permissions()` (spec §8.3, `home.rs` + `main.rs`) + `audit.jsonl` 0600 creation
     (spec §8.5 row 4, `audit.rs`) → fixes IT-201 + IT-004.
   - `config.json` cwd→home migration (spec §4/AC#9, §8.5 row 1, `config.rs`) → fixes IT-202.
   - DoH binary-path boundary check eager-wiring (spec §8.5 row 6/§8.7, `doh/manager.rs` +
     2 call-site propagations) → fixes IT-104.
   All 3 returned DONE with no regressions.
7. Rebuilt release binary, re-ran the full suite: all 14 pre-existing-failure files unchanged
   (no regressions), `cli_wallet_persistence.rs` and `cli_ws.rs` fully green,
   `cli_market.rs` down to 18 failures (17 pre-existing + IT-003).
8. Re-ran IT-003 in isolation 3x — failed identically each time (confirmed non-flaky,
   auth/infra precondition, correctly tagged `infra`, does not block verdict).
9. Cleaned up the disposable baseline worktree.

## Output
Wrote `oli-docs/zoeiw2gxqiyzhzkaxejlhhkzgyc/verify/integration-results.md` (artifact A-11a).

## Summary

| Condition | Status |
|-----------|--------|
| `cargo build --release` failed after 3 attempts | ✅ PASS (green both before and after fix loop) |
| Any non-infra integration test still failing after fix loop | ✅ none in this requirement's scope — IT-004/IT-104/IT-201/IT-202 fixed, IT-003 tagged `infra` |
| `integration-results.md` not written | ✅ written |

**Overall: PASS**
