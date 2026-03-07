# Portfolio Overview (Read-only)

## Goal
Check total portfolio value and token balances across selected chains.

## Commands

```bash
onchainos portfolio total-value --address <WALLET_ADDRESS> --chains "xlayer,solana,ethereum"
onchainos portfolio all-balances --address <WALLET_ADDRESS> --chains "xlayer,solana,ethereum"
```

## Optional follow-up

```bash
onchainos token price-info <TOKEN_ADDRESS> --chain <CHAIN>
onchainos market kline <TOKEN_ADDRESS> --chain <CHAIN> --bar 1H --limit 24
```
