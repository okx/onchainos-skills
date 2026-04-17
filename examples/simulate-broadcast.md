# Simulate then Broadcast

## Goal
Reduce failed transactions by simulating before submit.

## Commands

1) Estimate gas / gas limit
```bash
onchainos gateway gas --chain <CHAIN>
onchainos gateway gas-limit --from <FROM> --to <TO> --chain <CHAIN> --data <CALLDATA>
```

2) Simulate
```bash
onchainos gateway simulate --from <FROM> --to <TO> --data <CALLDATA> --chain <CHAIN>
```

3) If simulation passes, sign and broadcast
```bash
onchainos gateway broadcast --signed-tx <SIGNED_TX> --address <FROM> --chain <CHAIN>
onchainos gateway orders --address <FROM> --chain <CHAIN>
```
