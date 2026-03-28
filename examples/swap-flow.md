# Swap Flow (Quote -> Execute -> Broadcast)

## Goal
Perform a standard token swap with explicit confirmation points.

## Commands

1) Quote
```bash
onchainos swap quote --from <FROM_TOKEN> --to <TO_TOKEN> --amount <MINIMAL_UNITS> --chain <CHAIN>
```

2) Approve (EVM ERC-20 only)
```bash
onchainos swap approve --token <FROM_TOKEN> --amount <MINIMAL_UNITS> --chain <CHAIN>
```

3) Build swap tx
```bash
onchainos swap swap --from <FROM_TOKEN> --to <TO_TOKEN> --amount <MINIMAL_UNITS> --chain <CHAIN> --wallet <WALLET_ADDRESS> --slippage 1
```

4) Sign locally, then broadcast
```bash
onchainos gateway broadcast --signed-tx <SIGNED_TX> --address <WALLET_ADDRESS> --chain <CHAIN>
onchainos gateway orders --address <WALLET_ADDRESS> --chain <CHAIN>
```
