# Identity service contract

Use this contract for ASP service fields, payloads, and collection. Follow [`update.md`](update.md)
for updates, [`validate.md`](validate.md) for validation, and
[`output-templates.md`](output-templates.md) for display.

## Fields and payload

Create, update, and `validate-listing` share the case-sensitive `--service` element. Trim text
values and use exact camelCase keys. Never interchange the Agent profile's top-level
`--description` with a service's `serviceDescription`.

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

**A2A:** Include capability + audience, plus signal kind for signal services. Preserve supplied
text; do not invent details or impose more structure. Optional inputs and delivery/copy-trading
notes may use separate, optionally numbered lines. Recommend ≤2000 display width (CJK=2,
ASCII=1), with no per-part limit.

**A2MCP:** Require total width <2000 (CJK=2, ASCII=1) and exactly four numbered lines. Use these
bracketed headings or localized equivalents; preserve supplied headings/brackets and add missing
ones:

  1. `[Service Description]` — purpose.
  2. `[Parameter Spec]` — key parameters on one `;`-separated line, each formatted as
     `name(type, required/optional): meaning`; append optional defaults. Keep key parameters if the
     full list does not fit. Normalize malformed specs before storage.
  3. `[Request Method]` — HTTP verb or bare MCP tool. Strip URL/path text; map an unambiguous
     path-only value to POST.
  4. `[Request Example]` — runnable `curl` using the real `endpoint` and realistic inputs; reject
     placeholder or mismatched hosts.

### serviceGuide

On A2A create, accept optional non-blank text as supplied and leave length validation to the CLI.
Omit it on A2MCP create.

### Billing

Use quoted numeric price strings (including `"0"`) with ≤6 decimals and no units, symbols, or
approximations. Never combine per-call and monthly billing or use a non-monthly interval. Updates
cannot change billing model; create a replacement service and optionally delete the old one. Only
a 3-day monthly trial is supported.

| Model | `fee` | `subscription` | `freeTrial` |
|---|---|---|---|
| A2MCP per-call | `"N"` | Omit | Omit |
| A2A per-call | `"N"` | `[]` | Omit |
| A2A monthly | `""` | `[{"interval":"month","fee":"N"}]` | Omit |
| A2A monthly + trial | `""` | `[{"interval":"month","fee":"N"}]` | `"72"` |

### endpoint

Require a deployed public HTTPS URL of ≤512 characters. Reject HTTP, localhost, loopback,
RFC-1918, `*.local`, `*.internal`, mocks, and placeholders.

## Collection flow

1. Confirm the exact `serviceType`; enter its A2A or A2MCP workflow.
2. Accept batched answers and collect only missing fields.
3. After each service, ask **1. Add another service / 2. Done**. Wait for explicit Done; on 1,
   collect `serviceType` again and repeat its workflow.
4. Run validation only after explicit Done.

## A2A workflow

Collect fields in the order below.

### 1. fee, subscription, and freeTrial

Ask one numbered billing pick plus its price:

> Choose a billing model:
> 1. Per call
> 2. Monthly
> 3. Monthly + 3-day trial

For another trial length, explain that only 3 days is supported and re-ask option 2/3. Encode the
selection using [Billing](#billing).

### 2. serviceName and serviceDescription

Ask for the service name and description together.

### 3. serviceGuide

If `serviceGuide` is absent, show:

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

Ask for the service name and request description together:

> Provide the service name and request description. Format the request description as four numbered
> lines using the required labels and formats.

Encode it using [serviceDescription](#servicedescription). Obtain separate confirmation before
storing a normalized parameter spec or converted non-curl example. Without separate confirmation,
strip Request Method URL/path text, map an unambiguous path-only value to POST, and show the stored
result on the final card.

### 3. endpoint

After the fee and request description, ask for the endpoint and apply [endpoint](#endpoint). If none
is available, require deployment first or let the user choose A2A. Explain that an on-chain
endpoint change requires update. Confirm the request example against the endpoint.

## Validate

At the register/update QA gate, follow [`validate.md`](validate.md) for timing, input scope,
update-key stripping, semantic merging, and finding resolution.
