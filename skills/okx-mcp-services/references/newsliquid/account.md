# NewsLiquid Account

## Module Description

Log in to the service using the selected OnchainOS EVM wallet. Reuse a valid login for that same wallet. Login output contains metadata only. User-managed clients read the stored Token through the `token` operation in the root-linked CLI reference.

## Flow

### Collect Parameters

1. Obtain the current address from `onchainos wallet addresses --chain xlayer`. If the wallet is not logged in, use the existing wallet login flow.
2. Run login and credential recovery sequentially for the same wallet.

### Execute

1. Run `onchainos service call --service newsliquid --operation login`.
2. A valid same-wallet session is reused. If signing returns Confirming, display its actual message and wait for the user's answer. After approval, run the returned continuation with `confirmWalletWarning=true` and the same `wallet`.
3. Run `onchainos service call --service newsliquid --operation billing-quota` to read the account.
4. On HTTP 401, restore login for the current wallet. Do not treat authentication failure as an instruction to buy. Preserve an interrupted query, but do not replay it if its charging outcome is unknown.

### Handle Result

Service login `userId` and Twitter `userId` are different identifiers.

After a successful login, continue any requested query and apply the notice rules in the root-linked `presentation.md`. Do not call login on ordinary queries merely to repeat the notice.

## Security

### Confirmation

Follow the wallet's actual identity-signature confirmation. Do not add payment authority or automatically override a wallet warning.

### Risk and Stop Conditions

Stop on a wallet/host mismatch, stale challenge, an unreadable credential store or an unsupported response. A switched wallet never inherits the old wallet's credentials, orders, quotas or renewal state.

## Output Rules

Show the current plan/name, `subscription.remaining`, `periodEnd` and `serviceEnd` separately. Show actual `oneTime`, `dailyRefresh` and `legacy` categories. Show `autoRenew` separately, including status, monthly price/currency and `nextChargeAt`. A null category is absent; use quota/used to distinguish a never-held zero category from a held category exhausted to zero.

New-wallet service evidence reports 5 free daily points and no repeat grant on re-login. Daily points do not accumulate. The refresh timezone remains unverified; do not promise a local reset time.

## Troubleshooting

Use the centralized [newsliquid-troubleshooting.md](newsliquid-troubleshooting.md) linked from the root Skill.

## CLI Reference

- [client-cli-reference.md](client-cli-reference.md)
