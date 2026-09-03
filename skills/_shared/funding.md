# Funding and Receive

Business Reference for active wallet funding and for the shared address/QR flow
entered after another business asks whether the user wants to fund.

Before handling a returned action, read
[funding-action-routing.md](funding-action-routing.md). Render CLI results with
[funding-output-templates.md](funding-output-templates.md).

## Applies when

- The user asks to deposit, top up, receive a token, show a receive address, or
  show its QR code.
- The latest structured result contains `nextAction.id=fund_account` and the
  user explicitly chooses funding.

An upstream business owns its insufficient-balance message and first asks
whether the user wants to fund. Do not show the shared funding address/QR before
that choice.

## Active funding flow

1. Determine whether the current user input supplies a chain, a token, or
   neither.
2. Invoke exactly one read-only CLI entry:

   | Confirmed input | CLI call |
   | --- | --- |
   | Neither chain nor token | `onchainos wallet receive` |
   | Chain | `onchainos wallet receive --chain <chain>` |
   | Token without chain | `onchainos wallet receive --token <query>` |

3. Read `decision`, then `reason`, `nextAction`, and `payload` from the result.
4. Render the matching Funding Output Template.
5. Route a user-selected action only through Funding Action Routing.

When both chain and token are explicit, the chain determines the receive
address. Use the chain call and do not add token metadata that the CLI result
did not return.

## Token selection constraints

- Preserve the CLI order and the full `chainIndex + tokenContractAddress`
  identity from the latest result.
- A numeric reply can select only the corresponding `select_receive_token`
  action from that result.
- A “More” reply can execute only the current
  `more_receive_tokens.params.command`.
- If the result is stale, missing, or incomplete, restart Token Search. Never
  reconstruct a candidate from symbol, name, an abbreviated address, or prose.

## Insufficient-balance entry

Validate the current `fund_account` action and these common payload fields:

- `fundingTarget`: account, network, full receive address, same-network and gas
  facts.
- `qr`: QR representation for that exact address.
- `fundingNeed`: asset, token address, required amount, current balance, and
  CLI-calculated shortfall.

Render the shared funding target from the latest payload and wait. Do not query
another address, reconstruct a QR, or calculate a missing amount in the Skill.

When the user later says they funded the account:

1. Use only `payload.fundingTarget.chainIndex` and
   `payload.fundingNeed.{tokenAddress,required,asset}` from the latest structured
   result. If any required fact is unavailable, ask for it or restart the
   funding query; never guess.
2. Run:
   `onchainos wallet funding-check --chain <chainIndex> [--token-address <tokenAddress>] --required <required> --asset <asset>`.
   Omit `--token-address` only when the structured value is empty for a native
   asset.
3. Render Post-funding balance verification from
   [funding-output-templates.md](funding-output-templates.md).
4. If sufficient, infer the interrupted operation only from the current
   conversation context. When it is clear, ask whether the user wants to
   continue it. When it is not clear, use the generic continuation prompt from
   the template. This is a plain-language handoff, not a CLI action.
5. A request to continue is a new entry into the owning business Reference. It
   must re-query/re-preview current facts and obtain every confirmation required
   by that business. Never reuse an old preview, quote, payment ID, write
   command, or confirmation.

If the balance remains insufficient, retain the new verification result and
wait for another funding-complete event. A funding event authorizes no transfer,
swap, payment, signature, task creation, or broadcast.

## Address and QR constraints

- Display the full receive address byte-for-byte from the latest CLI result.
- Common QR encodes only those bare address bytes.
- Address and QR are one block; the QR immediately follows its address.
- QR failure degrades to the address; do not claim a QR was produced.
- Consume `sameNetworkRequired` and `gasFree` only as structured facts.
