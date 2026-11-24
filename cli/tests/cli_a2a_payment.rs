use assert_cmd::Command;
use predicates::str::contains;

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("onchainos"))
}

#[test]
fn public_entry_point_exposes_optional_payment_tokens_and_funding_recovery() {
    for (subcommand, flag) in [
        ("create-task", "--payment-token-symbol"),
        ("create-subscribe", "--service-token-address"),
        ("task-create-prepare", "--funding-token-symbol"),
    ] {
        command()
            .args(["agent", subcommand, "--help"])
            .assert()
            .success()
            .stdout(contains(flag));
    }
}

#[test]
fn explicit_contract_conflict_fails_before_auth_or_signing() {
    command()
        .args(["agent", "create-task"])
        .args([
            "--title",
            "Report",
            "--description",
            "Summarize the market inputs",
            "--provider-agent-id",
            "asp",
            "--payment-token-amount",
            "10",
            "--service-token-amount",
            "10",
            "--service-id",
            "uuid",
            "--payment-token-symbol",
            "USDC",
            "--service-token-address",
            "0x779dEd0c9e1022225f8E0630b35a9b54bE713736",
        ])
        .assert()
        .failure()
        .stdout(contains(
            "payment token symbol and service token address differ",
        ));
}

#[test]
fn funding_choices_are_bounded_and_cannot_enter_a2mcp() {
    command()
        .args([
            "agent",
            "task-create-prepare",
            "--sid",
            "123",
            "--funding-token-symbol",
            "DAI",
        ])
        .assert()
        .failure()
        .stderr(contains("invalid value"));
    command()
        .args([
            "agent",
            "task-create-prepare",
            "--sid",
            "123",
            "--service-type",
            "A2MCP",
            "--asp-agent-id",
            "asp",
            "--funding-token-symbol",
            "USDG",
        ])
        .assert()
        .failure()
        .stdout(contains("funding-token-symbol is only supported for A2A"));
}

#[test]
fn payment_token_recovery_constraint_cannot_enter_a2mcp() {
    command()
        .args([
            "agent",
            "task-create-prepare",
            "--sid",
            "123",
            "--service-type",
            "A2MCP",
            "--asp-agent-id",
            "asp",
            "--payment-token-symbol",
            "USDC",
        ])
        .assert()
        .failure()
        .stdout(contains("--payment-token-symbol is only supported for A2A"));
}

#[test]
fn creation_funding_routes_stay_in_the_user_flow_and_preserve_reconfirmation() {
    let prepare = include_str!("../../skills/okx-ai/references/a2a/user/create-prepare.md");
    let root = include_str!("../../skills/okx-ai/SKILL.md");
    let router = include_str!("../../skills/okx-ai/references/a2a/user/router.md");
    for action in ["select_funding_token", "fund_selected_token"] {
        assert!(prepare.contains(action));
        assert!(!root.contains(action));
        let route = router
            .lines()
            .find(|line| line.starts_with('|') && line.contains(&format!("`{action}`")))
            .expect("funding action must have a User Router mapping");
        let reference = route.split('|').nth(2).expect("Reference column");
        assert!(reference.contains("[`create-prepare.md`](create-prepare.md)"));
    }
    for page in [
        include_str!("../../skills/okx-ai/references/a2a/user/create.md"),
        include_str!("../../skills/okx-ai/references/a2a/user/subscription-create.md"),
    ] {
        assert!(!page.contains("--service-token-address <payload.feeToken>"));
        assert!(!page.contains("--payment-token-symbol <payload.feeTokenSymbol>"));
        assert!(page.contains("USD"));
        assert!(page.contains("confirmation card"));
        assert!(page.contains("`create-prepare.md`"));
        assert!(page.contains("payload.resumeContext"));
        assert!(page.contains("never creates automatically"));
        for action in ["select_funding_token", "fund_selected_token"] {
            assert!(page.contains(action));
        }
    }
    assert!(prepare.contains("--funding-token-symbol <chosen-USDC-USDG-or-USDT>"));
    assert!(prepare.contains("Ask for final task confirmation again"));
}
