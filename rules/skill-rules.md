# Skill Rules

Rules for writing and changing Skills in this repository. Read this file and the
matching template in [`templates/`](../templates/) before you edit any Skill file.
Page structure, allowed headings, and route-table formats are defined by
[`templates/README.md`](../templates/README.md); if this file and a template differ
on structure, the template wins.

A Skill is an instruction document for an AI agent, not a document for people.
It is loaded into the agent's context, so every token competes with the
conversation. Write it correctly and write it short.

The IDs in bold (for example **SKILL-04**) are labels you can use in reviews and
merge request discussions. `[P0]` and `[P1]` mark priority; `[Safety]` marks
rules that protect users' funds or data.

## How a Skill is loaded

- The `description` is always loaded in every session. The agent uses it to decide
  which Skill to use, so one extra character costs every session.
- The body of `SKILL.md` is loaded only after the Skill is chosen.
- Files under `references/` are loaded only when the body points to them and they are needed.

## Creating a Skill and writing its metadata

- **SKILL-01 [P1] Create a new Skill only when needed.** First try to change an API
  or reuse an existing Skill. Skills with overlapping duties compete to be chosen.
  For example, onboarding help, customer-support help, and how-to-play help all
  belong in the one `okx-guide` Skill, not in several small Skills.
- **SKILL-02 [P0] Name.** At most 64 characters; lowercase letters, digits, and
  hyphens only; no XML tags; no reserved words such as `anthropic` or `claude`;
  kebab-case. A gerund form (verb + "-ing") is recommended; a noun phrase or an
  action-oriented name is also acceptable. Example: `okx-dex-market`. Names such
  as `helper` or `utils` are too generic.
- **SKILL-03 [P0] Description content.** Not empty, at most 1024 characters, no XML
  tags, third person or imperative ("Use when ..."). Say what the Skill does, when
  to use it, and the concrete scenarios. The description must not start with the
  Skill name. A vague description ("Helps with crypto") or first person ("I can
  help you ...") makes the Skill hard to choose correctly.
- **DESC-03 [P0] Triggers and key boundaries only.** Do not put flows, command
  lists, or long examples in the description. Good: "Use for on-chain market data:
  token prices, K-line/OHLC candles, index price, wallet PnL." Bad: "First run X,
  then call quote, show it to the user, then call execute, ...".
- **DESC-04 [P0] No angle-bracket placeholders** such as `<symbol>` in a
  description. Some runtimes parse them badly and they do not help routing.
- **DESC-06 [P1] Start with a positive capability statement,** not with "HARD
  BLOCK" or "Do NOT". Negative openings crowd out the trigger words.
- **DESC-07 [P1] No "Not for X, see Y" rules in descriptions.** When two Skills
  overlap, fix it by redrawing the boundary between them, and keep the description
  for positive trigger words. Existing "Not for" rules may stay during a transition
  (at most one per description) and must be deleted once the boundary is clean.
  A description with several of them is a sign that the boundary needs redrawing.

## Body and references

- **SKILL-04 [P0] Body size.** Keep `SKILL.md` at or below 500 lines and write only
  what the model does not already know. Do not explain general knowledge.
- **SKILL-05 [P0] Progressive disclosure.** Keep frequent content inline. Move long
  or rarely used content (full parameter lists, return fields, error-code tables)
  to `references/`, and say when to read each file. Link every reference directly
  from `SKILL.md` (one level deep, no A to B to C chains). A directly linked
  reference that is deeper than one level and longer than 100 lines starts with a
  table of contents; `SKILL.md` itself does not. Organize folders by domain, give
  files self-explanatory names, and always use forward slashes in paths.
- **SKILL-06 [P0] No cycles, no dead files.** References link out from `SKILL.md`
  only and do not link back and forth to each other. Every reference file must be
  linked from `SKILL.md`.
- **REF-04 [P1] Examples are real commands.** An example must be a real command
  that can run, for example `onchainos token search --query PEPE --chain eth --format json`.
  A made-up command will be copied and executed by the agent.

## Writing style

- **SKILL-07 [P0] Give a default.** For choices such as slippage or gas level, state
  one default and the condition for deviating, instead of listing every option.
- **SKILL-08 [P0] Use consistent words.** Use one term per concept. For example,
  refer to a blockchain by its human-readable chain name together with one fixed
  chain-index mapping table, and do not mix "chain", "network", and "chainIndex"
  for the same thing. Different words make the model think they are different things.
- **SKILL-09 [P0] No XML tags for emphasis.** Use `**MUST**:` or `**NEVER**`
  instead of `<MUST>` or `<IMPORTANT>`; XML tags render badly in previews.
- **SKILL-10 [P0] No decorative emoji in Skill text.** Emoji are allowed only inside
  output templates that are shown to users.
- **SKILL-11 [P1] Explain why.** Give the reason behind a prohibition so the agent
  can handle cases the rule did not list. Example: never guess a token contract
  address, because the same symbol has different addresses on different chains.
- **SKILL-12 [P0] Prefer scripts over improvisation.** Put deterministic, reusable
  operations in a script or the CLI instead of asking the agent to assemble them
  (for example RPC calls, signatures, or calldata). Handle errors in the script and
  avoid magic constants.
- **SKILL-13 [P1] Break complex tasks into ordered steps** and add a
  check-fix-retry loop for critical operations.

## Content quality

- **SKILL-14 [P0] A clean-up must not change behavior.** A clean-up only changes
  structure or wording. If a change alters behavior, put it in a separate commit
  and mark it as a behavior change; otherwise reviewers will assume nothing changed.
- **SKILL-15 [P0] Handle failures in one place.** List the failure cases first and
  handle them together (for example in a troubleshooting reference), so the normal
  path stays as short as possible.
- **SKILL-16 [P0] Language.** Write the description in English. Chinese slang that
  has no reliable English equivalent may be listed in Chinese as trigger words, to
  improve matching for Chinese users. Do not add a Chinese copy of an ordinary
  English keyword, and do not mix both languages for the same keyword.
- **SKILL-17 [P0] Show amounts by audience.** Show users readable values with a fiat
  estimate; pass machine and on-chain values in raw minimal units.
- **SKILL-18 [P1] Report failures with a structured diagnosis,** not only an error
  code: include the transaction hash, chain, parameter snapshot, wallet, and time
  when available. This is for human troubleshooting and does not have to reuse the
  CLI output field names.
- **SKILL-20 [P0] Shared preflight.** Each `SKILL.md` points to the shared
  `_shared/preflight.md` instead of carrying its own copy of the install and
  checksum steps, so a change is made in one place. Use these fixed sentences:
  - In `okx-agentic-wallet/SKILL.md`: "Preflight checks: At the start of each
    thread, complete the checks in `_shared/preflight.md`." The file name is a
    Markdown link to that file.
  - In every other `SKILL.md`: "Preflight checks: At the start of each thread,
    complete the checks in `../okx-agentic-wallet/_shared/preflight.md`. If
    missing, read `_shared/preflight.md`."

  No Reference page may describe the onchainos update or preflight procedure.

## Ownership and change control

- **GEN-02 [P0] One owner per capability.** A rule shared by several Skills is
  written in one place and referenced elsewhere. The same user intent must not be
  claimed by two Skills. Example: define the 402 split once in the routing table
  (market-API quota 402 goes to `okx-dex-market`; payment-protocol 402 such as
  x402, MPP, or a payment ID goes to `okx-agent-payments-protocol`) and do not
  repeat it in each description.
- **GEN-03 [P0] Changing a Skill is changing code.** Many Skills compete for the
  same user requests, so a change to one description can break the routing of a
  neighbor, and this is hard to see by reading. A routing baseline is a fixed set of
  example user requests, each with the Skill that should be chosen. Before you
  merge a change to a description, a boundary, or a flow, run the baseline and make
  sure it passes. Record the result in the merge request description (for example
  "routing baseline 24/24 correct, including 5 adversarial cases"). If no baseline
  exists for the affected Skills, say so and describe how you checked routing.
- **GEN-06 [P0] Structural changes need a proposal first.** Splitting, merging, or
  renaming a Skill needs a written plan with all five parts: the current problem,
  the new boundary, the migration path, the validation set, and the rollback
  method. Update every routing table or list that mentions the Skill in the same
  change; a Skill that is missing from the routing is installed but never routed to.

## Skills and the CLI

- **GEN-04 [P0] Move deterministic logic into the CLI or a script.** Validation,
  parameter assembly, multi-step orchestration, and state guards belong in the CLI.
  The Skill keeps only routing, judgment, and the order of operations. Example: one
  command returns the wallet, identity, and communication checks, instead of a
  script that makes the model run three commands and combine the results. When you
  add a multi-step flow, ask first whether one command could do it.
- **CLI-01 [P0] New capability: CLI first, then Skill.** Add a stable on-chain
  operation as a CLI subcommand first. If a capability can only be done by
  assembling HTTP calls or complex scripts, add it to the CLI. The Skill then only
  says when to call it.
- **CLI-02 [P0] Every command in a Skill must exist.** Each `onchainos ...` command
  in a `SKILL.md` or reference must already be implemented or be added in the same
  change. The reverse also holds: each subcommand should have a documentation line.
  Check both directions before you submit (list the subcommands with `--help`, and
  confirm each documented command with `--help`).
- **CLI-03 [P0] Do not teach the agent internal HTTP.** Tell the agent to call a CLI
  subcommand, not to send requests to internal endpoints with custom headers.
- **CLI-04 [P0] Keep the four artifacts in sync.** When a CLI parameter or output
  changes, update `SKILL.md`, the reference, the MCP schema, and the tests in the
  same change.
- **CLI-06 [P1] Give the full command form.** Show the complete parameters, for
  example `onchainos wallet send --to <address> --token USDT --readable-amount 0.1 --chain base`,
  so the agent does not have to discover them with `--help`.

The CLI input/output contract itself is in [`cli-io-rules.md`](cli-io-rules.md).

## Safety

- **SEC-01 [Safety] Confirm before write operations.** For a trade, transfer,
  approval, signature, payment, broadcast, order, stake, redeem, or reward claim,
  confirm with the user first: operation type, chain, token, amount, recipient or
  contract address, fee, slippage, expiry, and tracking ID. A Skill that changes
  funds must state in its body what to confirm before execution. Operations that do
  not touch funds are exempt.
- **SEC-02 [Safety] Never fabricate.** Do not invent CLI commands, APIs, protocol
  fields, contract addresses, chain names, plugin names, yields, or security
  conclusions. A token address must come from the user or from CLI output, never
  from memory.
- **SEC-04 [Safety] Be restrained about safety conclusions.** A safety check can
  show that something is risky, not that it is safe. Do not endorse a token; block
  clearly on high risk. Example: "No known risk items found (this does not mean it
  is safe); honeypot check: not matched."
