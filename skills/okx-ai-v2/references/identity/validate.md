# ASP Listing Validation

Validate ASP listings.

## Validate listing

- Create: after explicit Done for every service, validate the full identity and service set.
- Update: after collection, run only when the agent name/description or a service create/update
  changed. Use new-or-current identity values and only changed create/update services.
- Both: call `validate-listing` once after collection, never in a service loop or after corrections.

## Add semantic checks

Keep all CLI findings and add only what requires semantic judgment:

- Service name: enforce the noun-phrase rule in the
  [`serviceName` contract](service-contract.md#servicename).
- Agent name: require a brand, not a personal/public-figure name or substring.
- A2A description: treat missing core capability as advisory.
- A2MCP description: block any semantic violation of the contract's
  [four-item structure](service-contract.md#servicedescription). On failure, localize and show only:
  - Reason: `The request description is incomplete — it is missing one or more of: what the service does, the parameter specification, the request method, or the CURL request example. Buyers and the sandbox cannot determine how to call this service.`
  - Suggestion: `In the request description, include all four: (1) what the service does, (2) each key parameter — all on one line, separated by ;, in the format name(type, required/optional): meaning (append the default value for an optional parameter), (3) the request method (POST/GET or tool name), (4) a working CURL example using the real endpoint.`

## Present results

- Preserve CLI severities. Use only the rules above for semantic severity and exceptions; never
  restate or reinterpret them.
- If there are no findings, say QA passed.
- Otherwise, map dotted `field` values to identity/service card rows; translate and de-duplicate
  `message` by `(field,message)`; never show `code`; retain originals; bold affected name rows.

## Resolve findings

- Ask one localized choice set, then redraw.
- Label every semantic rewrite `drafted from your words — please review` and obtain normal
  confirmation. If rejected, recollect and redraw.
- No safe draft: if a required field is blank or cannot be safely derived from the user's input,
  ask the user for it before offering correction choices.
- Any blocker: `1 Use the drafted corrections / 2 I'll revise`. Apply only drafts derived from
  the user's words; advisory drafts remain optional.
- Advisory only: `1 Skip and keep original / 2 Use suggestion / 3 I'll revise`.
