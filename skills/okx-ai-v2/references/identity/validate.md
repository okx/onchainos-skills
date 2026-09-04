# Validate

Apply during ASP create or update validation.

## Invoke once

- **Create:** wait for explicit Done after every service; validate the full identity and all services.
- **Update:** after collection, run only if agent name/description or a
  service create/update changed. Use new-or-current identity values and only changed create/update
  services without `operation`/`id`; pass `--service '[]'` when no service is validated.
- Call `validate-listing` exactly once after collection; never call it inside a service loop or rerun
  it after corrections. User and Evaluator skip listing QA.

## Merge findings

Always merge the CLI result with these semantic checks:

- service name is a descriptive noun phrase, not one letter;
- agent name is a brand, not a personal/public-figure name or substring; draft only a neutral brand
  alternative derived from the user's meaning;
- Apply the matching [type-specific QA](#type-specific-qa) rules for A2A or A2MCP.

Preserve every CLI finding's severity. For type-specific semantic severity and exceptions, use only
the matching type section's QA rules; do not restate or reinterpret them here.

If nothing is found, say QA passed. Otherwise map each dotted `field` to its identity/service card
row, translate and de-duplicate `message` by `(field,message)`, never show `code`, retain original
values, and render the affected name row in bold.

## Type-specific QA

### A2A QA

1. Block empty descriptions, test markers, and URLs.
2. Suggest changes for over-length descriptions or missing core capability.
3. Do not block wallet or contract addresses.
4. Ignore paragraph count.
5. Skip listing QA for delete entries.

### A2MCP QA

1. Block any request description missing one or more of the four required items by meaning.
2. Accept a description containing `0x` followed by 40 hexadecimal characters as-is.
3. Block empty text and test markers.
4. On four-item failure, localize and show only:
   - Reason: `The request description is incomplete — it is missing one or more of: what the service does, the parameter specification, the request method, or the CURL request example. Buyers and the sandbox cannot determine how to call this service.`
   - Suggestion: `In the request description, include all four: (1) what the service does, (2) each key parameter — all on one line, separated by ;, in the format name(type, required/optional): meaning (append the default value for an optional parameter), (3) the request method (POST/GET or tool name), (4) a working CURL example using the real endpoint.`

## Resolve findings

Ask one localized choice set, then redraw; never write yet and never rerun validation:

Mark every semantic rewrite on the card as `✏️ drafted from your words — please review` and obtain
normal confirmation. If rejected, recollect and redraw; never silently store a semantic rewrite.

- **No safe draft:** before offering choices, identify every blocking field that is blank or lacks
  enough user-provided content to derive a safe correction. Ask the user for each such field
  directly and keep the flow blocked until they provide a valid non-blank value; do not offer or
  apply `Use the drafted corrections` for that field. This applies to every required identity or
  service field. After all no-draft blockers are resolved, use the choices below for any remaining
  findings.
- **Any blocker:** `1 Use the drafted corrections / 2 I'll revise`. Apply only drafts derived from
  the user's words; advisory drafts remain optional.
- **Advisory only:** `1 Skip and keep original / 2 Use suggestion / 3 I'll revise`.

Never silently correct a semantic finding, repeat a draft, invent field content, or force advice.
The only no-separate-confirmation exception is A2MCP Request Method URL/path stripping defined in
[`service-contract.md` §serviceDescription](service-contract.md#servicedescription);
apply it silently and show the result on the normal final card. All other
normalizations—including malformed parameter specs and non-curl examples—must be shown and
separately confirmed before storage as required by that section. A2MCP failure uses only the
A2MCP-section reason and suggestion and remains blocked. The final create/update card still
requires confirmation.
