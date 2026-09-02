# Identity service contract

Use this contract for ASP service payload fields, display, and create workflows. For update behavior,
follow [`identity-update.md`](identity-update.md); for validation, follow
[`identity-validate-listing.md`](identity-validate-listing.md). In mixed batches, apply the workflow
matching each service's `serviceType`.

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
| `subscription` | A2A only; see [§1. fee, subscription, and freeTrial](#1-fee-subscription-and-freetrial) |
| `freeTrial` | A2A monthly only; fixed value below |
| `endpoint` | A2MCP only; see [§3. endpoint](#3-endpoint) |
| `operation` | Update only: `create`/`update`/`delete`; omit during register |
| `id` | Update/delete only: copy the fetched `serviceId` into `id`; never use numeric raw `id`. Delete sends only `operation` and `id` |

Trim text fields. `0` is a valid fee. Reject bare JSON numbers, units/symbols, and approximations.
Use exact camelCase keys.

## Display Rules

- Description: render each `serviceDescription` verbatim on create confirmation and update diff,
  preserving all content and line breaks; never summarize, rewrite, or omit any part.
- Fee: non-zero → `N USDT`; zero → localized Free; empty/inapplicable → `—`. Exception: legacy A2A
  with neither fee nor subscription displays Free; A2MCP without fee remains `—`.
- Subscription: `N USDT / month`; zero → Free.
- Trial: Skill-guided writes only use `freeTrial:"72"`. The CLI accepts other positive-hour values
  only for legacy write-back. On reads, show the CLI-normalized value verbatim so legacy
  positive-hour values remain visible as `N days` or `N hours`; never recompute it skill-side.
- Guide: show non-blank A2A `serviceGuide` only on create confirmation/update diff. Never display it
  for A2MCP. `service-list` and `service-match` omit it for every service type.

## Collection flow

1. Confirm the exact `serviceType`, then enter the matching A2A or A2MCP workflow.
2. Accept batched answers and collect only missing fields.
3. After each service, ask **1. Add another service / 2. Done** and wait for explicit Done. If the
   user chooses 1, return to `serviceType` collection and repeat the matching workflow.
4. Use only user-provided data; preserve fetched values only when update rules require it.
5. Run validation only after explicit Done.

## A2A create workflow

Actions:

1. Collect fields in the order below.
2. After all fields are collected, continue to the QA gate.

#### 1. fee, subscription, and freeTrial

Actions:

1. Ask one numbered billing pick plus its price.

User-facing prompt/options:

> Choose a billing model:
> 1. Per call
> 2. Monthly
> 3. Monthly + 3-day trial

Rules:

1. Store the selected billing model as follows:

| Pick | Store |
|---|---|
| 1 per-call | `fee:"N", subscription:[]`; omit trial |
| 2 monthly | `fee:"", subscription:[{"interval":"month","fee":"N"}]`; omit trial |
| 3 monthly + 3-day trial | same as 2 plus `freeTrial:"72"` |

2. Never offer both models, a non-monthly interval, or another trial duration.
3. Ask only for missing or ambiguous values.
4. For another trial length, explain that only 3 days is supported and re-ask 2/3.

#### 2. A2A serviceName and serviceDescription

Actions:

1. Ask for the service name and description together.

Rules:

1. Require core capability + audience; for signal services also require signal kind.
2. Accept optional user inputs and delivery/copy-trading notes on separate lines. Numbering is
   optional.
3. Keep supplied content; never invent/chase optional parts or require a particular paragraph,
   label, order, audience, market, example, disclaimer, or style.
4. Recommend ≤2000 East-Asian display width (about 1000 CJK characters; CJK=2, ASCII=1), with no
   per-part limit.

#### 3. serviceGuide

Actions:

1. If `serviceGuide` is absent, show the following prompt.

User-facing prompt/options:

> Describe the prerequisites, steps, and key parameters. For trading, payments, or authorization,
> include confirmation requirements and execution limits.
> [Service Guide Examples](https://web3.okx.com/onchainos/dev-docs/okxai/a2a-subscription).
>
> Send the guide body, or reply 2 to skip.

Rules:

1. Accept non-blank text directly; bare `1` asks for the body and `2` omits `serviceGuide`.
2. Trim, preserve, and submit every supplied value.
3. Leave guide-length validation to the CLI.

## A2MCP create workflow

Actions:

1. Collect fields in the order below.
2. After all fields are collected, continue to the QA gate.

#### 1. fee

Actions:

1. Ask for the fee first.

Rules:

1. Forbid `subscription` and `freeTrial`.

#### 2. A2MCP serviceName and serviceDescription

Actions:

1. Ask for the service name and request description together.

User-facing prompt/options:

> Provide the service name and request description. Format the request description as four numbered
> lines using the required labels and formats.

Rules:

1. Store four numbered lines with localized bracketed labels:

   1. `[Service Description]` — purpose.
   2. `[Parameter Spec]` — key parameters on one `;`-separated line as
      `name(type, required/optional): meaning`; append optional defaults.
   3. `[Request Method]` — HTTP verb or bare MCP tool name.
   4. `[Request Example]` — runnable `curl` against the endpoint with realistic inputs.

2. Preserve supplied labels and add missing ones.
3. Use locale-appropriate punctuation and required/optional terms. Recommend total width ≤2000
   (about 1000 CJK characters; CJK=2, ASCII=1).
4. Keep key parameters when the full enumeration does not fit. If the parameter spec is malformed,
   normalize it to the required one-line form, show it, and obtain separate confirmation before
   storage.
5. Strip URL/path text from line 3, keeping only the verb/tool name. If a path has no verb, default
   to POST unless ambiguous. Apply stripping silently and show the stored value in the normal final
   confirmation/diff card.
6. Collect the request example after the endpoint is available. Convert non-curl examples and reject
   placeholder or mismatched hosts; obtain separate confirmation before storage. Allow A2MCP curl
   URLs.
7. Never show a fill-in template.

#### 3. endpoint

Actions:

1. Ask for the endpoint after the fee and request description.

Rules:

1. Require a deployed public HTTPS endpoint of ≤512 characters.
2. Reject HTTP, localhost, loopback, RFC-1918, `*.local`, `*.internal`, mocks, and placeholders.
3. Explain that an on-chain endpoint change requires update.
4. If no endpoint is available, ask the user to deploy one first or choose A2A.

## Validate

At the register/update QA gate, follow `identity-validate-listing.md` for timing, input scope, update-key
stripping, semantic merging, and finding resolution.
