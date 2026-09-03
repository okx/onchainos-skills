# Funding Action Routing

Deterministic registry for actions owned by the shared Funding Reference. Route
only an ID present in the latest CLI result's `nextAction` array.

| Action ID | Required structured facts | Deterministic route |
| --- | --- | --- |
| `specify_funding_chain` | `phase=funding`; user-supplied chain | `funding.md` active-funding flow → `wallet receive --chain <chain>` |
| `search_receive_token` | `phase=funding`; user-supplied token query | `funding.md` active-funding flow → `wallet receive --token <query>` |
| `select_receive_token` | `phase=funding`; action params `sequence`, `chainIndex`, `tokenContractAddress` | `funding.md` token-selection flow → `wallet receive --chain <chainIndex>` |
| `more_receive_tokens` | `phase=funding`; action params `query`, `cursor`, `command` | Execute `params.command` exactly, then re-enter `funding.md` |
| `fund_account` | empty action params; `payload.fundingTarget`, `payload.qr`, `payload.fundingNeed` | `funding.md` insufficient-balance entry |

Rules:

- `fund_account` has one shared contract for every current and future business;
  a business does not register another phase-specific route.
- Validate the required structured facts before routing.
- Preserve returned action order; `recommend=true` changes presentation only.
- A numeric reply selects only the matching action from the latest rendered
  result.
- Unknown, stale, or incomplete actions stop with an unsupported-continuation
  result. Do not infer a route from labels or prose.
- `fund_account` authorizes only entry into Funding. It never authorizes the
  interrupted business operation or any write.
