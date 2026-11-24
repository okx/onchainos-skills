mod common;

use common::onchainos;
use predicates::prelude::*;

#[test]
fn message_eligible_help_advertises_offline_replay_flag() {
    onchainos()
        .args(["agent", "message-eligible", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--is-offline-replay"));
}

#[test]
fn message_eligible_offline_replay_requires_a_boolean_value() {
    onchainos()
        .args([
            "agent",
            "message-eligible",
            "--agent-id",
            "local-agent",
            "--client-agent-id",
            "client-agent",
            "--provider-agent-id",
            "provider-agent",
            "--job-id",
            "job-1",
            "--group-id",
            "group-1",
            "--direction",
            "provider_to_client",
            "--client-communication-address",
            "0xclient",
            "--provider-communication-address",
            "0xprovider",
            "--is-offline-replay",
            "not-a-boolean",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid value"));
}
