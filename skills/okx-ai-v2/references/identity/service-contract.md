# Identity service contract

Use this contract for ASP service payload fields, display, and create workflows. For update behavior,
follow [`update.md`](update.md); for validation, follow
[`validate.md`](validate.md). In mixed batches, apply the workflow
matching each service's `serviceType`.

## Shared payload

`create`, `update`, and `validate-listing` share this case-sensitive `--service` element. **NEVER**
interchange descriptions: the Agent profile uses the top-level `--description` flag; each service
uses `serviceDescription` inside its `--service` element.

| Key | Rule |
|---|---|
| `serviceName` | Required 5–30 character noun phrase; differ from agent name; no price |
| `serviceDescription` | Required; follow the selected type below |
| `serviceGuide` | Optional for A2A; see [§3. serviceGuide](#3-serviceguide) |
| `serviceType` | Required raw `A2MCP` or `A2A`; display unchanged |
| `fee` | Quoted numeric string, including `"0"`; A2A monthly uses `""`; ≤6 decimals; no units, symbols, or approximations |
| `subscription` | A2A only; see [§1. fee, subscription, and freeTrial](#1-fee-subscription-and-freetrial) |
| `freeTrial` | A2A monthly only; fixed value below |
| `endpoint` | A2MCP only; see [§3. endpoint](#3-endpoint) |
| `operation` | Update only: `create`/`update`/`delete`; omit during register |
| `id` | Update/delete only: copy the fetched `serviceId` into `id`; never use numeric raw `id`. Delete sends only `operation` and `id` |

Trim text fields. Use exact camelCase keys.

## Display Rules

- Description: render each `serviceDescription` verbatim on create confirmation and update diff,
  preserving all content and line breaks; never summarize, rewrite, or omit any part.
- Pricing: zero fee/subscription → localized Free with no suffix; non-zero fee → `N USDT`;
  non-zero subscription → `N USDT / month`; empty/inapplicable → `—`.
- Trial: `freeTrial:"72"` displays as `3 days`; absent/inapplicable → `—`.
- Guide: show non-blank `serviceGuide` verbatim on create confirmation/update diff; omit it when
  absent or blank.

## Collection flow

1. Confirm the exact `serviceType`, then enter the matching A2A or A2MCP workflow.
2. Accept batched answers and collect only missing fields.
3. After each service, ask **1. Add another service / 2. Done** and wait for explicit Done. If the
   user chooses 1, return to `serviceType` collection and repeat the matching workflow.
4. Run validation only after explicit Done.

## A2A create workflow

Actions:

1. Collect fields in the order below.

#### 1. fee, subscription, and freeTrial

Actions:

1. Ask one numbered billing pick plus its price.

User-facing options:

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

2. Never combine per-call and monthly billing or use a non-monthly interval.
3. For another trial length, explain that only 3 days is supported and re-ask 2/3.

#### 2. A2A serviceName and serviceDescription

Actions:

1. Ask for the service name and description together.

Rules:

1. Require core capability + audience; for signal services also require signal kind.
2. Accept optional user inputs and delivery/copy-trading notes on separate lines. Numbering is
   optional.
3. Preserve supplied content; do not invent optional details or impose additional format or content
   requirements.
4. Recommend ≤2000 East-Asian display width (about 1000 CJK characters; CJK=2, ASCII=1), with no
   per-part limit.

#### 3. serviceGuide

Actions:

1. If `serviceGuide` is absent, show the following prompt.

User-facing prompt:

> Describe the prerequisites, steps, and key parameters. For trading, payments, or authorization,
> include confirmation requirements and execution limits.
> [Service Guide Examples](https://web3.okx.com/onchainos/dev-docs/okxai/a2a-subscription).
>
> Send the guide body, or reply 2 to skip.

Rules:

1. Accept non-blank text directly; `2` omits `serviceGuide`.
2. Preserve and submit every supplied value.
3. Leave guide-length validation to the CLI.

## A2MCP create workflow

Actions:

1. Collect fields in the order below.

#### 1. fee

Actions:

1. Ask for the fee first.

#### 2. A2MCP serviceName and serviceDescription

Actions:

1. Ask for the service name and request description together.

User-facing prompt:

> Provide the service name and request description. Format the request description as four numbered
> lines using the required labels and formats.

Rules:

1. Store exactly four numbered lines using the bracketed headings below or their localized
   equivalents. Preserve supplied headings and brackets, and add any missing headings:

   1. `[Service Description]` — purpose.
   2. `[Parameter Spec]` — must list key parameters on one `;`-separated line. Each parameter must
      use `name(type, required/optional): meaning`; append optional defaults. Keep key parameters if
      the full list does not fit. Normalize malformed specs and obtain separate confirmation.
   3. `[Request Method]` — HTTP verb or bare MCP tool name. Strip URL/path text and default an
      unambiguous path-only value to POST; show the stored result on the final card.
   4. `[Request Example]` — runnable `curl` against the endpoint with realistic inputs. Confirm it
      after the endpoint is available; convert non-curl examples with separate confirmation and
      reject placeholder or mismatched hosts.

2. Require total width <2000 (CJK=2, ASCII=1).

#### 3. endpoint

Actions:

1. Ask for the endpoint after the fee and request description, then confirm the request example
   against it.

Rules:

1. Require a deployed public HTTPS endpoint of ≤512 characters.
2. Reject HTTP, localhost, loopback, RFC-1918, `*.local`, `*.internal`, mocks, and placeholders.
3. Explain that an on-chain endpoint change requires update.
4. If no endpoint is available, ask the user to deploy one first or choose A2A.

## Validate

At the register/update QA gate, follow `validate.md` for timing, input scope, update-key
stripping, semantic merging, and finding resolution.
