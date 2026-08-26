# Identity service contract

Use this single rule source for every ASP service create, update, validation, and display. In mixed
batches, apply the section matching each service's `serviceType`.

## Shared payload

`create`, `update`, and `validate-listing` share this case-sensitive `--service` element. **NEVER**
interchange descriptions: the Agent profile uses the top-level `--description` flag; each service
uses `serviceDescription` inside its `--service` element.

| Key | Rule |
|---|---|
| `serviceName` | Required 5–30 character noun phrase; differ from agent name; no price |
| `serviceDescription` | Required; follow the selected type below |
| `serviceGuide` | Optional for every A2A pricing model. Never offer, request, or display it for A2MCP; preserve a fetched non-blank legacy value on update |
| `serviceType` | Required raw `A2MCP` or `A2A`; display unchanged |
| `fee` | Quoted number, ≤6 decimals, no currency unit/symbol; type rules define its shape |
| `subscription` | A2A only; see [§A2A create and pricing](#a2a-create-and-pricing) |
| `freeTrial` | A2A monthly only; fixed value below |
| `endpoint` | A2MCP only; see [§A2MCP endpoint](#a2mcp-endpoint) |
| `operation` | Update only: `create`/`update`/`delete`; omit during register |
| `id` | Existing service id for update/delete; delete sends only `operation` and `id` |

Trim text fields. `0` is a valid fee. Reject bare JSON numbers, units/symbols, and approximations.
Use exact camelCase keys.

## Collect and route

Use only the user's replies. On update, preserve fetched values only where required; never use
email, wallet, or session metadata or invent capabilities, metrics, or optional content.

1. Ask service name + exact `A2MCP`/`A2A` type together.
2. Follow its section and ask missing pricing/endpoint fields. Do not ask for `serviceGuide` yet.
3. Collect and complete `serviceDescription`.
4. For an A2A registration, follow [§A2A serviceGuide](#a2a-serviceguide) after the description.
   On update, follow [§A2A update](#a2a-update). Never ask for `serviceGuide` on A2MCP.

Accept batched answers. During registration, after every service—including a fully batched first
service—ask **1. Add another service / 2. Done** and wait for explicit Done. Never validate during
collection.

## A2A

### A2A create and pricing

Ask one numbered billing pick plus its price:

| Pick | Store |
|---|---|
| 1 per-call | `fee:"N", subscription:[]`; omit trial |
| 2 monthly | `fee:"", subscription:[{"interval":"month","fee":"N"}]`; omit trial |
| 3 monthly + 3-day trial | same as 2 plus `freeTrial:"72"` |

Never offer both models, a non-monthly interval, or another trial duration. Ask only for missing or
ambiguous values. For another trial length, explain that only 3 days is supported and re-ask 2/3.

### A2A description

- Require core capability + audience; for signal services also require signal kind.
- Accept optional user inputs and delivery/copy-trading notes on separate lines. Numbering is
  optional.
- Keep supplied content; never invent/chase optional parts or require a particular paragraph,
  label, order, audience, market, example, disclaimer, or style.
- Recommend ≤2000 East-Asian display width (about 1000 CJK characters; CJK=2, ASCII=1), with no
  per-part limit.

### A2A serviceGuide

`serviceGuide` is optional for all A2A pricing models. If absent, ask:

> Describe the prerequisites, steps, and key parameters. For trading, payments, or authorization,
> include confirmation requirements and execution limits.
> [Service Guide Examples](https://web3.okx.com/onchainos/dev-docs/okxai/a2a-subscription).
>
> Send the guide body, or reply 2 to skip.

Accept non-blank text directly; bare `1` asks for the body and `2` omits `serviceGuide`.

Trim, preserve, and submit every supplied value. Leave guide-length validation to the CLI.

### A2A update

Apply [§Update delta](#update-delta). Keep an existing A2A billing model fixed even if the CLI
accepts a flip; the backend rejects it.

- Per-call: send current/new numeric `fee` and `subscription:[]`.
- Subscription: send `fee:""` and the current/new monthly tier; include `serviceGuide` only when
  non-blank.
- Trial: change only on explicit request; enable with `freeTrial:"72"`, disable by omission; never
  send `""` or `"0"`.
- Preserve a fetched non-blank guide unless explicitly changed. A missing/blank guide does not need
  to be filled during update.

To change billing models, add a new service and optionally remove the old one.

### A2A QA and display

- **Block:** description empty; test marker; URL.
- **Suggest:** over-length description or missing core capability.
- Wallet/contract addresses do not block. Ignore paragraph count. Delete entries bypass QA per
  [§Update delta](#update-delta).

Display per [§Display](#display). A2A has no endpoint.

## A2MCP

### A2MCP collect

Ask for quoted numeric per-call `fee`, deployed public HTTPS `endpoint`, and the four-part request
description below. Forbid `subscription` and `freeTrial`. A2MCP has no user-facing `serviceGuide`
option: never offer, request, or display one.

### A2MCP request description

Store four numbered lines with localized bracketed labels:

1. `[Service Description]` — purpose.
2. `[Parameter Spec]` — key parameters on one `;`-separated line as
   `name(type, required/optional): meaning`; append optional defaults.
3. `[Request Method]` — HTTP verb or bare MCP tool name.
4. `[Request Example]` — runnable `curl` against the endpoint with realistic inputs.

Preserve supplied labels and add missing ones. Use ASCII `( , ) : ;` in Latin-language
conversations; use full-width `（ ， ） ： ；` and localized required/optional words in CJK.
Recommend total width ≤2000 (about 1000 CJK characters; CJK=2, ASCII=1). Key parameters suffice
when all cannot fit; never block solely for incomplete enumeration.

If a present parameter spec is malformed, normalize it into that strict one-line form, show it, and
obtain a separate confirmation before storage. Strip URL/path text from line 3, keeping only the
verb/tool name; apply it silently and show the stored value on the normal final confirmation/diff
card. A path without a verb defaults to POST; ask only if ambiguous. Convert a non-curl example and
obtain a separate confirmation before storage. Reject placeholder/mismatched hosts. Allow A2MCP curl
URLs. Never show a fill-in template.

### A2MCP endpoint

Require deployed public HTTPS, ≤512 characters. Reject HTTP, localhost, loopback, RFC-1918,
`*.local`, `*.internal`, mocks, and placeholders. Explain that an on-chain endpoint change requires
update. Without one, deploy first or choose A2A.

### A2MCP QA

All four request-description items block by meaning. A description containing `0x` + 40 hex
characters is exempt and accepted as-is. Empty text and test markers also block. On four-item
failure, localize and show only:

- Reason: `The request description is incomplete — it is missing one or more of: what the service does, the parameter specification, the request method, or the CURL request example. Buyers and the sandbox cannot determine how to call this service.`
- Suggestion: `In the request description, include all four: (1) what the service does, (2) each key parameter — all on one line, separated by ;, in the format name(type, required/optional): meaning (append the default value for an optional parameter), (3) the request method (POST/GET or tool name), (4) a working CURL example using the real endpoint.`

### A2MCP update and display

Apply [§Update delta](#update-delta). Send current/new `fee`, endpoint, and description. On update,
preserve a fetched non-blank `serviceGuide` internally so unrelated edits do not erase legacy data;
never render it. Never send subscription fields. Display per [§Display](#display).

## Update delta

Send changed services only. Create: `operation:"create"` without `id`. Modify: use fetched `id` and
other parser-required current fields. Delete: send only `operation:"delete"` and the fetched `id`.
Omit unchanged services; never interpret omission as deletion or suggest deletion without an
explicit request. Delete entries bypass listing QA. Apply the matching type's update rules before
building the payload.

## Validate

At the register/update QA gate, follow `identity-validate-listing.md` for timing, input scope, update-key
stripping, semantic merging, and finding resolution.

## Display

- Fee: non-zero → `N USDT`; zero → localized Free; empty/inapplicable → `—`. Exception: legacy A2A
  with neither fee nor subscription displays Free; A2MCP without fee remains `—`.
- Subscription: `N USDT / month`; zero → Free.
- Trial: Skill-guided writes only use `freeTrial:"72"`. The CLI accepts other positive-hour values
  only for legacy write-back. On reads, show the CLI-normalized value verbatim so legacy
  positive-hour values remain visible as `N days` or `N hours`; never recompute it skill-side.
- Guide: show non-blank A2A `serviceGuide` only on create confirmation/update diff. Never display it
  for A2MCP. `service-list` and `service-match` omit it for every service type.
