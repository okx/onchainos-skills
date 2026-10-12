# Capability: Token — Troubleshooting

## Recovery

Error handling, edge cases, and region restrictions for the Token capability. For normal flow, return to [token.md](token.md); read [token-cli-reference.md](token-cli-reference.md) only when command facts are needed.

- **Token not found**: suggest verifying the contract address (symbols can collide)
- **Same symbol on multiple chains**: show all matches with chain names
- **Too many results**: name/symbol search caps at 100 — suggest using exact contract address
- **Network error**: retry once
- **Region restriction (error code 50125 or 80001)**: do NOT show the raw error code to the user. Instead, display a friendly message: `⚠️ Service is not available in your region. Please switch to a supported region and try again.`
