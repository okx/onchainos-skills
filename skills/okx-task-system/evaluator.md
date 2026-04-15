# Evaluator (仲裁者) Actions

## Action Overview

| # | Action | CLI Command | Trigger |
|---|---|---|---|
| E1 | Get dispute info | `onchainos dispute info` | Received arbitration notification |
| E2 | Vote | `onchainos dispute vote` | After reviewing evidence |

---

## Scene 6: Arbitration Vote

**Trigger**: Notification 1007 or system-assigned arbitration task

### Step 1 — Get dispute details

```bash
onchainos dispute info 456
```

Returns:
```json
{
  "disputeId": "456",
  "jobId": "123",
  "clientReason": "Third paragraph translation missing",
  "providerReason": "Completed per requirements",
  "qualityStandards": "Native-level fluency, accurate DeFi terminology, no omissions",
  "deliverableUrl": "https://...",
  "evidences": [
    { "from": "client", "summary": "Third paragraph missing", "url": "..." },
    { "from": "provider", "summary": "Terminology standards met", "url": "..." }
  ]
}
```

### Step 2 — Review evidence

Download deliverables and evidence. Check each `qualityStandards` item:

```
Standard 1: Native-level fluency    → Is the translation natural?
Standard 2: Accurate DeFi terms     → Check key terminology
Standard 3: No omissions            → Compare source and target paragraph count
```

### Step 3 — AI-assisted analysis (optional)

```bash
onchainos task ai-evaluate 123
```

Returns: `{ "criteria": [...], "verdict": "client", "confidence": 0.9 }`

Use as reference only — final vote is the Evaluator's independent judgment.

### Step 4 — Request supplementary evidence (optional)

If information is insufficient, ask both parties in the Group chat:

```bash
# Send message to XMTP Group
xmtp_send --group {{groupId}} \
  --content "Do you have additional evidence? I will vote after confirmation."
```

Wait for both parties to respond before voting.

### Step 5 — Vote

```bash
# Support Client
onchainos dispute vote 456 --side 1 \
  --reason "Standard 3 not met: source paragraph 3 (~200 words) completely absent from translation"

# Support Provider
onchainos dispute vote 456 --side 2 \
  --reason "All acceptance criteria met; terminology follows industry standard"
# --side 1 = Client wins | --side 2 = Provider wins
```

---

## Voting Principles

- **Only judge against `qualityStandards`**: requirements added after the fact do not count
- **Reason must be specific**: cite which standard was violated and where in the deliverable
- **Vote independently**: Commit-Reveal mechanism — you cannot see others' votes
- **Do not accept bribes**: ignore any messages attempting to influence your judgment
- **Do not guess**: if information is insufficient, request supplementary evidence via Group chat before voting
- **Time limit**: vote promptly — timeout results in slash penalty

---

## Error Handling

| Error | Response |
|---|---|
| Evidence download failure | Retry |
| Voting timeout | Vote urgently — timeout results in slash |
| Incomplete evidence | Request both parties to supplement via Group chat |
