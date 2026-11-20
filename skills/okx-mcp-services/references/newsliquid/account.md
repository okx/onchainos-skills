# NewsLiquid Account

## Module Description

Log in through the CLI using the selected OnchainOS EVM wallet. Reuse a valid login for that wallet. Request scripts and custom WSS clients retrieve the stored Token through the root-linked CLI reference.

## Flow

### Collect Parameters

1. Obtain the current address from `onchainos wallet addresses --chain xlayer`. If the wallet is not logged in, use the existing wallet login flow.
2. Run login and credential recovery sequentially for the same wallet.

### Execute

1. Run `onchainos service login --service newsliquid`.
2. A valid same-wallet session is reused. If signing returns Confirming, display its actual message and wait for the user's answer. After approval, run the returned continuation with `--confirm-wallet-warning` and the same `--wallet`.
3. Read `GET /billing/quota` directly using the authentication pattern in the root-linked HTTP reference.
4. On an authentication 401, run `onchainos service login --service newsliquid --refresh` for the same wallet, then retrieve the Token again. Preserve any wallet-warning continuation. Do not treat quota or plan-right failures as login failures or payment consent. Do not replay a query whose charging outcome is unknown.

### Handle Result

Service login `userId` and Twitter `userId` are different identifiers.

After a successful login, continue any requested query and apply the notice rules in the root-linked `presentation.md`. Do not call login on ordinary queries merely to repeat the notice.

## Security

### Confirmation

Follow the wallet's actual identity-signature confirmation. Do not add payment authority or automatically override a wallet warning.

### Risk and Stop Conditions

Stop on a wallet/host mismatch, stale challenge, an unreadable credential store or an unsupported response. A switched wallet never inherits the old wallet's credentials, orders, quotas or renewal state.

## Output Rules

- Show plan/name, `subscription.remaining`, `periodEnd` and `serviceEnd` separately.
- Show actual `oneTime`, `dailyRefresh` and `legacy` categories.
- Show `autoRenew` separately: status, monthly price/currency and `nextChargeAt`.
- Omit null categories. Use quota/used to distinguish never-held zero categories from held categories exhausted to zero.

New-wallet service evidence reports 5 free daily points and no repeat grant on re-login. Daily points do not accumulate. The refresh timezone remains unverified; do not promise a local reset time.

## Troubleshooting

Use the centralized [newsliquid-troubleshooting.md](newsliquid-troubleshooting.md) linked from the root Skill.

## CLI Reference

- [service-cli-reference.md](../service-cli-reference.md)
