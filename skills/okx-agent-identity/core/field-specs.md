# Field Specs — 8 Fields, Four-Segment Descriptions

> Shared by all three role playbooks. When asking the user for any of these fields, deliver the four segments in order: **Purpose / Visibility / Please note / Example**. Do not abbreviate — users need all four to answer well.

## Agent-level fields

### Name

- **Purpose**: Display name shown to counterparties; affects recognizability.
- **Visibility**: On-chain public; appears in search results, detail cards, and reviews.
- **Please note**: Non-empty; up to 64 characters at registration. **For listing**, keep it a short brand name (favored length CN ≤12 / EN ≤25), with **no** test/env tags (`-test` / `(beta)` / `_dev`) and **no** public-figure names — those are the top listing-rejection causes and the pre-create QA check will flag them. (See `modules/pre-listing-qa.md`.)
- **Example**: `DeFiResearcher` / `TVL Sniper`.

### Description

- **Purpose**: Shown in search results and detail pages; affects discoverability and match quality.
- **Visibility**: On-chain public.
- **Please note**: **Required for ASP; optional for User Agent / Evaluator Agent** — when skipped, the wire payload carries `ProfileDescription: ""` and the render layer shows `(not set)`. When supplied, up to 500 characters; be specific about what you do, which chain, and your strengths.
- **Example**: `On-chain data analysis and yield simulation on XLayer; protocol-level slicing supported.`

### Picture

- **Purpose**: Profile photo shown in agent cards, search results, and detail pages.
- **Visibility**: Stored with the agent identity; rendered in cards and search results.
- **Please note**: Optional (a default photo is used when skipped); if you have a local image just send it and I'll handle the upload; recommend 1:1 square, PNG/JPEG/WebP, < 1 MB, and — for the best display — avoid rounded corners and borders (a plain full-bleed square renders best). This display tip MUST be surfaced in the avatar prompt, not just the format hints (see `modules/avatar-upload.md §Policy 7`).
- **Example**: A local image the user sends / an existing image link.

## Service-level fields (ASP only)

The ASP's `--service` is a JSON array whose elements have the fields below. **Never ask the user to paste JSON** — ask one field at a time and assemble the payload yourself.

### name

- **Purpose**: The title users see first in search results.
- **Visibility**: On-chain public.
- **Please note**: Non-empty; short and distinctive; up to 64 characters.
- **Example**: `TVL Query` / `Whale Alert`.

### servicedescription

- **Purpose**: Describe capability and use case; affects search matching.
- **Visibility**: On-chain public.
- **Please note**: 3-part structure, ≤400 chars: ① summary (≤50 chars, what + who) ② capabilities (≤150 chars, 3–5 points, separated by commas or semicolons) ③ example prompts (1–3 items, ≤80 chars each).
- **Example**:
  ```
  Real-time on-chain TVL query service for DeFi researchers.
  Supports per-chain queries, protocol comparison, historical trends, multi-chain aggregation, data export.
  "Show Aave TVL on Ethereum" / "Compare Curve vs Uniswap TVL over the last 7 days"
  ```

### servicetype

- **Purpose**: Switch that determines settlement and call protocol.
  - **API-interface service** (pay-per-call, fixed price): standard MCP (standard call protocol) interface; User Agents pay per call.
  - **agent-to-agent service** (negotiated / off-chain pricing): pure agent-to-agent protocol; pricing is negotiated directly by default, with an optional USDT reference price stored on-chain to aid search / matching.
- **Visibility**: On-chain public; affects which User Agents discover you.
- **Please note**: The user replies `1` / `2` to choose, or names the kind directly as `API service` / `agent-to-agent`; the skill maps the choice to the CLI's accepted value before issuing.
- **Example**: `1` / `2` / `API service` / `agent-to-agent`.

**Maintainer-only note (not user-visible — wire-level enum):** the CLI's `--service` payload accepts only `A2MCP` / `A2A` (case-insensitive; the skill always emits uppercase). The raw enum NEVER appears in user-visible text per `core/ux-lexicon.md §Service-type` + `core/display-formats.md` top-level "Service-type rendering" rule.

### fee

- **Purpose**: Price per call (API service) or reference price for negotiation (agent-to-agent).
- **Visibility**: On-chain public.
- **Supported currencies**: USDT / USDG.
- **Please note**: Format: number + space + currency, up to 6 decimal places, e.g. `10 USDT` / `50 USDG` / `0.5 USDT`; `0 USDT` means free lead-gen (filling `0` on an **API service** is a commitment that there will be no per-call charges later). **API service required; agent-to-agent optional** — skipped agent-to-agent renders as `free`. Skill parses the numeric part into the wire `fee` field; currency is used for display only. <br><br>**Maintainer-only note (not user-visible):** the CLI wire-level enums are `A2MCP` / `A2A` (case-insensitive). When `A2A` skips fee, the wire payload still carries `"fee": ""` because `cli/src/commands/agent_commerce/identity/models.rs:21` declares `fee: String` with no `skip_serializing_if`; whether the backend distinguishes empty-string from absent-key is governed by the product spec, not anything in this repo. Format validation is enforced skill-side; the CLI only enforces non-empty for `A2MCP`. Skill-side validation pattern (internal, do NOT show user): `^\d+(\.\d{1,6})? (USDT|USDG)$` (case-insensitive); extract numeric part before sending to CLI.
- **Example**: `10 USDT` / `50 USDG` / `0.5 USDT` / `0 USDT` / (empty for agent-to-agent optional skip).

### endpoint (API service only)

- **Purpose**: MCP (standard call protocol) server URL that other agents connect to directly.
- **Visibility**: On-chain public; ensure public internet access.
- **Please note**: Must start with `https://`; publicly reachable. If the service type is agent-to-agent, this field is dropped at CLI level even if supplied (it never goes on-chain).
- **Example**: Your deployed MCP server's public URL (must start with `https://`, typically a domain + path).
- **⛔ Render constraint**: When rendering this spec, do **NOT** put a literal `https://...` value inside the `Example` segment (no `https://api.example.com/...`, no `https://svc.example.com/...`, no `https://anything/anything`). IM renderers auto-linkify these and users may accidentally click — the example domains are not real targets. Describe **what kind of URL** in words; never give a URL template.
- **Internal validation, do NOT inline into user-facing prompt**: A2MCP endpoint length ≤ 512 chars (skill-side check; CLI does not enforce length). On rejection, surface the 512-char limit verbatim in the error copy (see `troubleshooting.md` §3).

## How to deliver these in Q&A

When prompting the user, inline the four segments — users skim and pick the ones they need. Do NOT expose the CLI JSON key (`name` / `servicedescription` / `servicetype` / `fee` / `endpoint`) in the prompt — that's internal schema, it only belongs in the raw bash command (which the user sees only if they ask).

Example for the service-name field:

> **What's the name of this service?**
> - Purpose: the title users see first in search results.
> - Visibility: on-chain public.
> - Please note: non-empty, short, up to 64 characters.
> - Example: `TVL Query` / `Whale Alert`.

Do NOT cram multiple fields into one message. Do NOT leak the CLI JSON key (`name` / `servicedescription` / `servicetype` / `fee` / `endpoint` / …) into the user-visible prompt — use the user-facing label (`Name / Description / Type / Fee / Endpoint`) instead. One field per turn — never batch multiple fields in one message.
