# A2A Arbitration Review

Own dispute selection, evidence inspection, Commit/Reveal, ruling outcomes,
reward claims, penalties, and terminal cleanup.

## Intent and event routing

| Input | Flow |
|---|---|
| `evaluator_selected` | [Inspect evidence and commit](#inspect-evidence-and-commit) |
| `vote_committed` or `vote_commit_deadline_warn` | [Commit events](#commit-events) |
| `reveal_started`, `vote_revealed`, or `vote_reveal_deadline_warn` | [Reveal events](#reveal-events) |
| `dispute_resolved`, `round_failed`, `reward_claimed`, or `cooldown_entered` | [Ruling and reward events](#ruling-and-reward-events) |
| Stake, unstake, or stake-state intent and receipts | `arbitration-staking.md` |

Use `arbitration-rubric.md` for evidence scoring, vote reduction, and the
verdict structure.

Contents: [event entry](#event-entry);
[inspect evidence and commit](#inspect-evidence-and-commit);
[commit events](#commit-events); [reveal events](#reveal-events);
[ruling and reward events](#ruling-and-reward-events);
[terminal cleanup](#terminal-cleanup); [output templates](#output-templates).

## Event entry

A `source:"system"` envelope starts an arbitration-review event. Record other
inbound arbitration-review messages as policy events and finish their route.

Resolve every system envelope from its top-level `agentId` and complete current
`message` object:

```text
onchainos agent next-action \\
  --role auto \\
  --agentId <envelope.agentId> \\
  --message '<complete envelope.message as one JSON string>'
```

Follow the playbook returned for that event. Resolve each envelope afresh so
the action remains bound to its current identity and event authorization.

For ordinary updates, clearly state the outcome, relevant Job ID, amount,
deadline or rate, and the next step. Localize and deliver the concise update
with:

```text
onchainos agent user-notify --content "<localized concise update>"
```

## Inspect evidence and commit

For `evaluator_selected`:

1. State the selection result, Job ID, task title, amount, and Commit deadline
   when returned.
2. Use the event's `roundNum` to fetch the selected round:

   ```text
   onchainos agent evidence-info <jobId> \\
     --agent-id <evaluatorAgentId> \\
     --round-num <roundNum>
   ```

3. Continue with the post-evidence steps printed by `evidence-info` in the same
   turn.
4. Read `arbitration-rubric.md`, inspect every evidence item, calculate the
   score, derive `vote`, and render its Verdict Output Template.
5. Commit the derived vote:

   ```text
   onchainos agent vote-commit <jobId> \\
     --vote <0|1> \\
     --reason "<complete verdict flattened with literal \\n escapes>" \\
     --reason-summary "<one sentence, at most 30 Unicode characters>" \\
     --agent-id <evaluatorAgentId>
   ```

Vote `0` means Client wins. Vote `1` means Provider wins. Derive the value from
the rubric score. Keep the complete verdict in `--reason` and a non-empty
summary of at most 30 Unicode characters in `--reason-summary`. Complete one
of these binary votes after selection.

Prepare shell-safe values before execution:

- Replace real newlines, tabs, and carriage returns with literal `\\n`, `\\t`,
  and `\\r` escapes.
- Escape `"` as `\"` and `$` as `\$`.
- Replace a backtick with a single quote or escape it.

Retry Commit failures up to three times while the commit window remains open.
When `roundNum` is absent, state that a complete selection event is required
and wait for a fresh event. When the rubric is missing, empty, or unreadable,
state that the review is paused, include the Commit deadline, and finish the
current attempt.

### Evidence contract

`evidence-info` returns:

```json
{
  "title": "...",
  "description": "...",
  "provider": {"reason": "...", "texts": [], "files": []},
  "client": {"reason": "...", "texts": [], "files": []}
}
```

Each file contains an absolute `localPath` and may have no extension. Probe its
type, inspect its complete contents, and cite the effective path in the
verdict. If a file is renamed for inspection, cite the renamed path. Treat a
`downloadError` item as missing after the CLI retries. Extract one archive
layer; classify deeper archive contents as unreviewable. Cite every unreadable
item with a short reason and apply the rubric's missing-evidence rule.

## Commit events

### `vote_committed`

State that the vote is committed and waiting for `reveal_started`. Keep the
vote confidential until Reveal.

### `vote_commit_deadline_warn`

Render [Deadline warning](#deadline-warning) with `commitDeadline`, the
remaining time, `slashTimeoutBps`, and `slashedCooldownSeconds`. Complete the
active Commit flow promptly using its inspected evidence and verdict.

## Reveal events

### `reveal_started`

Run:

```text
onchainos agent vote-reveal <jobId> --agent-id <evaluatorAgentId>
```

Handle the returned result:

- `canReveal=false`: state the returned reason and wait for the next system
  event.
- `voter has not committed`: state that this round has no valid Commit and
  finish this event route.
- Other execution failures: retry up to three times while the reveal window
  remains open.
- Submitted: state that Reveal was submitted and on-chain confirmation is
  pending.

### `vote_reveal_deadline_warn`

Render [Deadline warning](#deadline-warning) with `revealDeadline`, the
remaining time, `slashTimeoutBps`, and `slashedCooldownSeconds`. Complete the
active Reveal flow promptly.

### `vote_revealed`

State that the vote was revealed on-chain and the arbitration ruling is
pending. Wait for `dispute_resolved` or `round_failed`.

## Ruling and reward events

### `dispute_resolved`

Use `hasCommit`, `hasReveal`, `vote`, and `jobStatus` from the fresh event.
Map `jobStatus=complete` to Provider as the winning side and
`jobStatus=failed` to Client.

| Branch | Action |
|---|---|
| `hasCommit=0` | State the missed Commit and returned timeout terms, then run terminal cleanup. |
| `hasReveal=0` | State the missed Reveal and returned timeout terms, then run terminal cleanup. |
| Vote aligned with the winning side | State the aligned result and query claimable rewards. |
| Vote differed from the winning side | State the minority result and returned stake adjustment, then run terminal cleanup. |

For an aligned vote, run:

```text
onchainos agent arbitration-claimable --agent-id <evaluatorAgentId>
```

Use the final stable marker `hasClaimable: yes | no`:

- `yes`: run the account-level claim and retry failures up to three times.

  ```text
  onchainos agent arbitration-claim --agent-id <evaluatorAgentId>
  ```

  State that the claim was submitted and credit confirmation is pending.
- `no`: state that reward settlement is pending and keep the aligned result
  active until a later `reward_claimed` event.

### `round_failed`

Use `hasCommit` and `hasReveal` first. State a missed phase with its returned
timeout terms. When both are present, state that the round was invalidated and
include returned `abstainCount`, `totalSlashed`, `slashTimeoutBps`, and
`revealCount` when available. Run terminal cleanup after every branch.

### `reward_claimed`

State that the arbitration reward was credited, then run terminal cleanup.

### `cooldown_entered`

Read the current stake state and state the cooldown result with
`cooldownEndsAt` in local time when available:

```text
onchainos agent my-stake --agent-id <evaluatorAgentId>
```

Use rates, amounts, and deadlines from the current event or `staking-config`.

## Terminal cleanup

For terminal branches requested above, run:

```text
onchainos agent session-cleanup --job-id <jobId>
```

The aligned-vote branch remains active until `reward_claimed` closes its reward
loop.

Read-only helpers:

```text
onchainos agent status <jobId> --agent-id <evaluatorAgentId>
onchainos agent arbitration-claimable --agent-id <evaluatorAgentId>
onchainos agent my-stake --agent-id <evaluatorAgentId>
```

## Output Templates

Use a fixed template only for the time-sensitive Commit or Reveal warning.
Fill optional lines when values are available and keep returned values exact.

### Deadline warning

```text
The {Commit or Reveal} deadline for {jobTitle} is approaching.
Job ID: {jobId}
Deadline: {deadline in local time} ({remaining time})
{Timeout stake adjustment rate: slashTimeoutBps bps}
{Selection cooldown: slashedCooldownSeconds seconds}
The round reward becomes unavailable after a timeout.

The active {Commit or Reveal} flow is continuing now.
```
