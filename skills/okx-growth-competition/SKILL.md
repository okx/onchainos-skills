---
name: okx-growth-competition
description: "Agentic Wallet exclusive trading competitions. Full lifecycle: discover → view rules → join → trade → check rank → claim reward. Triggers: 'list trading competitions', 'show available competitions', 'view competition details', 'show competition rules', 'show prize pool', 'register for competition', 'join trading contest', 'check my competition status', 'view leaderboard', 'check my ranking', 'claim competition reward', 'did I win', 'winners list', 'show registered wallet', 'export wallet'. Do NOT use for: general DEX swaps (use okx-dex-swap); portfolio / PnL queries outside a competition (use okx-wallet-portfolio or okx-dex-market); wallet login or tx history (use okx-agentic-wallet); any non-competition trading activity. Do NOT use when the user says only a single word like 'competition', 'rank', or 'claim' without naming a specific competition."
license: MIT
metadata:
  author: okx
  version: "1.2.0"
  homepage: "https://web3.okx.com"
---

# OKX Growth Competition — Trading Competition

Agentic Wallet exclusive trading competitions. Full lifecycle: list → detail → join → trade → rank → claim.

CLI reference: `references/cli-reference.md`

## Facts about every Agentic Wallet competition (always true unless backend confirms otherwise)

<MUST>
Treat the following as **factual ground truth** when the user asks about how a competition works. Every activity exposes two chain-related fields, and both contribute to the trading set:

- `chainId` — single id. **A trading chain AND the claim/reward chain** (rewards are paid on this chain; its contract address is here).
- `participateChainIds` — array of ids returned by **both `list` and `detail`** endpoints (may be empty on activities created before the field was added). **Additional trading chains** (no claim role).

The **full trading-chain set** for an activity is the **UNION**: `{chainId} ∪ participateChainIds`. Trades on any chain in that union count toward the same competition standing. The **claim chain** is `chainId` only.
</MUST>

1. **The trading-chain set is `{chainId} ∪ participateChainIds` (dedup).** Render each id as a human-readable name via the canonical `Chain id → display name` table. Currently supported competition chains: `1 → Ethereum`, `196 → X Layer`, `501 → Solana`.
2. **Trades on any chain in the union count toward the same competition standing.** Trades on chains NOT in the union do not count. Never tell a user "your chain doesn't count" without first checking the union.
3. `myRankInfo.userTotal = 0` means the user has not yet hit the qualifying threshold or the backend metric pipeline has not picked up their trades yet — it does **NOT** mean the user's chain is unsupported.
4. `competition_rank` takes a single optional `wallet`. Omit it for self-rank — the tool auto-resolves the chain-appropriate address from the active account based on `competition_detail.chainId` (the primary chain used for reward claim and rank indexing). Pass an explicit address ONLY when querying someone else's rank; the address chain (EVM `0x...` else Solana) must match the activity's primary chain or the tool rejects the call (no silent wrong-chain queries). The chain you query on is just a lens — trades on every other chain in the union still count toward the same ranking.
5. **Claim path uses only `chainId`** (that's where the reward contract / activity address lives). Everything else — trading eligibility, "which chains count", chain-list display in templates — uses the union of both fields.

When the user asks "Does my chain count for this competition?" or "Which chains can I trade on?", answer from the **union** of `chainId` and `participateChainIds`. When rendering the "Chain" column / line in a template, render that same union (deduplicated) — see the `{supportedChains}` computation rule in each Step's field-mapping section.

## Identity resolution invariant (deterministic — always answerable)

<MUST>
The query identity for `competition_rank` and `competition_user_status` is **mutually exclusive**: the backend accepts EITHER `accountId` (self-query, multi-chain by design) OR `walletAddress` (cross-user query on a specific chain) — never both, never neither. When the user asks "which identity did you use?", the answer is **deterministic from the call shape** — never reply with "both", "unknown", or "I'm not sure".
</MUST>

**Algorithm (always runs internally; AI cannot bypass):**

| Tool / call shape | Identity sent | Why |
|---|---|---|
| `competition_user_status` (always) | `accountId` (self) | self-only tool; covers every chain in `participateChainIds` in one call |
| `competition_rank` with no `wallet` | `accountId` (self) | default self-rank, no chain picking needed |
| `competition_rank` with `wallet=<addr>` | `walletAddress` (cross-user) | the tool validates that `addr`'s chain family (EVM `0x...` else Solana) matches the activity's primary `chainId`; mismatch → call is rejected (no silent wrong-chain query) |
| `competition_claim` (pre-check) | `accountId` (self) | claim is always self-action; uses accountId for the eligibility check |

**How to answer "which identity did you use?":**

| Call | What to say (translate to user's language) |
|---|---|
| Self-query (no `wallet` arg) | "I used your `accountId` — the backend looks up your status across every chain in this competition (`participateChainIds`) in one call." |
| Cross-user query (`wallet` arg passed) | "I used the wallet address `<addr>` you specified — it was validated to match the activity's primary chain before the call." |

**Forbidden answers** (paraphrase patterns observed and are wrong):

- ❌ "The tool sends both `accountId` and `walletAddress`" — Wrong. Exactly one, never both.
- ❌ "The tool picks EVM or SOL based on chain" — Wrong (post-refactor). That was the old wallet-only model. Self-queries now send `accountId`, no chain pick.
- ❌ "I'm not sure which one the tool ended up sending" — Wrong. It is deterministic from the call shape; always answerable from the above table.

For multi-activity queries (e.g. `competition_user_status` with no `activity_name`), the same `accountId` is reused for every activity in the iteration — the backend joins by accountId, not address. The answer is uniformly "I used your accountId for all activities".

## ⚠️ Mandatory reading order

<MUST>
**Before producing ANY user-facing message about a competition (list / detail / join / claim / rank / status / wallet-export-guard), you MUST first locate the matching `Step N` section below and follow its fixed template structure.** Do NOT improvise the format. Do NOT shorten the templates. Do NOT drop sections or merge them.

The template **structure is fixed**; the **language follows the user** — see the `## Output Language` rule above. When the user writes Chinese, translate the template strings to natural Chinese. When the user writes English, use English as written. Placeholders (including chain display names from `{supportedChains}`) stay as-is.

Quick router (user intent → template section):

- "list competitions / show available competitions" → **Step 1** (table, optionally split by Active / Ended)
- "show details / show rules / show prize pool" → **Step 2** (Basic info block + 4 reward sections, with `{supportedChains}` chain line and required participation / Skill copy)
- "register / join" → **Step 3** (registration success fixed template + disclaimer)
- "trade for me" → **Step 4** (delegate to okx-dex-swap)
- "leaderboard / ranking" → **Step 5**
- "claim reward" → **Step 6** (use `competition_claim` MCP, atomic)
- "show registered wallet" → Additional Flows / Query Registered Wallet
- "export wallet" → Additional Flows / Wallet Export Guard

If the user's intent does not clearly map to one of the above, ask which they meant before responding — do **not** invent a freeform format.
</MUST>

## Pre-flight

> Read `../okx-agentic-wallet/_shared/preflight.md`. If missing, read `_shared/preflight.md`.

## Command Index

| # | Command | Auth | Description |
|---|---------|------|-------------|
| 1 | `onchainos competition list [--status 0\|1\|2] [--page-size N] [--page-num N]` | None | List Agentic Wallet exclusive competitions (default status=0, active only) |
| 2 | `onchainos competition detail --activity-id <id>` | None | Get rules, prize pool, chain, timeline |
| 3 | `onchainos competition rank --activity-id <id> [--wallet <addr>] --sort-type <type> [--limit N]` | None | Leaderboard + user rank. Omit `--wallet` to auto-resolve from the active account; the command fetches `competition_detail.chainId` and picks the chain-appropriate address. Pass `--wallet` ONLY to query someone else's rank — the address chain must match the activity chain or the call is rejected. MCP tool `competition_rank` mirrors this (single optional `wallet`). Discover available `sort-type` values from `competition_detail` → `tabConfigs[].rankFieldConfig[].sortValueMap.descend` (do not hardcode). |
| 4 | `onchainos competition user-status [--activity-id <id>]` | Wallet login | Check participation & reward status using the active user's `accountId` (omit `--activity-id` to check all activities). MCP tool `competition_user_status` takes no wallet args — `accountId` is loaded from the local wallet store. |
| 5 | `onchainos competition join --activity-id <id> --evm-wallet <addr> --sol-wallet <addr> --chain-index <chain_id>` | Wallet login | Register for the competition. MCP tool `competition_join` makes both wallet args optional. |
| 6 | `onchainos competition claim --activity-id <id> --evm-wallet <addr> --sol-wallet <addr>` | Wallet login | CLI returns unsigned calldata. MCP tool `competition_claim` is **atomic** — wallets are optional, signing + broadcast happens inside the tool, returns txHash array. Surfaces `needContact: true` for top-tier winners (see Step 6 contact-collection sub-flow). |
| 7 | `onchainos competition submit-contact --activity-id <id> --contact-type <Telegram\|WeChat\|Email\|Twitter> --contact-value <text>` | Wallet login | Record a contact method for top-tier winners (called only after a claim that returned `needContact: true`). MCP tool `competition_submit_contact` looks up accountId + joinedAddress internally. |

`--status` (request filter): `0`=active, `1`=ended, `2`=all  
`activityStatus` (response field): **`3`=active, `4`=ended** — these are DIFFERENT values from the request filter  
`sort-type`: dynamic — read from `competition_detail` → `tabConfigs[].rankFieldConfig[].sortValueMap.descend`. Currently observed values: `1`=PnL% (realized ROI), `7`=PnL (realized profit). Future activities may add more — always trust `tabConfigs` over hardcoding.

## Output Rules

> **Internal-only IDs vs user-facing display.** Internal numeric IDs (`activityId`, `chainIndex`, `accountId`) are returned in tool responses on purpose — they are needed to chain calls between tools (e.g. after `competition_join`, you may need to call `competition_detail` with the activity id to fill the success template). **Keep them in the data layer; never render them in user-visible messages.**

<NEVER>
**Never include any internal id in a message produced for the user — under ANY circumstance, in ANY format.** Identify activities to the user EXCLUSIVELY by `activityName` (or `shortName` if name is unavailable).
</NEVER>

**Forbidden user-visible patterns** (do NOT produce output like this):
- ❌ `Agentic Trading Contest (#107)`
- ❌ `#106 (agenticwallettest1)`
- ❌ **A column** titled `ID`, `Activity ID`, `#`, or any equivalent in the user's language
- ❌ **A row** labeled `Activity ID`, `ID`, `#`, or any equivalent in the user's language (e.g. a 2-column key/value table where a row maps the id label to the numeric value) — this is the same violation as a column, just rotated
- ❌ Any reference like `competition 107`, `id 107`, `the activity id is 107`, or the same wording in another language
- ❌ Numeric id rendered anywhere a user can see it, regardless of label, shape, or language

**Correct user-visible pattern**:
- ✅ `Agentic Trading Contest`
- ✅ When disambiguating two activities with the same name, append `chainName` (e.g. `Agentic Trading Contest (Solana)`), never the ID.

**Behind the scenes (allowed and expected)**:
- ✅ Reading `activityId` from a `competition_user_status` / `competition_join` response and passing it to `competition_detail` to fetch the data needed by a fixed template.
- ✅ Any tool-to-tool chaining via numeric ids — as long as the final user-facing message omits them.

When the user asks to act on a specific activity (e.g. "claim Agentic Trading Contest"), the MCP tools `competition_claim` / `competition_join` accept `activity_name` and resolve the id server-side, so you can also use names directly without doing your own lookup.

## Time Formatting (MANDATORY)

<MUST>
All timestamps from competition APIs MUST be rendered using these exact rules. Do NOT free-style time conversions.
</MUST>

**Preferred — use backend-formatted strings when available:**

For the `competition_detail` response, the backend now returns pre-formatted UTC+8 strings — use them verbatim, do NOT recompute from the raw epoch:

| Field | Format | Example |
|-------|--------|---------|
| `startTimeFormatted` | `yyyy-MM-dd HH:mm:ss` (UTC+8, no suffix) | `"2026-05-07 18:00:00"` |
| `endTimeFormatted` | `yyyy-MM-dd HH:mm:ss` (UTC+8, no suffix) | `"2026-05-21 18:00:00"` |

When rendering, take the string as-is and append ` (UTC+8)` for the timezone marker (e.g. `2026-05-07 18:00:00 (UTC+8)`). This is the **only** correct path for detail view times — the backend has already done the math; computing from `startTime` / `endTime` epoch invites off-by-an-hour errors and AI hallucination.

**Raw fields — only use when no `*Formatted` counterpart is present:**

| Field | Format | Example raw | Notes |
|-------|--------|-------------|-------|
| `startTime`, `endTime` (list response — no formatted variant returned) | **10-digit Unix seconds** | `1778148000` | Multiply by 1000 only if the runtime expects ms |
| `joinTime`, `claimTime` (user-status response) | **10-digit Unix seconds** | `1778148000` | Same |
| `rankUpdateTime` (rank response) | **13-digit Unix milliseconds** | `1774359000638` | Divide by 1000 to convert to seconds first |

If you see a 13-digit value where a 10-digit is documented (or vice versa), do NOT silently coerce — flag it as a backend anomaly.

**Display format — exact, no improvisation:**

- Detail view (with `*Formatted` available): `YYYY-MM-DD HH:mm:ss (UTC+8)` — example: `2026-05-07 18:00:00 (UTC+8)` (use the formatted string + suffix)
- List view compact range: `YYYY-MM-DD ~ YYYY-MM-DD` (UTC+8 day boundary) — example: `2026-05-07 ~ 2026-05-21` (compute from raw `startTime` / `endTime`, take the date portion only)
- `joinTime` / `claimTime` in user-facing context: `YYYY-MM-DD HH:mm (UTC+8)` (compute from raw epoch)
- `rankUpdateTime` (last refresh marker): `YYYY-MM-DD HH:mm:ss (UTC+8)` (compute from raw ms epoch)

**Timezone rule:** ALL competition times displayed to the user are in **UTC+8** (China Standard Time). The competition product is operated in UTC+8; never display raw UTC, never use the user's local timezone.

**When you DO need to convert from raw epoch (no formatted field available):**
1. Identify the field's documented unit (seconds or milliseconds — see table above).
2. Convert to a Date object using the correct unit.
3. Format as a UTC+8 wall-clock string per the display format above.
4. Always append the `(UTC+8)` suffix.

<NEVER>
- ❌ Do NOT recompute time from `startTime` / `endTime` epoch when `startTimeFormatted` / `endTimeFormatted` is present in the response — use the backend-formatted string directly.
- ❌ Do NOT shell out to `date -r <epoch>` (or any equivalent) when a `*Formatted` field is available — it adds tool-call noise for no benefit and risks platform-specific conversion bugs.
- ❌ Do NOT mentally compute the date from a raw timestamp using your training-data sense of "current date" — always either use the backend `*Formatted` field, or do the explicit numeric conversion.
- ❌ Do NOT mix seconds and milliseconds — a 13-digit value treated as seconds lands in year ~58000; a 10-digit value treated as ms lands in 1970.
- ❌ Do NOT drop the `(UTC+8)` suffix on date-time strings.
- ❌ Do NOT use the user's local timezone — even if the user is overseas, competition times are operated in UTC+8.
</NEVER>

## Output Language

<MUST>
**Render every fixed template in the user's conversation language.** The template structure (sections, ordering, numbered items, table column count, placeholder positions, the `{supportedChains}` placeholder, and the `[Disclaimer: ...]` block) is fixed and must NOT change. Only the natural-language text inside is translated to the user's language naturally.

**Placeholders are never translated.** `{supportedChains}`, `{chainName}`, `{rewardUnit}`, `{txHash}`, `{accountName}`, etc. are filled with API values verbatim — do not localize them. Chain display names (e.g. `Solana`, `X Layer`, `Base`) come from the canonical id → name mapping and stay as-is in every language.
</MUST>

## Execution Flow

### Step 1 — Discover Competitions

#### Choosing the status filter

Decide which `status` to pass based on the user's intent:

| User intent | Pass `status` | Returned `activityStatus` values |
|---|---|---|
| Generic listing ("show competitions") | `2` (all) | mix of 3 (active) and 4 (ended) |
| Active only ("which can I join now") | `0` (active filter) | only 3 |
| Ended only ("winners list") | `1` (ended filter) | only 4 |

When in doubt, prefer `status=2` so the user can see the full picture and pick.

<MUST>
**Display the result as markdown tables — one row per competition. Do not use a numbered prose list, do not collapse fields into a single sentence.**

When the result contains BOTH active (`activityStatus=3`) and ended (`activityStatus=4`) entries, **split into two separate tables under bold subheadings (`**Active**` / `**Ended**`, translated to the user's language), in that order**. When only one status is present, render a single table without a subheading.
</MUST>

#### Fixed table template (English canonical; translate cells when user is non-English)

```
**Active**

| Name | Chain | Time | Total Prize Pool | Details |
|------|-------|------|------------------|---------|
| {name} | {supportedChains} | {startTime} ~ {endTime} | {rewards} | [View](https://web3.okx.com/boost/trading-competition/{shortName}) |
| ... | ... | ... | ... | ... |

**Ended**

| Name | Chain | Time | Total Prize Pool | Details |
|------|-------|------|------------------|---------|
| {name} | {supportedChains} | {startTime} ~ {endTime} | {rewards} | [View](https://web3.okx.com/boost/trading-competition/{shortName}) |
| ... | ... | ... | ... | ... |
```

For non-English users, translate the column headers, section headers, and link text naturally. The structure (column count, ordering, `{supportedChains}` placeholder) does not change.

#### Field-mapping rules

- Group rows by `availableCompetitions[].status`: `3` → Active table, `4` → Ended table.
- Name column ← `name`
- **Chain column** ← `{supportedChains}`, computed as the **union of `participateChainIds` and `chainId`**:
  1. Start with the ids in `participateChainIds` (in backend-returned order).
  2. If `chainId` is not already in that list, append it at the end.
  3. Map each id to its display name. Currently supported competition chains: `1 → Ethereum`, `196 → X Layer`, `501 → Solana`.
  4. Join with `, `.
  - Examples:
    - `chainId=196`, `participateChainIds=[501]` → `Solana, X Layer`
    - `chainId=196`, `participateChainIds=[196, 501]` → `X Layer, Solana` (chainId already in list — no duplicate)
    - `chainId=501`, `participateChainIds=[501]` → `Solana`
    - `chainId=196`, `participateChainIds` empty/missing (legacy activity created before the field was added) → `X Layer` (chainId only)
  - Rationale: Both `chainId` and `participateChainIds` are trading chains — trades on any of them count. `chainId` additionally is the claim chain. The display union exposes the user to the full set so they can pick where to trade.
- Time column ← `startTime` ~ `endTime` formatted per **Time Formatting** rules above. List-table compact form: `YYYY-MM-DD ~ YYYY-MM-DD` in UTC+8 (e.g. `2026-05-07 ~ 2026-05-21`). Do NOT include time-of-day in the compact list to keep the column narrow — full time-of-day is shown in Step 2 detail view only.
- Total Prize Pool column ← `rewards` field (already a formatted string like `50,000 USDC`)
- Details column ← `https://web3.okx.com/boost/trading-competition/<shortName>` as a markdown link

After the table(s), ask the user (in their language):
- If only Active has entries: `Which competition would you like to view in detail, or would you like to register directly?`
- If only Ended has entries: `Would you like to check your ranking or claim status for any of these?`
- If both: combine — `Which active competition would you like to register or view, or which ended competition would you like to check your ranking / claim?`

#### Empty-result handling (English canonical; translate to user's language)

- All filters returned 0 entries → `No trading competitions available right now.`
- `status=0` filter returned 0 entries → `No active trading competitions at the moment.`
- `status=1` filter returned 0 entries → `No ended trading competitions yet.`

#### CLI equivalent

```bash
onchainos competition list --status 2   # all
onchainos competition list --status 0   # active only
onchainos competition list --status 1   # ended only
```

### Step 2 — View Details (if requested)

```bash
onchainos competition detail --activity-id <id>
```

<MUST>
**Display competition / reward info using the fixed English template below.** The structure (sections, ordering, numbered list, placeholder positions, the `{supportedChains}` placeholder on the chain line) is fixed. Copy the template character-for-character; only fill in placeholders. Do not paraphrase, abbreviate, or substitute synonyms.

When the user's language is not English, translate the natural-language strings to the user's language while preserving the structure, the placeholders, and every required content invariant listed below. Do not reorder, omit, or merge sections.
</MUST>

#### Fixed display template

```
Basic Information
Supported chains: {supportedChains}
Duration: {startTime} ~ {endTime}
Total Prize Pool: {totalPrizePool}

Prize Categories:
Realized PNL% Prize Pool ({roiPoolAmount})
Ranked from highest to lowest by realized PNL%.
{roiRankTable}

Realized PnL Prize Pool ({pnlPoolAmount})
Ranked from highest to lowest by realized PNL amount.
{pnlRankTable}

Participation Prize ({participationPoolAmount})
Registered users who accumulate $100 or more in total trading volume via Agentic Wallet and maintain a total wallet balance of $100 or above throughout the competition period, will share the {participationPoolAmount} participation prize pool equally. Random asset snapshots will be taken during the competition period to verify eligibility.

Skill Quality Prize ({skillPoolAmount})
The Skill Quality Prize is an independently judged award. During the competition period, participants may submit their Agent Skills through the event landing page. Eligible submissions include, but are not limited to, on-chain autonomous yield strategies, trading analysis, and trading signal monitoring. All submitted Agent Skills will be evaluated through a dual-review process combining AI pre-screening and manual judging. The top {skillTopN} Skill creators by score will each receive a reward of {skillPerCreatorReward}.
```

#### Field-mapping rules

- Chain line ← `{supportedChains}`, computed as the **union of `data.participateChainIds` and `data.chainId`**:
  1. Start with `data.participateChainIds` (in backend-returned order).
  2. Append `data.chainId` at the end if not already present.
  3. Map each id to its display name. Currently supported competition chains: `1 → Ethereum`, `196 → X Layer`, `501 → Solana`.
  4. Join with `, `.
  - Examples (using real backend shapes):
    - `chainId=196`, `participateChainIds=[501]` → `Solana, X Layer`
    - `chainId=196`, `participateChainIds=[196, 501]` → `X Layer, Solana`
    - `chainId=501`, `participateChainIds=[501]` → `Solana`
    - `chainId=196`, `participateChainIds` empty/missing (legacy activity) → `X Layer` (chainId only)
  - Both fields are trading chains (trades on any of them count toward the competition standing); `chainId` additionally hosts the reward contract / claim path. Display the union so the user sees the full trading set.
- `{startTime}` / `{endTime}` ← read `startTimeFormatted` / `endTimeFormatted` directly from `competition_detail.data` and append ` (UTC+8)`. Final form: `YYYY-MM-DD HH:mm:ss (UTC+8)` (e.g. `2026-05-07 18:00:00 (UTC+8)`). Do NOT compute from raw `startTime` / `endTime` epoch — the backend has already done the math.
- `{totalPrizePool}` ← sum of all `prizePoolDistribution[].totalReward` plus `rewardUnit` (e.g. `50,000 USDC`).
- `{roiPoolAmount}` ← totalReward of the realized-ROI tab.
- `{pnlPoolAmount}` ← totalReward of the realized-PnL tab.
- `{participationPoolAmount}` ← totalReward of the participation prize tab.
- `{skillPoolAmount}` ← totalReward of the Skill quality prize tab.
- `{skillTopN}` ← upper bound of the Skill tab's `rules[].interval` (e.g. `"1-10"` → `10`).
- `{skillPerCreatorReward}` ← that rule entry's `reward` + `rewardUnit` (e.g. `500 USDC`).
- `{roiRankTable}` / `{pnlRankTable}` ← markdown table built from the corresponding tab's `rules[]`. Format (English canonical; localize headers to user's language):

  ```
  | Rank | Reward |
  |------|--------|
  | <interval-formatted> | <reward-formatted> |
  | ...                  | ...                |
  | Total | <totalReward> {rewardUnit} |
  ```

  Interval / reward formatting per row:
  - Single rank (`interval = "1"`) → Rank cell `Rank 1`, Reward cell `<reward> <rewardUnit>` (no `each` prefix)
  - Range (`interval = "2-6"`) → Rank cell `Ranks 2-6`, Reward cell `<reward> <rewardUnit> each`
  - Always end with a totals row whose Reward cell is the tab's `totalReward` + `rewardUnit`.

If any of the four pools is absent for a particular activity, omit just that section (keep the others as-is).

#### Required content invariants (per section)

**Section 1 — Realized PNL% Prize Pool**
- Title MUST be exactly `Realized PNL% Prize Pool` (or its faithful translation in the user's language). Do NOT substitute with `PnL% Ranking Award` / `ROI Ranking Award` / `Realized ROI Pool`.
- Description MUST mention: ranking by realized PNL%, highest to lowest.
- Rank table MUST have headers `Rank / Reward` and end with a `Total` row.

**Section 2 — Realized PnL Prize Pool**
- Title MUST be exactly `Realized PnL Prize Pool`. Do NOT substitute with `PnL Ranking Award` / `Realized PnL Pool`.
- Description MUST mention: ranking by realized PNL amount, highest to lowest.
- Rank table MUST follow the same format as Section 1.

**Section 3 — Participation Prize** (PRODUCT-MANDATED COPY)
- Title MUST be exactly `Participation Prize`.
- The description body MUST include all of these specific terms:
  - `Agentic Wallet`
  - accumulate `$100` or more in total trading volume
  - maintain a total wallet balance of `$100` or above throughout the competition period
  - share the participation prize pool equally
  - random asset snapshots to verify eligibility

**Section 4 — Skill Quality Prize** (PRODUCT-MANDATED COPY)
- Title MUST be exactly `Skill Quality Prize`.
- The description body MUST include all of these specific terms:
  - independently judged award
  - submission of Agent Skills through the event landing page
  - examples of eligible submissions (on-chain autonomous yield strategies, trading analysis, trading signal monitoring)
  - dual-review process combining AI pre-screening and manual judging
  - `top {skillTopN} Skill creators ... each receive a reward of {skillPerCreatorReward}`

<NEVER>
- ❌ Do NOT invent or omit chains on the chain line — `{supportedChains}` must reflect the **union of `participateChainIds` and `chainId`** (dedup, participateChainIds order first, then `chainId` if missing). Never drop `chainId` because `participateChainIds` is present; never drop `participateChainIds` because `chainId` exists.
- ❌ Do NOT reorder or merge the four reward sections — they must appear in the order 1 → 2 → 3 → 4.
- ❌ Do NOT add ID columns or expose any internal numeric id (`activityId`, etc.) anywhere in the output.
- ❌ Do NOT paraphrase, abbreviate, or substitute synonyms in Sections 3 and 4. These are product-mandated copy.
- ❌ Do NOT invent rank-distribution rules from the pool amount. The actual rules come from `prizePoolDistribution[].rules[]` — read them; do not divide.
- ❌ Do NOT use bullet markers (`-`) inside the four numbered sections — the structure is `1. Title (amount)\n description text` then the rank table; not a bullet list.
</NEVER>

After printing the template, ask: `Would you like me to register you for this competition?`

### Step 3 — Join (requires wallet login)

**MCP**: call `competition_join` with `activity_name` and `chain_index` only — `evm_wallet` and `sol_wallet` are auto-resolved from the active account.

**CLI**: pass addresses explicitly:
```bash
onchainos competition join --activity-id <id> --evm-wallet <evm_addr> --sol-wallet <sol_addr> --chain-index <chain_id>
```

Get `chainIndex` from `competition_detail` → `chainIndex` field.

If the user is not logged in, the tool returns `not logged in — please run: onchainos wallet login`. Tell the user verbatim:
> Please run `onchainos wallet login <your_email>` in your terminal to log in (it cannot be done from inside this conversation), then ask me to register again.

#### Required pre-flight: distinguish duplicate-registration scenarios

<MUST>
**Before calling `competition_join`, you MUST first call `competition_user_status` for the activity to read the current account's `joinStatus`.** This separates the two duplicate-registration cases that have different user-facing messages.
</MUST>

| Scenario | `user_status.joinStatus` (current account) | Action | Template |
|----------|-------------------------------------------|--------|----------|
| **A — current account already joined** | `1` | Do NOT call `competition_join` | Scenario A template (below) |
| **B — current account NOT joined** | `0` | Call `competition_join` | If success → success template; if `code=11016` → Scenario B template |

##### Scenario A — current wallet already registered

Template:

```
Your current wallet account [accountName] is already registered for [activityName]. No need to register again. Would you like me to walk you through the rules in detail, or start trading directly?
```

Field-mapping:
- `[accountName]` ← `accountName` of the currently selected account (read from `wallet_store` / `wallet status`, e.g. `Account 1`)
- `[activityName]` ← `activityName` from the prior `competition_user_status` / `competition_list` response

##### Scenario B — same login, different account already registered

Triggered when `competition_join` returns `code=11016 Participation limit reached`.

Template:

```
Registration failed. Your wallet account [registeredAccountName] is already registered. You cannot register again. Please switch to your registered account to trade.
```

Field-mapping:
- `[registeredAccountName]` ← name of the OTHER account in the same login that holds the registration. To find it, iterate every account from `wallet_store` other than the current one and call `competition_user_status` for the activity, picking the one whose `joinStatus=1`. If no account is found (rare race), fall back to a generic phrase like `another of your wallet accounts is already registered` and ask the user to check `onchainos wallet status` themselves.

#### Successful registration

<MUST>
**On every successful `competition_join` call (the tool returns `joined: true`), output the fixed template below.** Structure (the lead phrase + the supported-chains sentence + the closing question + the bracketed disclaimer on its own line) is fixed. `{supportedChains}` is the union of `participateChainIds` and `chainId` (see Field-mapping rules below); `{totalPrizePool}` is filled from `competition_detail` (call it before formatting if you don't have it cached). Translate the natural-language strings to the user's language while preserving structure and placeholders.
</MUST>

Template:

```
Registered successfully! This competition runs on {supportedChains}, with a total prize pool of {totalPrizePool}. The trading contest ranks players by both PnL% and realized PnL, with additional Participation and Skill Quality Prizes. Would you like me to walk you through the detailed rules, or help you initiate a trade on {supportedChains}?

[Disclaimer: Digital asset trading involves risk. Prices can be highly volatile. Please understand the risks fully and do your own research before trading.]
```

**Field-mapping rules**

- `{supportedChains}` ← computed as the **union of `data.participateChainIds` and `data.chainId`** from `competition_detail`. Take participateChainIds in backend order, append `chainId` at the end if not already in the list, map each id to its display name (currently supported competition chains: `1 → Ethereum`, `196 → X Layer`, `501 → Solana`), join with `, `. Examples: `chainId=196`+`participateChainIds=[501]` → `Solana, X Layer`; `chainId=501`+`participateChainIds=[501]` → `Solana`. The lead sentence and the closing question both use the same string; do not paraphrase by listing chains separately.
- `{totalPrizePool}` ← total reward pool (sum of all `prizePoolDistribution[].totalReward` + `rewardUnit`, e.g. `500 DJT`).

<NEVER>
- ❌ Do NOT invent or omit chains on the chain line — `{supportedChains}` must reflect the **union of `participateChainIds` and `chainId`** (dedup, participateChainIds order first, then `chainId` if missing). Never drop `chainId` because `participateChainIds` is present; never drop `participateChainIds` because `chainId` exists.
- ❌ Do NOT drop or merge the four key phrases of the lead sentence: (1) which chains it runs on (from `{supportedChains}`), (2) the total prize pool, (3) the dual-axis PnL%/realized PnL ranking, (4) the existence of Participation and Skill Quality Prizes. These are required content; the wording can be localized but the four pieces must all appear.
- ❌ Do NOT drop the bracketed disclaimer line — it must appear on its own line at the end of the message, in the user's language.
</NEVER>

#### Other errors

**On error containing `region` / `not available in your region`:**
> Registration failed: service is not available in your region. Please switch to a supported region and try again.

**On any other error:**
> Operation failed. Please contact customer support.

### Step 4 — Trade (delegate to okx-dex-swap)

When user asks to trade per competition rules:

**Case A — User does NOT provide a CA (only token name/symbol):**
1. Resolve the CA via the `token_search` MCP tool (CLI: `onchainos token search`).
2. Confirm with user before proceeding:
   > Just to confirm, the CA for token "{tokenSymbol}" is "{contractAddress}". Is that correct?
3. Wait for user to confirm. Only proceed after explicit "yes".
4. Then follow **Case B** below.

**Case B — User provides a CA directly:**
1. **Execute swap** via the `swap_swap` MCP tool (CLI: `onchainos swap swap`); see the `okx-dex-swap` skill for parameters.
2. Report: "Done — your trade has been submitted." + tx hash.

> Note: do NOT pre-empt the swap with an extra "token prices are volatile, do you accept the risk?" prompt. The user already requested the trade — additional friction is unwanted. Per-token risk metadata (e.g. honeypot / extreme volatility flags) belongs to `okx-security` and only fires when actually flagged.

**Competition constraints per trade:**
- Single-trade min $1 (orders below $1 are not counted)
- Token pairs must match competition rules from `detail` response

### Step 5 — Check Status & Rank

#### Check participation status

```bash
onchainos competition user-status                       # all activities (uses accountId)
onchainos competition user-status --activity-id <id>    # single activity (uses accountId)
```

Display: join status, join time, reward status, reward amount.

- If `rewardStatus=1` (won, not claimed): proactively ask "You have won a reward. Would you like me to claim it for you?"
- If `rewardStatus=4` (pending draw): use the **Pending-draw canonical template** (English canonical below; translate to the user's language; substitute `{activityName}` from the activity's `name` / `shortName` field; do NOT paraphrase the 5-business-day window):
  > "{activityName} has ended. The winners list is currently being finalized. The final reward list will be announced within 5 business days after the activity end — please return here to check your result and claim your reward then. Thank you for participating!"
- If `rewardStatus=3` (expired): "Your reward has expired and can no longer be claimed."

#### Check leaderboard (full board)

<MUST>
When the user says "view leaderboard" without specifying which one, you MUST:

1. Call `competition_detail` for the activity and enumerate `tabConfigs[].rankFieldConfig[].sortValueMap.descend` — this is the full set of leaderboards the activity exposes.
2. Call `competition_rank` ONCE PER `sort_type` (one HTTP call per leaderboard) so you have data for every leaderboard.
3. Render ALL of them in the response — one section per leaderboard. Do NOT silently default to a single leaderboard (e.g. only `sort_type=1`) when the activity has more than one.

Only ask the user to pick one when there are clearly too many to fit (≥ 3 leaderboards on a single competition). With 1–2 leaderboards, always show all by default.
</MUST>

`tabConfigs[].rankFieldConfig[]` fields:
- `title` — display name (e.g. `PnL%`, `PnL`)
- `key` — internal sort field (e.g. `pnl`, `realizedProfit`)
- `sortValueMap.descend` — the numeric value to pass as `--sort-type`

**Per-leaderboard fetch:**
```bash
onchainos competition rank --activity-id <id> [--wallet <addr>] --sort-type <descend> --limit 20
```

**Display rules:** for each leaderboard render a separate section labeled by its `title`. Each section shows top N entries: rank, nickname (masked), score (`userTotal` formatted by `format` field), estimated reward.

Example response (activity with two leaderboards):
> **PnL% leaderboard** — pool 200 DJT
> Rank 1, Agentic....sMWP, PnL% +0.17%, estimated reward 100 DJT
> Rank 2, Agentic....gweD, PnL% +0.03%, estimated reward 20 DJT
>
> **PnL leaderboard** — pool 200 DJT
> Rank 1, Agentic....sMWP, PnL $0.1885, estimated reward 100 DJT
> Rank 2, Agentic....gweD, PnL $0.0006, estimated reward 20 DJT

After the leaderboards, append a "Your rank" section using the **CASE 1 / 2 / 3 templates** from the next section, since you already have all the data.

#### Check user's own rank (across ALL leaderboards)

A user can simultaneously appear on multiple leaderboards (e.g. PnL% AND PnL). When the user asks "what's my rank?", you MUST query every leaderboard the activity exposes, then render one of the three fixed templates below.

**Required flow:**
1. Call `competition_detail` → enumerate `tabConfigs[].rankFieldConfig[].sortValueMap.descend` to get the full set of `sort_type` values for this activity.
2. For EACH `sort_type`, call `competition_rank --sort-type <descend>` and capture `myRankInfo` plus the leaderboard's threshold (lowest `userTotal` in `allRankInfos`).
3. Classify the result:
   - **CASE 1** — user has `currentRank > 0` on every leaderboard
   - **CASE 2** — user has `currentRank > 0` on at least one but not all
   - **CASE 3** — user has no `currentRank > 0` on any leaderboard
4. Output the matching fixed template, **rendered in the user's language** (English canonical below; localize for Chinese / other-language users).

<MUST>
**Output exactly the matching template structure below — never paraphrase the data fields, never collapse the two-leaderboard sections into one. Localize the natural-language strings to the user's language; keep placeholders, numeric values, and units verbatim.**
</MUST>

##### CASE 1 — ranked on both PnL and PnL%

Template:

```
Realized PnL ranking:
You are currently ranked #{pnlRank}, estimated reward {pnlReward} {rewardUnit}!

Realized ROI ranking:
You are currently ranked #{roiRank}, estimated reward {roiReward} {rewardUnit}!

| Leaderboard | My rank | Estimated reward |
|-------------|---------|------------------|
| Realized PnL | #{pnlRank} | {pnlReward} {rewardUnit} |
| Realized ROI | #{roiRank} | {roiReward} {rewardUnit} |

Your total estimated reward across both rankings: {totalReward} {rewardUnit} (sum of the two)
```

##### CASE 2 — ranked on one leaderboard, off the other

There are two symmetric sub-cases. The structure is identical: the ranked leaderboard goes first ("ranked #N, estimated reward X"), then the unranked one ("not on the leaderboard, current value Y, threshold Z"). Each sub-case has its own pinned template — do NOT improvise the unranked-section unit (`%` for PnL%, currency `$` for PnL).

###### CASE 2-A — on PnL, off PnL% (currentRank for sort_type=7 > 0; sort_type=1 == 0)

Template:

```
Realized PnL ranking:
You are currently ranked #{pnlRank}, estimated reward {pnlReward} {rewardUnit}!

Realized ROI ranking:
Not on the leaderboard yet. Your current realized ROI is {currentRoi}%. You need at least {minRoi}% (the current leaderboard minimum) to qualify.
```

###### CASE 2-B — on PnL%, off PnL (currentRank for sort_type=1 > 0; sort_type=7 == 0)

Template:

```
Realized ROI ranking:
You are currently ranked #{roiRank}, estimated reward {roiReward} {rewardUnit}!

Realized PnL ranking:
Not on the leaderboard yet. Your current realized PnL is ${currentPnl}. You need at least ${minPnl} (the current leaderboard minimum) to qualify.
```

**Section ordering rule**: the leaderboard the user **IS** ranked on ALWAYS goes first. Don't put the "Not on the leaderboard" section before the ranked one.

**Unit rule**: PnL% uses `%` suffix (no currency symbol); PnL uses `$` prefix (or the appropriate currency unit). Do NOT mix them up — the user's threshold for PnL is a dollar amount, not a percentage.

##### CASE 3 — off both leaderboards

Template:

```
Your address is not on any leaderboard. Your current realized PnL is ${currentPnl}, realized ROI {currentRoi}%.
The current minimum to qualify: realized PnL ${minPnl}, realized ROI {minRoi}%.
```

##### Field-mapping rules

- `{pnlRank}` ← `myRankInfo.currentRank` of the PnL leaderboard (sort_type 7)
- `{pnlReward}` ← `myRankInfo.expectedRewards` of the PnL leaderboard
- `{roiRank}` ← `myRankInfo.currentRank` of the PnL% leaderboard (sort_type 1)
- `{roiReward}` ← `myRankInfo.expectedRewards` of the PnL% leaderboard
- `{rewardUnit}` ← `myRankInfo.rewardUnit` (e.g. `DJT`); per-leaderboard if they ever differ
- `{totalReward}` ← `pnlReward + roiReward` (numeric sum, same unit)
- `{currentRoi}` ← user's PnL% score from `myRankInfo.userTotal` of the PnL% board (or 0 if backend returned null)
- `{currentPnl}` ← user's PnL score from `myRankInfo.userTotal` of the PnL board
- `{minRoi}` ← lowest qualifying PnL% — last entry's `userTotal` in the PnL% board's `allRankInfos[]`
- `{minPnl}` ← lowest qualifying PnL — last entry's `userTotal` in the PnL board's `allRankInfos[]`

If the activity exposes leaderboards beyond PnL/PnL% (future expansion via `tabConfigs[]`), extend the same template pattern: one section per leaderboard, summary table aggregates all, total reward sums all `expectedRewards`.

`format`: `1`=number, `2`=percentage, `3`=token amount with unit.

### Step 6 — Claim Reward

Check status first via `competition_user_status`:

| `rewardStatus` | Action |
|---|---|
| 0 | Not won — inform user, no claim needed |
| 1 | Won — proceed to claim |
| 2 | Already claimed |
| 3 | Expired — "Your reward has expired and can no longer be claimed" |
| 4 | Pending draw — render the **Pending-draw canonical template** (see Step 5 above). Do NOT call `competition_claim`; the winners list is not finalized yet. |

#### Pre-claim guard (rewardStatus=4 / Pending draw)

<MUST>
When the user explicitly requests to claim a reward (any "claim my reward" / "claim X" intent in any language) for an activity whose `rewardStatus` is `4` (Pending draw), do **NOT** call `competition_claim`. Render the **Pending-draw canonical template** (see Step 5 above, with `{activityName}` substituted) instead.

This applies whether the user explicitly named the activity or you inferred it from prior status output. Calling claim on a `rewardStatus=4` activity would either be rejected by the backend or, worse, returns a confusing technical error. The canonical template is the only correct user-facing response.
</MUST>

#### Atomic claim (the only correct path)

Both the MCP tool `competition_claim` and the CLI `onchainos competition claim` now do the **same atomic flow**: pre-check `rewardStatus`, fetch calldata, sign each entry with the TEE session, broadcast on-chain, return txHash array. The CLI no longer returns raw unsigned calldata — the only externally visible behavior is the final result.

**MCP** (preferred when running inside Claude Code / any AI environment):
```
competition_claim(activity_name="...")  →  { rewardAmount, rewardUnit, succeeded[], failed[] }
```

**CLI** (terminal use, or AI shelling out via Bash):
```bash
onchainos competition claim --activity-id <id> --evm-wallet <evm_addr> --sol-wallet <sol_addr>
# → returns the same { rewardAmount, rewardUnit, succeeded[], failed[] } shape
```

Result shape (both paths):
```json
{
  "rewardAmount": "460",
  "rewardUnit": "PYBOBO",
  "totalEntries": 1,
  "succeeded": [{"contractAddress": "...", "chain": "501", "txHash": "...", "orderId": "..."}],
  "failed": [],
  "needContact": false,
  "activityId": "107",
  "accountId": "5747d742-...",
  "joinedAddress": "0x8e3f..."
}
```

**How to report to the user:**
- All succeeded (`failed: []`): "Claimed {rewardAmount} {rewardUnit}, tx hash: {txHash}"
- Partial success (some `failed`): list each succeeded txHash, then list the failed entries with their `error`, then append the **fixed failure-suggestion block** (template below). **Do NOT re-run claim blindly** — succeeded entries already landed; another call will hit the "reward already claimed" guard.
- All failed: the tool returns an error, not this shape — surface the error message verbatim, then append the **fixed failure-suggestion block**.
- **If `needContact: true` in the response** (user is a top-tier winner who has NOT yet submitted contact info): after the success line above, also render the **Contact-collection prompt** below — invite (do NOT force) the user to share one contact method. See `#### Contact collection (top-tier winners only)` further down for the prompt template, parsing rules, and follow-up.

The flow blocks before signing if `rewardStatus` is 0 (not eligible), 2 (already claimed), 3 (expired), or 4 (winners not announced yet). The error message is plain text — relay it to the user. **Skip** the failure-suggestion block in these pre-check rejections (they are semantic, not transient — telling the user to "check Gas / try later" is misleading).

##### Fixed failure-suggestion block

<MUST>
For runtime failures (signing/broadcast/simulation errors, network errors, unknown errors), append this block after the error description. Translate to the user's language while preserving the heading + 3 bullet items in this order. Do NOT add or remove items.
</MUST>

Template:

```
Suggestions:
- The claim process requires Gas. Please make sure your Gas is sufficient.
- Try again later — this may be a transient network issue.
- If it fails repeatedly, please contact customer support.
```

<NEVER>
- ❌ Do NOT show this block on pre-check rejections (rewardStatus=0/2/3/4) — the issue is not Gas / not transient.
- ❌ Do NOT show this block on `code=11002` (not won) or `code=11008` (claim expired/already claimed) — same reason.
</NEVER>

<NEVER>
- ❌ Do NOT chain `gateway_broadcast` after a claim call — the on-chain submission already happened inside the tool.
- ❌ Do NOT manually construct, encode, or sign a transaction (no Python base58 encoding, no manual hex assembly). The TEE-managed wallet key is the only valid signer.
- ❌ Do NOT inspect the result for an empty `base58CallData` and conclude the CLI cannot sign a Solana claim — that field is empirically empty for Solana; the CLI/MCP code internally falls back to encoding `tx.data` byte array via base58 and proceeds. Just trust the `succeeded[]` and `failed[]` arrays.
- ❌ Do NOT split into a two-step "fetch calldata then wallet contract-call" flow — that mode no longer exists; the claim command is atomic.
</NEVER>

**On claim error (code 11002 `not eligible for reward`):** "You did not win a reward and cannot claim."  
**On any other error:** "Operation failed. Please contact customer support."

#### Contact collection (top-tier winners only)

<MUST>
Run this sub-flow **if and only if** the `competition_claim` response contains `needContact: true`. Do NOT run it when `needContact: false` or the field is missing. Do NOT ask for a contact pro-actively in any other claim path.
</MUST>

**Step 6a — After the claim-success line, append this prompt** (English canonical; translate the natural-language strings to the user's conversation language; keep the 4 numbered options in this exact order and the literal labels `Telegram` / `WeChat` / `Email` / `Twitter (X)` as-is — these are product-canonical, do not paraphrase):

```
Congratulations on your standout performance in this competition! As a thank-you, we have a custom merchandise pack reserved for top winners. Please share ONE of the following contact methods so we can reach out about delivery — sharing is optional:

1. Telegram
2. WeChat
3. Email
4. Twitter (X)
```

**Step 6b — When the user replies with a contact method**, parse out `contactType` and `contactValue` from their message:

| User's message contains | `contactType` | `contactValue` |
|---|---|---|
| `Telegram @handle`, `tg @handle`, `Telegram: @handle` | `Telegram` | the handle (preserve `@` if user included it) |
| `WeChat <id>`, `WeChat: <id>` | `WeChat` | the WeChat id |
| Anything looking like an email (`user@domain.com`) or `Email <addr>` | `Email` | the address |
| `Twitter @handle`, `X @handle`, `Twitter: @handle` | `Twitter` | the handle |

`contactType` MUST be one of the four exact case-sensitive strings (`Telegram`, `WeChat`, `Email`, `Twitter`) — the backend rejects anything else. If the user's message is ambiguous (e.g. just `@username` with no platform), ask once which platform they meant; do NOT guess.

**Step 6c — Call `competition_submit_contact`**:

```
competition_submit_contact(
  activity_name="<same activity name used in competition_claim>",
  contact_type="Telegram" | "WeChat" | "Email" | "Twitter",
  contact_value="<the parsed value, max 256 chars>"
)
```

CLI equivalent:
```bash
onchainos competition submit-contact --activity-id <id> --contact-type Telegram --contact-value "@testemma"
```

**Step 6d — On `submitted: true` response, render this confirmation** (English canonical; translate to the user's language; do NOT echo the contact value back; do NOT show any internal id):

```
Got it. Thanks for sharing! We will reach out shortly — please keep an eye on your messages.
```

**On submit_contact error**, surface the message verbatim with a short hint:
- If the backend returns a validation error on `contactType`, re-prompt the user with the 4 options.
- If `not registered for activity` — this should never happen post-claim; flag as a backend anomaly and tell the user to retry later.
- Other errors: "Failed to record your contact, please try again later or contact customer support."

<NEVER>
- ❌ Do NOT ask for contact on regular (non-top-tier) claims — `needContact: false` means do nothing.
- ❌ Do NOT echo the contact value back to the user in the confirmation message — they already see it in the conversation; repeating it can feel intrusive.
- ❌ Do NOT push / pressure the user if they decline to share. Acknowledge politely and move on.
- ❌ Do NOT prompt for multiple contacts — one is enough. Stop after the first valid submission.
- ❌ Do NOT call `competition_submit_contact` proactively without the user explicitly providing a contact value in this conversation turn.
- ❌ Do NOT expose `activityId` / `accountId` / the parsed `contactType` enum string to the user in any rendered message.
</NEVER>

## Additional Flows

### Query Registered Wallet

When user asks "show my registered address" or similar:

1. Call `competition_user_status` (MCP) — `accountId` is loaded from the active wallet session; no wallet args needed. CLI equivalent: `onchainos competition user-status` (omit `--activity-id` to query all activities).
2. Find entries where `joinStatus=1`
3. For each matched entry, present: competition name (`activityName`) + chain (`chainName`) + masked address (first4...last4). Use chain to determine which address was used (EVM or SOL).

If multiple entries match, list all of them.

Example (single):
> Your Account 1 is registered for **XXX Trading Competition**. Registered address: Solana address DeEV...Fbx.

Example (multiple):
> Your Account 1 is registered for the following trading competitions:
> - **XXX Trading Competition** (Solana): DeEV...Fbx
> - **YYY Trading Competition** (XLayer): 0x1234...abcd

If no entry has `joinStatus=1`:
> You are not currently registered for any trading competition.

### Wallet Export Guard

When the user requests to export the Agentic Wallet:

1. Call `competition_user_status` (MCP) — uses `accountId` from active session. CLI equivalent: `onchainos competition user-status`.
2. If any `joinStatus=1`:
   > Your wallet is registered for an Agentic Wallet trading competition. Exporting the wallet will forfeit your eligibility for this competition. Please confirm whether you want to proceed with the export.
3. Only proceed with export if the user explicitly confirms.

## Status Codes

### `--status` filter parameter (input only)

| Value | Meaning |
|-------|---------|
| 0 | Active competitions (default) |
| 1 | Ended competitions |
| 2 | All competitions |

### Response field values

| Field | Value | Meaning |
|-------|-------|---------|
| status | 3 | Competition active |
| status | 4 | Competition ended |
| joinStatus | 0 | Not joined |
| joinStatus | 1 | Joined |
| rewardStatus | 0 | Not won |
| rewardStatus | 1 | Won, not claimed |
| rewardStatus | 2 | Claimed |
| rewardStatus | 3 | Reward expired |
| rewardStatus | 4 | Pending draw (winners not yet announced) |

## Error Handling

| Error | Response |
|-------|----------|
| `not logged in` | Login is interactive (email + OTP) and cannot run inside this conversation. Tell the user verbatim: `Please run "onchainos wallet login <your_email>" in your terminal, then ask me again.` |
| `address limit reached` | Registration failed: this wallet account is already registered and cannot register again |
| code 11002 `not eligible for reward` | You did not win a reward and cannot claim |
| code 11003 `activity not found / status mismatch` | The competition does not exist or its status no longer permits this action |
| code 11008 `Claim expired` | The reward has already been claimed or the claim window has expired |
| code 1860402 `failed to assemble transaction` | Backend failed to build the on-chain transaction. Ask the user to retry; if it persists, contact customer support |
| `Sui-chain reward claims are not yet supported` | Sui rewards must be claimed from the Sui-compatible wallet UI (this client only signs EVM and Solana) |
| `region` / `not available in your region` | Registration failed: service is not available in your region. Please switch to a supported region and try again. |
| Any other error | Operation failed. Please contact customer support. |
