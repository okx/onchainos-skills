# SUI Agentic Wallet

Use this flow for SUI addresses, assets, and transfers. SUI transfer follows ordinary `wallet send`; the service remains authoritative for Coin Object selection, Gas, and pre-execution.

## Flow

1. Resolve one SUI intent and collect the parameters in the routing table. Use `sui` as the `--chain` value and a complete Coin Type for `Coin<T>`.
2. Invoke the matching CLI command. The current account supplies the SUI sender address when transfer `--from` is absent.
3. Relay `data.message` or `error` with the relevant returned facts.
4. A SUI transfer first signs and returns `confirming:true` before broadcast. **MUST** show `message` and the complete `preview`, then stop and wait for a new explicit user confirmation. Execute `next` only after that confirmation. A cancellation ends that operation.
5. Execute `nextSteps` when it is the returned immediate or scheduled read continuation. Each transfer begins from an explicit user intent and receives its own confirmation.

For a SUI transfer confirmation, the CLI has already signed and has not broadcast. **MUST** render the complete preview:

`Transfer ready to broadcast`

`From: ${preview.from}`

`To: ${preview.to}`

`Amount: ${preview.asset.readableAmount} ${preview.asset.symbol}`

Render `Network fee: ${preview.feeReadable} ${preview.feeSymbol}` only when `preview.feeReadable` is present.

## Intent Routing

| Intent | Parameters | Command |
| --- | --- | --- |
| SUI address | none | `wallet addresses --chain sui` |
| SUI assets and value | optional refresh | `wallet balance --chain sui` |
| Coin<T> assets and value | complete Coin Type | `wallet balance --chain sui --token-address <coin-type>` |
| Native SUI transfer | recipient, readable amount | `wallet send --chain sui` |
| Coin<T> transfer | Coin Type, recipient, readable amount | `wallet send --chain sui --contract-token <coin-type>` |

## Conversation Context

- Keep addresses, Coin Types, tx hashes, order IDs, balance facts, and prior intent within the current conversation.
- Resolve a unique current-conversation reference such as “that transaction” to its identifier. Ask for the identifier when multiple candidates exist or a new conversation begins.
- After account switching, login changes, environment changes, or address changes, query the required facts again.
- A changed recipient, amount, Coin Type, message, or account starts a fresh command and confirmation.

## Additional Resources

- Full parameter tables, return-field schemas, and worked examples → [sui-cli-reference.md](sui-cli-reference.md). Load only when the flow above does not provide the required exact syntax or fields.
- Load on error → [sui-troubleshooting.md](sui-troubleshooting.md).
