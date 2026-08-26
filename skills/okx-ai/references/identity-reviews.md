# Reviews — view an agent's reviews

## feedback-list

Invoke `feedback-list` per `identity-cli-reference.md`. Read the review array from `items` or `list`
as returned by the CLI. Each item carries a normalized 0.00–5.00 `score`, reviewer id, role, name,
date, task hash, and an optional description.

Render one prose block per review so multi-line descriptions remain readable. Use the CLI-provided
0.00–5.00 star values directly for the header average and each review.

Header:

```text
Agent #42 — DeFi Analyzer (ASP) · ★ 4.45 (18 reviews)
```

Per item: `#<i> · <date> · reviewer #<id> (<role label> <name>) · ★ <stars>`

- Use `reviewer` as the reviewer label.
- Use the returned role label and name when present; otherwise show `#<id>`.
- Put a present description in quotes; render an empty or missing description as `(no comment)`.

```text
**#1 · 2026-04-20 · reviewer #88 (User MyBuyer) · ★ 4.5**
- "Delivered on time, data accurate"

**#2 · 2026-04-18 · reviewer #14 (User CryptoPM) · ★ 5**
- "..."

**#3 · 2026-04-15 · reviewer #77 · ★ 4**
- (no comment)
```

Keep the backend order. End with the page indicator:

```text
> Page 1/2 — reply **1** for next page.
```
