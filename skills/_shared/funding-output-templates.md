# Funding Output Templates

Render only the latest structured CLI result. These templates own presentation;
they do not define commands, routing, retries, confirmations, or state changes.

## Common rendering rules

Use the user's language and this fixed order, without section headings such as
`Result`, `Details`, or `Next` (including localized variants):

```text
<one-sentence result>

<fields required by the matched template>

<optional labels for the returned nextAction items, in order, only when the
matched template includes them>
```

Action labels are presentation-only:

| `nextAction.id` | English label | Chinese label |
| --- | --- | --- |
| `specify_funding_chain` | Specify network | 指定网络 |
| `search_receive_token` | Search token | 搜索 Token |
| `select_receive_token` | Select option `{params.sequence}` | 选择第 `{params.sequence}` 项 |
| `more_receive_tokens` | More results | 更多结果 |
| `fund_account` | Fund account | 充值 |
| `open_create_playbook` | Continue task creation | 继续创建任务 |

When the matched template includes actions, render only actions present in
`nextAction`, preserving their order. If the array is empty, render a localized
non-action status such as “No further CLI action is available.” Do not add
Cancel, Confirm, Retry, or Continue unless the CLI returned that action. A
template may intentionally omit action labels while retaining `nextAction` for
machine routing.

Never expose raw JSON, internal commands, opaque cursors, account IDs, internal
phase names, or provider text.

## Shared funding target fields

Match an active receive result using its direct payload fields. For a routed
`fund_account`, use `payload.fundingTarget` and `payload.qr` from the same latest
structured result.

```text
Account: {payload.accountName}
Network: {payload.chainName}
Receive address: {payload receive-address field}
{QR selected from payload.qr.displayMode}
```

Field and omission rules:

- Omit Account when `payload.accountName` is absent or empty.
- Always display the complete receive address verbatim.
- For `terminal-unicode`, render `payload.qr.terminalQr` verbatim.
- For `image-notify`, render `payload.qr.markdownImage` or the runtime image
  represented by `imagePath` and `notifyCommandArgs`.
- A QR must appear immediately after the address it encodes, with no other
  address, field, or explanatory sentence between them.
- If QR output is absent, retain the address and omit the QR line.
- When `payload.sameNetworkRequired=true`, append the localized same-network
  warning using `payload.chainName`.
- When `payload.gasFree=true`, append the localized X Layer gas-sponsored note.

The same QR field mapping applies to `payload.evmQr` in the generic receive
template; no other generic address has a QR input.

## Generic receive addresses

Match:

```text
phase=funding
decision=ready
reason=receive_addresses_ready
```

```text
The receive addresses for the current account {payload.accountName} are ready. {optional concise unresolved-request context}

EVM: {payload.evmAddress}
{QR selected from payload.evmQr.displayMode}
X Layer: {payload.xLayerAddress}
Solana: {payload.solanaAddress}
Bitcoin: {payload.bitcoinAddress}
Sui: {payload.suiAddress}
```

Omit every absent address line. Render only `payload.evmQr`; do not construct QR
fields for any other generic address. The EVM QR must be the element immediately
after the EVM address and before X Layer, Solana, Bitcoin, or Sui.

When the current user input supplies an exact amount/asset phrase but still
leaves the stablecoin and network unresolved, preserve that phrase verbatim in
the first sentence; for example, Chinese input `1 U` renders as “要充值 1 U，还需
指定稳定币和网络。” Never infer, normalize, or reconstruct an amount from
conversation memory. If the current input has no exact phrase, omit that clause.

End this template immediately after the final available address. Do not append
action labels, reply examples, instructions about external wallets or
exchanges, or session/tool narration.

## Chain or selected-token receive address

Match:

```text
phase=funding
decision=ready
reason=funding_target_ready
```

```text
The {payload.chainName} receive address for the current account {payload.accountName} is ready.

Token: {payload.tokenName} ({payload.tokenSymbol})
Contract: {payload.tokenContractAddress}
Receive address: {payload.receiveAddress}
{QR selected from payload.qr.displayMode}
```

Omit Token when both token name and symbol are absent. Omit Contract for a
native asset or an absent contract address. Omit the account name when absent.
The QR must immediately follow the receive address. End after the QR, except
for same-network or gas-sponsored notices backed by the corresponding payload
flags. Do not render action labels or a no-action status.

## Token selection

Match:

```text
phase=funding
decision=requires_user_input
reason=token_selection_required
```

```text
Multiple tokens match “{payload.query}”.

{sequence}. {tokenName} | {networkName} | {contractAddressDisplay}

{rendered nextAction labels in returned order}
```

Render each `payload.list` item in order. Use `contractAddressDisplay` for the
compact list and never reconstruct a full contract address from it.

## Token not found

Match:

```text
phase=funding
decision=blocked
reason=token_not_found
```

```text
No matching token was found for “{payload.query}”.

No token candidate is available.

{rendered nextAction labels or localized no-action status}
```

## Funding target after a funding intent

This shared template starts only after either:

- the user directly asks to fund/receive; or
- a business flow returned `fund_account` and the user explicitly selects the
  funding action (for example, replies “充值”).

It does not render the business flow's preceding insufficient-balance result.
That result belongs to the owning Transfer, Swap, A2A payment, or Task output
template.

Match the routed action:

```text
selectedAction.id=fund_account
payload.fundingTarget is present
payload.qr is present
payload.fundingNeed is present
```

```text
The {payload.fundingTarget.chainName} receive address for the current account is ready.
Receive address: {payload.fundingTarget.receiveAddress}
{QR selected from payload.qr.displayMode}
```

Append the same-network warning only when
`payload.fundingTarget.sameNetworkRequired=true`. Address and QR
are one display block: the QR must immediately follow the address. If QR output
is unavailable, keep the address and show a localized QR-unavailable state in
that block. This routed view creates and renders no new action.

## Post-funding balance verification

Match after the user reports funding complete and the Funding Reference has
queried the real balance:

```text
phase=funding_verification
reason=funding_sufficient|insufficient_balance|balance_unavailable
```

When sufficient:

```text
The latest {payload.asset.symbol} balance is {payload.currentBalance} {payload.asset.symbol}; the required {payload.required} {payload.asset.symbol} is available.

{continuation prompt selected below}
```

When still insufficient:

```text
The latest {payload.asset.symbol} balance is {payload.currentBalance} {payload.asset.symbol}; {payload.shortfall} {payload.asset.symbol} is still required.

The {payload.chainName} receive address for the current account is ready.
Receive address: {payload.fundingTarget.receiveAddress}
{QR selected from payload.qr.displayMode}
```

Balance is mandatory. A failed balance query must return and render an explicit
blocked/unavailable result instead of omitting it. Shortfall comes only from the
CLI. Address and QR remain one block. Do not show a quote.

For a sufficient result, the Funding Reference chooses exactly one continuation
prompt from conversation context:

- If the interrupted operation is clear, ask whether to continue that operation
  in the user's language.
- If it is not clear, render the localized equivalent of: `You can continue the
  operation you were doing before funding.` Chinese: `您可以继续充值前的操作。`

This is plain-language handoff copy, not an action label or authorization. Do
not number it and do not synthesize a `nextAction`.
