const SKILL: &str = include_str!("../../skills/okx-ai/SKILL.md");
const CLI_REFERENCE: &str =
    include_str!("../../skills/okx-ai/references/identity-cli-reference.md");
const REGISTER: &str = include_str!("../../skills/okx-ai/references/identity-register.md");
const UPDATE: &str = include_str!("../../skills/okx-ai/references/identity-update.md");
const DISCOVER: &str = include_str!("../../skills/okx-ai/references/identity-discover.md");
const LISTING: &str = include_str!("../../skills/okx-ai/references/identity-listing.md");
const REVIEWS: &str = include_str!("../../skills/okx-ai/references/identity-reviews.md");
const SERVICE_CONTRACT: &str =
    include_str!("../../skills/okx-ai/references/identity-service-contract.md");
const VALIDATE_LISTING: &str =
    include_str!("../../skills/okx-ai/references/identity-validate-listing.md");
const ERRORS: &str = include_str!("../../skills/okx-ai/references/identity-errors.md");
const TASK_CLI: &str = include_str!("../../skills/okx-ai/references/task-cli-reference.md");
const TASK_PUBLISH: &str =
    include_str!("../../skills/okx-ai/references/task-user-actions-publish.md");
const AI_GUIDE: &str = include_str!("../../skills/okx-guide/references/ai-guide.md");
const REGISTERED_HOME: &str = include_str!("../../skills/okx-guide/references/registered-home.md");
const UNREGISTERED_ROLE_SELECTION: &str =
    include_str!("../../skills/okx-guide/references/unregistered-role-selection.md");
const AGENT_COMMAND_SOURCE: &str = include_str!("../src/commands/agent_commerce/mod.rs");
const IDENTITY_ARGS_SOURCE: &str = include_str!("../src/commands/agent_commerce/identity/args.rs");

fn flatten(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn identity_cli_reference_is_compact_and_owns_shared_cli_rules() {
    let cli = flatten(CLI_REFERENCE);
    assert!(CLI_REFERENCE.lines().count() <= 110);
    assert!(cli.contains("Agent identities live on XLayer"));
    assert!(cli.contains("Run each call prescribed by the active flow once"));
    assert!(!cli.contains("reload after context compaction"));
}

#[test]
fn identity_cli_reference_preserves_command_contracts() {
    for command in [
        "agent pre-check --role <user|asp|evaluator> [--consent-key <uuid>]",
        "agent upload --file <local-image-path>",
        "agent create --role <role> --name <name>",
        "agent update --agent-id <id>",
        "agent validate-listing --role <role>",
        "`agent get-my-agents`",
        "`agent get-agents`",
        "`agent service-list`",
        "`agent feedback-list`",
        "agent service-match [--keywords <k...>]",
        "agent service-match --search-after <cursor>",
        "agent activate --agent-id <id> --preferred-language <BCP-47>",
        "agent deactivate --agent-id <id>",
        "agent search --query <text>",
        "agent get [--agent-ids <ids>]",
        "agent get-by-address --communication-address <address>",
        "agent xmtp-sign --key-uuid <uuid> --message <text>",
    ] {
        assert!(
            CLI_REFERENCE.contains(command),
            "missing contract: {command}"
        );
    }
    assert!(CLI_REFERENCE.contains("Never add `--chain`, `--address`, or undocumented `--format`"));
    assert!(CLI_REFERENCE.contains("`agent consent` (the command does not exist)"));
}

#[test]
fn identity_command_inventory_tracks_the_entire_rust_namespace() {
    let identity_block = AGENT_COMMAND_SOURCE
        .split("// ── Identity")
        .nth(1)
        .expect("identity command section")
        .split("// ── Task system")
        .next()
        .expect("task command boundary");

    assert_eq!(identity_block.matches("identity::").count(), 18);
    for variant in [
        "Create(",
        "Update(",
        "Get(",
        "GetMyAgents(",
        "GetAgents(",
        "Precheck(",
        "GetByAddress(",
        "Activate(",
        "Deactivate(",
        "Upload(",
        "Search(",
        "ServiceList(",
        "ServiceMatch(",
        "FeedbackSubmit(",
        "FeedbackList(",
        "TaskFeedback(",
        "XmtpSign(",
        "ValidateListing(",
    ] {
        assert!(
            identity_block.contains(variant),
            "untracked Rust command: {variant}"
        );
    }
}

#[test]
fn task_flows_own_task_feedback_commands() {
    assert!(TASK_CLI.contains(
        "agent feedback-submit --agent-id <ratee> --creator-id <rater> --score <0.00..5.00>"
    ));
    assert!(TASK_CLI.contains("agent task-feedback --agent-id <rater> --task-id <jobId>"));
    assert!(!TASK_CLI.contains("--score <0-100>"));

    assert!(!CLI_REFERENCE.contains("agent feedback-submit"));
    assert!(!CLI_REFERENCE.contains("agent task-feedback"));

    assert!(TASK_PUBLISH.contains("identity-cli-reference.md"));
    assert!(!TASK_PUBLISH.contains("onchainos agent get-my-agents --role user"));
    assert!(!TASK_PUBLISH.contains("onchainos agent service-list --agent-id"));

    assert!(AI_GUIDE.contains("../../okx-ai/references/identity-cli-reference.md"));
    assert!(REGISTERED_HOME.contains("../../okx-ai/references/identity-cli-reference.md"));
    assert!(REGISTERED_HOME.contains("./unregistered-role-selection.md"));
    assert!(UNREGISTERED_ROLE_SELECTION.contains("../../okx-ai/references/identity-register.md"));
    assert!(!REGISTERED_HOME.contains("onchainos agent search --query"));
}

#[test]
fn identity_write_gates_are_preserved() {
    let cli = flatten(CLI_REFERENCE);
    let register = flatten(REGISTER);
    let update = flatten(UPDATE);
    let service_contract = flatten(SERVICE_CONTRACT);
    let validate_listing = flatten(VALIDATE_LISTING);
    assert!(register.contains("`agent pre-check` **requires** `--role`"));
    assert!(register.contains("Invoke the initial form from `identity-cli-reference.md`"));
    assert!(register.contains("confirmation cannot be skipped or reused from an earlier action"));
    assert!(update.contains("Obtain fresh explicit confirmation for the final diff"));
    assert!(service_contract
        .contains("During registration, after every service—including a fully batched first"));
    assert!(service_contract.contains("wait for explicit Done"));
    assert!(!update.contains("Add another / Done"));
    assert!(validate_listing.contains("**Update:** after collection"));
    assert!(cli.contains("never follow a successful write with a query or poll"));
    assert!(register.contains("Continue to §4 only after explicit Done"));
    assert!(register.contains("it never runs `agent create`"));
    assert!(register.contains("I won't run anything until you reply **1**"));
    assert!(update.contains("before collecting any change"));
}

#[test]
fn identity_update_documents_parseable_service_delta_shapes() {
    let prefix = "onchainos agent update --agent-id 42 --service '";
    let mut operations = Vec::new();
    let mut service_types = Vec::new();

    for line in UPDATE.lines().filter(|line| line.starts_with(prefix)) {
        let raw = line
            .strip_prefix(prefix)
            .and_then(|value| value.strip_suffix('\''))
            .expect("complete quoted --service example");
        let parsed: serde_json::Value =
            serde_json::from_str(raw).expect("service example must be valid JSON");
        let entries = parsed.as_array().expect("service delta array");
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        let operation = entry["operation"]
            .as_str()
            .expect("service delta operation");
        operations.push(operation.to_string());
        if operation == "delete" {
            assert_eq!(entry.as_object().expect("delete delta object").len(), 2);
            assert!(entry["id"].is_string());
            continue;
        }
        for field in ["serviceName", "serviceDescription", "serviceType", "fee"] {
            assert!(!entry[field].is_null(), "{operation} missing {field}");
        }
        let service_type = entry["serviceType"].as_str().expect("service delta type");
        service_types.push(service_type.to_string());
        match service_type {
            "A2A" => {
                let subscription = entry["subscription"]
                    .as_array()
                    .expect("A2A subscription array");
                assert!(entry["endpoint"].is_null());
                if subscription.is_empty() {
                    assert!(!entry["fee"].as_str().expect("A2A per-call fee").is_empty());
                } else {
                    assert_eq!(entry["fee"], "");
                    assert_eq!(subscription.len(), 1);
                    assert_eq!(subscription[0]["interval"], "month");
                    assert!(subscription[0]["fee"].is_string());
                    assert!(entry["serviceGuide"].is_string());
                }
            }
            "A2MCP" => {
                assert!(entry["endpoint"].is_string());
                assert!(entry["subscription"].is_null());
            }
            other => panic!("unexpected service type: {other}"),
        }
        assert_eq!(entry["id"].is_null(), operation == "create");
    }

    operations.sort();
    operations.dedup();
    assert_eq!(operations, ["create", "delete", "update"]);
    service_types.sort();
    service_types.dedup();
    assert_eq!(service_types, ["A2A", "A2MCP"]);
    assert!(IDENTITY_ARGS_SOURCE.contains(r#"{"operation":"delete","id":"svc_123"}"#));
}

#[test]
fn identity_read_and_toggle_behavior_is_preserved() {
    let discover = flatten(DISCOVER);
    let listing = flatten(LISTING);
    let reviews = flatten(REVIEWS);
    assert!(discover.contains("user's original utterance verbatim"));
    assert!(discover.contains("do not repeat initial-search filters"));
    assert!(discover.contains("preserving the returned Agent and Service order"));
    assert!(discover.contains("chain exactly ONE"));
    assert!(discover.contains("never auto-chain `feedback-list`"));
    assert!(listing.contains("card-exempt"));
    assert!(listing.contains("never chase a successful toggle"));
    assert!(reviews.contains("Use the CLI-provided 0.00–5.00 star values directly"));
}

#[test]
fn identity_historical_service_behavior_is_preserved() {
    let contract = flatten(SERVICE_CONTRACT);
    let qa = flatten(VALIDATE_LISTING);

    // Reads preserve legacy positive-hour trials, while skill-guided writes remain fixed to 72 hours.
    assert!(contract.contains("Skill-guided writes only use `freeTrial:\"72\"`"));
    assert!(
        contract.contains("legacy positive-hour values remain visible as `N days` or `N hours`")
    );
    assert!(contract.contains("another trial duration"));

    // Discovery reads do not expose buyer-facing service guides.
    assert!(contract.contains("`service-list` and `service-match` omit it for every service type"));

    // Agent and service descriptions remain distinct inputs.
    assert!(contract.contains("Agent profile uses the top-level `--description` flag"));
    assert!(
        contract.contains("each service uses `serviceDescription` inside its `--service` element")
    );

    // Request-method URL/path cleanup is deterministic and does not add a rewrite-confirmation turn.
    assert!(contract.contains("Strip URL/path text from line 3"));
    assert!(contract.contains("apply it silently"));
    assert!(contract.contains("show the stored value on the normal final confirmation/diff card"));
    assert!(contract.contains("obtain a separate confirmation before storage"));
    assert!(qa.contains(
        "The only no-separate-confirmation exception is A2MCP Request Method URL/path stripping"
    ));
    assert!(qa.contains(
        "malformed parameter specs and non-curl examples—must be shown and separately confirmed before storage"
    ));

    // Existing A2A services still cannot switch billing model in place.
    assert!(contract.contains("Keep an existing A2A billing model fixed"));
    assert!(contract.contains("add a new service and optionally remove the old one"));
}

#[test]
fn optional_a2a_service_guide_and_guided_a2mcp_behavior_are_consistent() {
    let contract = flatten(SERVICE_CONTRACT);
    let errors = flatten(ERRORS);
    let args = flatten(IDENTITY_ARGS_SOURCE);

    assert!(contract.contains("### A2A serviceGuide"));
    assert!(contract.contains("### A2MCP collect"));
    assert!(contract.contains("follow [§A2A serviceGuide]"));
    assert!(
        SERVICE_CONTRACT.find("### A2A description").unwrap()
            < SERVICE_CONTRACT.find("### A2A serviceGuide").unwrap()
    );
    assert!(contract.contains("optional for all A2A pricing models"));
    assert!(contract.contains("Describe the prerequisites, steps, and key parameters"));
    assert!(contract.contains("confirmation requirements and execution limits"));
    assert!(contract.contains(
        "[Service Guide Examples](https://web3.okx.com/onchainos/dev-docs/okxai/a2a-subscription)"
    ));
    assert!(contract.contains("Send the guide body, or reply 2 to skip"));
    assert!(!contract.contains("Subscription A2A: require a non-blank guide"));
    assert!(!contract.contains("do not offer or accept Skip"));
    assert!(!contract.contains("optional: do not prompt for it"));
    assert!(contract.contains("Leave guide-length validation to the CLI"));
    assert!(contract.contains("A2MCP has no user-facing `serviceGuide` option"));
    assert!(contract.contains("never offer, request, or display one"));
    assert!(contract.contains("preserve a fetched non-blank `serviceGuide` internally"));
    assert!(contract.contains("Never display it for A2MCP"));
    assert!(contract.contains("`service-list` and `service-match` omit it for every service type"));
    assert!(args.contains("optional for every A2A pricing model"));
    assert!(args.contains("CLI accepts and forwards an"));
    assert!(args.contains("explicitly supplied A2MCP value"));
    assert!(
        !errors.contains("missing required field in --service for A2A subscription: serviceGuide")
    );
    assert!(!errors.contains("The service guide for [<serviceName>] exceeds the length limit"));
    assert!(contract.contains("Use this single rule source"));
    assert!(!contract.contains("serviceGuide invariant"));
    assert!(!errors.contains("serviceGuide invariant"));
}

#[test]
fn identity_shared_rules_have_single_owners() {
    let cli = flatten(CLI_REFERENCE);
    let discover = flatten(DISCOVER);
    let contract = flatten(SERVICE_CONTRACT);
    let errors = flatten(ERRORS);
    let listing = flatten(LISTING);

    assert!(contract.contains("`service-list` and `service-match` omit it for every service type"));
    assert!(!discover.contains("Never display `serviceGuide`"));

    assert!(cli.contains("Use service `id` only to build an update/delete delta; never display it"));
    assert!(discover.contains(
        "[identity-cli-reference.md §Read and discovery](identity-cli-reference.md#read-and-discovery)"
    ));
    assert!(!discover.contains("Never display the raw `serviceId`"));

    assert_eq!(discover.matches("do not repeat initial-search filters").count(), 1);
    assert!(!cli.contains("A continuation cannot repeat initial filters"));

    assert!(listing.contains("`submitApproval.success: true`"));
    assert!(listing.contains("`submitApproval.success: false`"));
    assert!(listing.contains("`activate.approvalStatus: 2`"));
    assert!(!errors.contains("activate / submit-approval outcomes"));
    assert!(!errors.contains("submit-approval success:"));
    assert!(!errors.contains("manage.md"));
}

#[test]
fn identity_qa_and_failure_gates_are_preserved() {
    let qa = flatten(VALIDATE_LISTING);
    let errors = flatten(ERRORS);

    assert!(qa.contains("Call `validate-listing` exactly once after collection"));
    assert!(qa.contains("never call it inside a service loop or rerun it after corrections"));
    assert!(qa.contains("de-duplicate `message` by `(field,message)`"));
    assert!(qa.contains("never show `code`"));
    assert!(qa.contains("The final create/update card still requires confirmation"));

    assert!(errors.contains("Redaction overrides verbatim"));
    assert!(errors.contains("strip/redact that token before showing it"));
    assert!(errors.contains("Never auto-retry"));
    assert!(errors.contains("Never chase a failure"));
}

#[test]
fn identity_skill_owns_runtime_prerequisites() {
    assert_eq!(SKILL.matches("## Pre-flight Checks").count(), 1);
    assert_eq!(SKILL.matches("## Language Lock").count(), 1);
    assert!(
        SKILL.contains("Creating, updating, activating, and deactivating an agent costs nothing")
    );
}

#[test]
fn evaluator_registration_continues_to_staking() {
    let register = flatten(REGISTER);
    assert!(register.contains("run only [`task-core.md`](task-core.md) §Pre-flight"));
    assert!(register.contains(
        "follow the matching scenario in [`task-evaluator-staking.md`](task-evaluator-staking.md)"
    ));
}

#[test]
fn identity_references_do_not_self_bootstrap_runtime_prerequisites() {
    for (name, reference) in [
        ("cli", CLI_REFERENCE),
        ("discover", DISCOVER),
        ("errors", ERRORS),
        ("validate-listing", VALIDATE_LISTING),
        ("listing", LISTING),
        ("register", REGISTER),
        ("reviews", REVIEWS),
        ("service-contract", SERVICE_CONTRACT),
        ("update", UPDATE),
    ] {
        assert!(
            !reference.contains("../../okx-agentic-wallet/_shared/preflight.md"),
            "{name} duplicates preflight instead of inheriting SKILL.md"
        );
        assert!(
            !reference.contains("Lock replies to the user's first language"),
            "{name} duplicates the language lock instead of inheriting SKILL.md"
        );
    }
}
