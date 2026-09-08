# Capability: Trenches — Troubleshooting

Error handling, edge cases, and region restrictions for the Trenches capability. Load operational flow and parameter schemas separately through the top-level conditional-reference table.

## Edge Cases

- **Unsupported chain for meme pump**: only Solana (501), BSC (56), X Layer (196), TRON (195) are supported — verify with `onchainos memepump chains` first
- **Invalid stage**: must be exactly `NEW`, `MIGRATING`, or `MIGRATED`
- **Token not found in meme pump**: `memepump-token-details` returns null data if the token doesn't exist in meme pump ranking data — it may be on a standard DEX
- **No dev holding info**: `memepump-token-dev-info` returns `devHoldingInfo` as `null` if the creator address is unavailable
- **Empty similar tokens**: `memepump-similar-tokens` may return empty array if no similar tokens are found
- **Empty aped wallets**: `memepump-aped-wallet` returns empty array if no co-holders found

## Region Restrictions (IP Blocking)

When a command fails with error code `50125` or `80001`, display:

> DEX is not available in your region. Please switch to a supported region and try again.

Do not expose raw error codes or internal error messages to the user.

## Protocol ID Fallback

Use this table only when `onchainos memepump chains` is unavailable. Prefer the command because the supported list can change.

| Chain | Protocol | ID |
|---|---|---|
| Solana | pumpfun | `120596` |
| Solana | bonk | `136266` |
| Solana | bonkers | `139661` |
| Solana | jupStudio | `137346` |
| Solana | believe | `134788` |
| Solana | bags | `129813` |
| Solana | moonshotMoney | `133933` |
| Solana | launchlab | `136137` |
| Solana | moonshot | `121201` |
| Solana | meteoradbc | `136460` |
| Solana | mayhem | `139048` |
| BNB Chain | fourmeme | `135086` |
| BNB Chain | flap | `129826` |
| Base | clanker | `130981` |
| Base | bankr | `134522` |
| X Layer | dyorfun | `137823` |
| X Layer | flap | `129826` |
| TRON | sunpump | `121263` |
