# Identity service contract

Use this contract for ASP service fields, payloads, and collection. For update behavior, follow
[`update.md`](update.md); for validation, follow [`validate-listing.md`](validate-listing.md); for
display, follow [`output-templates.md`](output-templates.md).

## Fields and payload

The case-sensitive `--service` element is shared across create, update, and `validate-listing`.
Never interchange descriptions: the Agent profile uses the top-level `--description` flag; each
service uses `serviceDescription` inside its `--service` element.

Trim text values and use exact camelCase keys.

### Field summary

| Key | Applies to | Rule |
|---|---|---|
| `serviceName` | Both | Required 5–30 character noun phrase; differ from agent name; no price. |
| `serviceDescription` | Both | Required; follow [serviceDescription](#servicedescription). |
| `serviceGuide` | A2A | Optional on create; follow [serviceGuide](#serviceguide). |
| `serviceType` | Both | Required exact `A2MCP` or `A2A`. |
| `fee` | Both | Required; follow [Billing](#billing). |
| `subscription` | A2A | Required billing-model value; omit for A2MCP. |
| `freeTrial` | A2A monthly | `"72"` or omitted. |
| `endpoint` | A2MCP | Required; follow [endpoint](#endpoint). |
| `operation` | Update | `create`/`update`/`delete`; omit during register. |
| `id` | Update/delete | Copy fetched `serviceId`; never use numeric raw `id`. Delete sends only `operation` + `id`. |

### serviceDescription

**A2A**

- Include capability + audience; add signal kind for signal services.
- Preserve supplied text. Optional inputs and delivery/copy-trading notes may use separate,
  optionally numbered lines. Do not invent details or require more structure.
- Recommend ≤2000 display width (CJK=2, ASCII=1), with no per-part limit.

**A2MCP**

- Require total width <2000 (CJK=2, ASCII=1).
- Require purpose, parameter specification, request method, and a runnable request example.
- Store exactly four numbered lines. Use the bracketed headings below or localized equivalents;
  preserve supplied headings/brackets and add missing ones:

  1. `[Service Description]` — purpose.
  2. `[Parameter Spec]` — key parameters on one `;`-separated line, each formatted as
     `name(type, required/optional): meaning`; append optional defaults. Keep key parameters if the
     full list does not fit. Normalize malformed specs before storage.
  3. `[Request Method]` — HTTP verb or bare MCP tool. Strip URL/path text; map an unambiguous
     path-only value to POST.
  4. `[Request Example]` — runnable `curl` using the real `endpoint` and realistic inputs; reject
     placeholder or mismatched hosts.

### serviceGuide

- A2A create accepts optional non-blank text as supplied; CLI validates length.
- Omit on A2MCP create.

### Billing

- Prices are quoted numeric strings (including `"0"`) with ≤6 decimals; no units, symbols, or
  approximations.
- Never combine per-call and monthly billing or use a non-monthly interval.
- An update cannot change billing model. Create a replacement service and optionally delete the old
  one.
- Only a 3-day monthly trial is supported.

| Model | `fee` | `subscription` | `freeTrial` |
|---|---|---|---|
| A2MCP per-call | `"N"` | Omit | Omit |
| A2A per-call | `"N"` | `[]` | Omit |
| A2A monthly | `""` | `[{"interval":"month","fee":"N"}]` | Omit |
| A2A monthly + trial | `""` | `[{"interval":"month","fee":"N"}]` | `"72"` |

### endpoint

- Require a deployed public HTTPS URL of ≤512 characters.
- Reject HTTP, localhost, loopback, RFC-1918, `*.local`, `*.internal`, mocks, and placeholders.

## Collection flow

1. Confirm the exact `serviceType`, then enter the matching A2A or A2MCP workflow.
2. Accept batched answers and collect only missing fields.
3. After each service, ask **1. Add another service / 2. Done** and wait for explicit Done. If the
   user chooses 1, return to `serviceType` collection and repeat the matching workflow.
4. Run validation only after explicit Done.

## A2A workflow

Collect fields in the order below.

### 1. fee, subscription, and freeTrial

1. Ask one numbered billing pick plus its price.

User-facing options:

> Choose a billing model:
> 1. Per call
> 2. Monthly
> 3. Monthly + 3-day trial

2. If the user requests another trial length, explain that only 3 days is supported and re-ask
   option 2/3.
3. Collect the price and encode the selection using [Billing](#billing).

### 2. serviceName and serviceDescription

Ask for the service name and description together.

### 3. serviceGuide

If `serviceGuide` is absent, show the following prompt.

User-facing prompt:

> Describe the prerequisites, steps, and key parameters. For trading, payments, or authorization,
> include confirmation requirements and execution limits.
> [Service Guide Examples](https://web3.okx.com/onchainos/dev-docs/okxai/a2a-subscription).
>
> Send the guide body, or reply 2 to skip.

Accept non-blank text as supplied; `2` omits `serviceGuide`.

## A2MCP workflow

Collect fields in the order below.

### 1. fee

Ask for the fee first.

### 2. serviceName and serviceDescription

1. Ask for the service name and request description together.

User-facing prompt:

> Provide the service name and request description. Format the request description as four numbered
> lines using the required labels and formats.

2. Encode the description using [serviceDescription](#servicedescription).
3. Obtain separate confirmation before storing a normalized parameter spec or converted non-curl
   example.
4. Strip Request Method URL/path text without separate confirmation. Map an unambiguous path-only
   value to POST and show the stored result on the final card.

### 3. endpoint

1. Ask for the endpoint after the fee and request description.
2. Apply [endpoint](#endpoint). If no endpoint is available, require deployment first or let the
   user choose A2A.
3. Explain that an on-chain endpoint change requires update.
4. Confirm the request example against the endpoint.

## Validate

At the register/update QA gate, follow [`validate-listing.md`](validate-listing.md) for timing, input
scope, update-key stripping, semantic merging, and finding resolution.
