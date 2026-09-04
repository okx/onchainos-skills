mod common;

use common::onchainos;

const REFERENCE: &str = include_str!("../../skills/okx-ai/references/task-cli-reference.md");

fn section(start: &str, end: &str) -> &'static str {
    let from = REFERENCE
        .find(start)
        .unwrap_or_else(|| panic!("missing documentation section: {start}"));
    let rest = &REFERENCE[from..];
    let to = rest
        .find(end)
        .unwrap_or_else(|| panic!("missing documentation boundary: {end}"));
    &rest[..to]
}

fn help(args: &[&str]) -> String {
    let output = onchainos()
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run onchainos help: {e}"));
    assert!(
        output.status.success(),
        "help command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("help output must be UTF-8")
}

#[test]
fn pending_decisions_reference_lists_the_cli_subcommands() {
    let cli_help = help(&["agent", "pending-decisions-v2", "--help"]);
    let intro = section("### pending-decisions-v2", "#### request");
    for command in [
        "request",
        "request-prompt",
        "resolve",
        "resolve-with-sessionkey",
        "resolve-prompt",
        "pick",
        "list",
        "cancel",
    ] {
        assert!(cli_help.contains(command), "CLI help is missing {command}");
        assert!(
            intro.contains(command),
            "reference intro is missing {command}"
        );
    }
    assert!(!intro.contains("four subcommands"));
}

#[test]
fn request_reference_matches_content_input_flags() {
    let cli_help = help(&["agent", "pending-decisions-v2", "request", "--help"]);
    let docs = section("#### request", "#### request-prompt");

    for flag in ["--user-content", "--user-content-file"] {
        assert!(cli_help.contains(flag), "CLI help is missing {flag}");
        assert!(docs.contains(flag), "request reference is missing {flag}");
    }
    assert!(!cli_help.contains("--continuation-id"));
    assert!(!docs.contains("--continuation-id"));
    assert!(docs.contains("Required unless `--user-content-file`"));
    assert!(docs.contains("Required unless `--user-content`"));
}

#[test]
fn request_prompt_reference_matches_flags_and_single_pass_contract() {
    let cli_help = help(&["agent", "pending-decisions-v2", "request-prompt", "--help"]);
    let docs = section("#### request-prompt", "#### resolve-prompt");

    for flag in [
        "--user-content",
        "--user-content-file",
        "--template-vars-b64",
    ] {
        assert!(cli_help.contains(flag), "CLI help is missing {flag}");
        assert!(
            docs.contains(flag),
            "request-prompt reference is missing {flag}"
        );
    }
    assert!(docs.contains("originates in an input template"));
    assert!(docs.contains("inserted literally and is not scanned or expanded again"));
    assert!(!docs.contains("a literal placeholder can never be pushed"));
}
