//! Integration tests — CLI command-layout surface guards for the
//! agentic_wallet directory-spec refactor (WWINFRA-3500 / branch
//! `feat/cliFileLayoutSpec`).
//!
//! Source plan: `oli-docs/kob5wtqqoidlktk7d5slopb4gzf/integration-plan.csv`
//!   rows IT-001…IT-009. Spec: `oli-docs/kob5wtqqoidlktk7d5slopb4gzf/spec.md`.
//!
//! ─── Why one file for four top-level areas ────────────────────────────────────
//! These nine rows are a single, cohesive acceptance suite for a *move-only*
//! refactor whose invariant is "the CLI surface is byte-identical" (spec §1).
//! They span the top-level menu plus the `wallet`, `strategy` and `swap`
//! commands only to cross-check that the module reorganization did not perturb
//! the clap tree. Keeping them together — rather than scattering guards into the
//! unrelated `cli_wallet.rs` (WOO-96 sysLocale login suite) and `cli_swap.rs`
//! (live swap smoke tests) files — keeps the refactor's guard suite findable and
//! avoids mixing requirements. This mirrors the repo's existing concern-scoped
//! test-file convention (`cli_wallet_login_mode.rs`, `cli_wallet_testnet.rs`).
//!
//! ─── Conventions ──────────────────────────────────────────────────────────────
//!   - Every row is `network_required: offline`: `--help` renders and clap parse
//!     errors are resolved before any home/network access, so no `ONCHAINOS_HOME`
//!     sandbox and no `run_with_retry` are needed (and none is used).
//!   - The binary is invoked through the shared `common::onchainos()` builder.
//!   - No base URL or environment-specific hostname is referenced anywhere.
//!   - Assertions check the presence of the structural tokens the refactor must
//!     preserve (subcommand names, flag long-names, clap error text) — not exact
//!     help formatting, which would be brittle without guarding anything extra.

mod common;

use common::onchainos;
use predicates::prelude::*;

// ════════════════════════════════════════════════════════════════════════════
//  Top level — `onchainos --help`
// ════════════════════════════════════════════════════════════════════════════

// ── IT-001: top-level --help still lists wallet, strategy and swap ────────────
//   Guards the main.rs dispatch re-paths (Phase 1 & Phase 4). `strategy` stays a
//   top-level command even though Phase 4 physically moves it under
//   agentic_wallet (spec §1 / Appendix C).
#[test]
fn top_level_help_lists_wallet_strategy_swap() {
    onchainos()
        .arg("--help")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("wallet")
                .and(predicate::str::contains("strategy"))
                .and(predicate::str::contains("swap")),
        );
}

// ════════════════════════════════════════════════════════════════════════════
//  wallet
// ════════════════════════════════════════════════════════════════════════════

// ── IT-002: `wallet --help` still lists every wallet action ───────────────────
//   Guards the Phase 1 WalletCommand facade move (wallet.rs enum → mod.rs). A
//   variant dropped during the move would vanish from this tree. Representative
//   variants across the moved domains are asserted (the CSV mandates
//   `gas-station`).
#[test]
fn wallet_help_lists_all_actions() {
    onchainos()
        .args(["wallet", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("gas-station")
                .and(predicate::str::contains("send"))
                .and(predicate::str::contains("login"))
                .and(predicate::str::contains("contract-call"))
                .and(predicate::str::contains("sign-message")),
        );
}

// ── IT-003: `wallet gas-station --help` still shows its management actions ─────
//   Guards the Phase 1 GasStationCommand import-path change and confirms the
//   enum still wires as a nested subcommand (spec Appendix B).
#[test]
fn wallet_gas_station_help_lists_management_actions() {
    onchainos()
        .args(["wallet", "gas-station", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("setup")
                .and(predicate::str::contains("enable"))
                .and(predicate::str::contains("disable"))
                .and(predicate::str::contains("status"))
                .and(predicate::str::contains("update-default-token")),
        );
}

// ── IT-006: `wallet send --help` still shows the full Send flag surface ────────
//   Confirms the Send variant and its flags moved intact from wallet.rs into
//   mod.rs during Phase 1 (CSV mandates `--recipient`).
#[test]
fn wallet_send_help_shows_flag_surface() {
    onchainos()
        .args(["wallet", "send", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("--recipient")
                .and(predicate::str::contains("--amt"))
                .and(predicate::str::contains("--readable-amount"))
                .and(predicate::str::contains("--contract-token"))
                .and(predicate::str::contains("--enable-gas-station")),
        );
}

// ── IT-007: `wallet send` rejects --amt together with --readable-amount ────────
//   Guards that the `--amt conflicts_with --readable-amount` attribute survived
//   the Phase 1 move; clap exits 2 before any network call. spec §3 exit-code
//   taxonomy (2 = usage/confirming class). Mirrors the cli_cross_chain.rs
//   conflict tests.
#[test]
fn wallet_send_amt_and_readable_amount_conflict() {
    onchainos()
        .args([
            "wallet",
            "send",
            "--amt",
            "1",
            "--readable-amount",
            "1",
            "--recipient",
            "0x0000000000000000000000000000000000000000",
            "--chain",
            "ethereum",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

// ── IT-008: an unknown `wallet` action is refused ─────────────────────────────
//   Guards that the WalletCommand enum still parses every declared variant after
//   the Phase 1 facade consolidation. clap exits 2 (spec §3).
#[test]
fn wallet_unrecognized_subcommand_errors() {
    onchainos()
        .args(["wallet", "bogus-subcommand"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

// ════════════════════════════════════════════════════════════════════════════
//  strategy
// ════════════════════════════════════════════════════════════════════════════

// ── IT-004: `strategy --help` still lists all limit-order actions ─────────────
//   Guards the Phase 4 move of commands/strategy into
//   commands/agentic_wallet/strategy plus the Context re-path (CSV mandates
//   `create-limit`).
#[test]
fn strategy_help_lists_limit_order_actions() {
    onchainos()
        .args(["strategy", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("create-limit")
                .and(predicate::str::contains("cancel"))
                .and(predicate::str::contains("list"))
                .and(predicate::str::contains("resume")),
        );
}

// ── IT-009: an unknown `strategy` action is refused ───────────────────────────
//   Guards that the StrategyCommand enum still parses intact after Phase 4
//   relocates it under agentic_wallet. clap exits 2 (spec §3).
#[test]
fn strategy_unrecognized_subcommand_errors() {
    onchainos()
        .args(["strategy", "bogus-subcommand"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

// ════════════════════════════════════════════════════════════════════════════
//  swap  — sibling command not moved by the refactor; rendered-identically
//          cross-check to catch collateral damage (spec §1)
// ════════════════════════════════════════════════════════════════════════════

// ── IT-005: `swap --help` output is unchanged after the move ──────────────────
//   A command tree NOT touched by the refactor must still render its actions
//   (CSV mandates `quote`).
#[test]
fn swap_help_lists_quote_and_swap() {
    onchainos()
        .args(["swap", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("quote").and(predicate::str::contains("swap")));
}
