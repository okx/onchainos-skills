# SUI CLI Reference

Use `--readable-amount` for human-readable quantities. `sui` is the canonical chain value and `784` is accepted.

## Assets And Address

```bash
onchainos wallet addresses --chain sui
onchainos wallet balance --chain sui [--force]
onchainos wallet balance --chain sui --token-address <coin-type> [--force]
```

Balance results show total assets and value. Transfer preparation obtains the service-prepared transaction, including returned Coin Objects and Gas, from `unsignedInfo`.

## Transfers

```bash
onchainos wallet send --chain sui --recipient <address> --readable-amount <amount>
onchainos wallet send --chain sui --contract-token <coin-type> --recipient <address> --readable-amount <amount>
```

SUI transfer uses the same external CLI interaction as ordinary `wallet send`. The CLI keeps the service-prepared transaction authoritative and owns Coin Object selection, Gas, signing, and ordinary single-transaction broadcast. Do not reproduce those steps in the Skill. It does not add a SUI-specific preview, balance precheck, or second preparation stage.

The initial command signs and returns the ordinary `confirming` response before broadcast. The Agent **MUST** display its complete confirmation and wait for a new explicit user confirmation before executing `next`. The confirmed continuation broadcasts and returns `state=PENDING`, `txHash`, and `orderId` when supplied by the service.

## History And Status

SUI uses the same wallet history request and response mapping as other supported chains.

```bash
onchainos wallet history --chain sui [--page-num <cursor> --limit <n>]
onchainos wallet history --chain sui (--tx-hash <hash> | --order-id <id>)
```

Status results contain the shared filtered history fields, including the normalized `txStatus` and returned failure reason when present.
