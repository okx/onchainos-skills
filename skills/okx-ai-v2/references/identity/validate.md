# Validate

ASP create/update listing QA.

## Run once

- **Create:** after explicit Done for every service, validate the full identity and service set.
- **Update:** after collection, run only when the agent name/description or a service create/update
  changed. Use new-or-current identity values and only changed create/update services. Omit
  [`operation`](service-contract.md#operation) and [`id`](service-contract.md#id) from
  `validate-listing`; pass `--service '[]'` when no service is validated.
- **Both:** call `validate-listing` once after collection, never in a service loop or after corrections.

## Merge findings

### Semantic additions

Keep all CLI findings and add only what requires semantic judgment:

- **Service name:** enforce the noun-phrase rule in the
  [`serviceName` contract](service-contract.md#servicename).
- **Agent name:** require a brand, not a personal/public-figure name or substring; draft only a
  neutral brand derived from the user's meaning.
- **A2A description:** treat missing core capability as advisory. Do not add findings for
  wallet/contract addresses or paragraph count.
- **A2MCP description:** block any semantic violation of the contract's
  [four-item structure](service-contract.md#servicedescription), but accept `0x` followed by 40
  hexadecimal characters as-is. On failure, localize and show only:
  - Reason: `The request description is incomplete — it is missing one or more of: what the service does, the parameter specification, the request method, or the CURL request example. Buyers and the sandbox cannot determine how to call this service.`
  - Suggestion: `In the request description, include all four: (1) what the service does, (2) each key parameter — all on one line, separated by ;, in the format name(type, required/optional): meaning (append the default value for an optional parameter), (3) the request method (POST/GET or tool name), (4) a working CURL example using the real endpoint.`

### Render

- Preserve CLI severities. Use only the rules above for semantic severity and exceptions; never
  restate or reinterpret them.
- If there are no findings, say QA passed.
- Otherwise, map dotted `field` values to identity/service card rows; translate and de-duplicate
  `message` by `(field,message)`; never show `code`; retain originals; bold affected name rows.

## Resolve findings

### Draft and choose

- Ask one localized choice set, then redraw; do not write or rerun validation.
- Label every semantic rewrite `✏️ drafted from your words — please review` and obtain normal
  confirmation. If rejected, recollect and redraw. Never silently correct/store/repeat a draft,
  invent field content, or force advice.
- **No safe draft:** before choices, ask directly for every blocking required identity/service field
  that is blank or lacks enough user content for a safe correction. Stay blocked until valid and
  non-blank; never offer or apply `Use the drafted corrections` to it.
- **Any blocker:** `1 Use the drafted corrections / 2 I'll revise`. Apply only drafts derived from
  the user's words; advisory drafts remain optional.
- **Advisory only:** `1 Skip and keep original / 2 Use suggestion / 3 I'll revise`.

### Normalize

- Silently strip A2MCP Request Method URL/path text only as defined by the
  [`serviceDescription` contract](service-contract.md#servicedescription), then show it on the final
  card.
- Before storage, show and separately confirm every other normalization—including malformed
  parameter specs and non-curl examples—as required by that contract.
