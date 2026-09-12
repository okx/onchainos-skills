# Identity service contract

Use this contract for ASP service fields, payloads, and collection.

## Service fields

### Shared rules

The case-sensitive `--service` element is shared across create, update, and `validate-listing`.
Trim text values and use exact camelCase keys.

### serviceType

- Require the exact value `A2MCP` or `A2A`.

#### Service Information Collection Guide

Immediately below the service type choices, show:

> Not sure how to proceed? See the [Developer Documentation](https://web3.okx.com/onchainos/dev-docs/okxai/asp) for detailed instructions.

### serviceName

- Require a 5–30 character noun phrase.
- Require it to differ from the agent name.
- Do not include a price.

### serviceDescription

#### Value requirements

- Required. It describes the service inside `--service`; never use the Agent profile's top-level `--description` as its value.
- Require total width <2000. Measure display width as CJK=2 and ASCII=1.

A2A differences:

- Include capability and audience; signal services also include signal kind.
- Preserve supplied text and do not invent details or impose extra structure. Optional inputs and delivery/copy-trading notes may use separate, optionally numbered lines.

A2MCP differences:

- Store exactly four numbered lines. Preserve supplied headings/brackets and add missing ones, using these headings or localized equivalents:

  1. `[Service Description]` — purpose.
  2. `[Parameter Spec]` — `;`-separated key parameters formatted as `name(type, required/optional): meaning`, including optional defaults. Keep only key parameters if needed to fit; normalize malformed specs.
  3. `[Request Method]` — HTTP request method only, such as `GET`, `POST`, `PUT`, or `DELETE`. Remove URL/path text; map an unambiguous path-only value to `POST`.
  4. `[Request Example]` — runnable `curl` with the real `endpoint` and realistic inputs; reject placeholders and mismatched hosts.

#### Collection behavior

- For A2MCP, provide the following concise field guide:

  > Provide the service name (5–30 characters, different from the agent name, without a price), a deployed public HTTPS endpoint, and exactly these four description lines:
  > `1. [Service Description]` What the service does and what result it returns.
  > `2. [Parameter Spec]` Key parameters as `name(type, required/optional): meaning`, separated by `;`.
  > `3. [Request Method]` HTTP request method only, such as `GET` or `POST`; do not include a URL/path.
  > `4. [Request Example]` Runnable `curl` using the real endpoint and realistic values; no placeholders.

  Normalize malformed parameter specs and non-curl examples before storing. The endpoint must match the curl example.

### serviceGuide

#### Value requirements

- A2A create accepts optional non-blank text as supplied; CLI validates length.
- Omit on A2MCP create.

#### Collection behavior

If `serviceGuide` is absent for A2A create, show the following prompt:

> Describe the prerequisites, steps, and key parameters. For trading, payments, or authorization, include confirmation requirements and execution limits.
> [Service Guide Examples](https://web3.okx.com/onchainos/dev-docs/okxai/a2a-subscription).
>
> Send the guide body, or reply 2 to skip.

### fee

#### Value requirements

- `fee` is required.
- Prices are quoted numeric strings (including `"0"`) with no units, symbols, or approximations.
- A2A prices have ≤2 decimals; A2MCP prices have ≤6 decimals.
- Never combine per-call and monthly billing or use a non-monthly interval.
- Store the per-call price in `fee`; for A2A monthly billing, use `""`.

#### Collection behavior

- For A2A, ask the user to choose a billing model and provide its price:

  > Choose a billing model:
  > 1. Per call
  > 2. Monthly
  > 3. Monthly + 3-day trial
- Derive `fee`, `subscription`, and `freeTrial` from the selected model and price; do not ask the user for raw encoded values.
- If the user requests another trial length, explain the restriction above and re-ask options 2/3.

### subscription

- Required for A2A; omit for A2MCP.
- For A2A per-call billing, use `[]`.
- For A2A monthly billing, use `[{"interval":"month","fee":"N"}]`, where `N` is greater than 0, and set `fee` to `""`.

### freeTrial

- Only a 3-day monthly trial is supported.
- For A2A monthly billing with a trial, use `"72"`.
- Otherwise omit `freeTrial`.

### endpoint

#### Value requirements

- For A2MCP, require a deployed public HTTPS URL of ≤512 characters.
- Reject HTTP, localhost, loopback, RFC-1918, `*.local`, `*.internal`, mocks, and placeholders.

#### Collection behavior

- If no endpoint is available, require deployment first or let the user choose A2A.
- Explain that an on-chain endpoint change requires update.
- Confirm that the request example uses the endpoint.

### operation

- Use `operation` only during update, with the exact value `create`, `update`, or `delete`; omit it during register.

### id

- For update and delete, use the matched record's `id`.
- For delete, send only `operation` and `id`.
