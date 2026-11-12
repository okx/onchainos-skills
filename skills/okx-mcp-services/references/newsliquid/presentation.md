# NewsLiquid Presentation

## Module Description

Apply these service-specific rules when showing NewsLiquid results. Purchase confirmations use the billing templates.

## Output Rules

### Connection notice

| Result in this request | Notice |
| --- | --- |
| Successful service login/reconnection, including explicit reuse of a valid session | Append once after the requested result |
| Login failure, local status read, ordinary query or WSS handshake/ACK alone | No new connection notice |

For login plus a query, show the query's actual result or failure before the notice. For login only, report the new/reused session first.

Canonical provider wording; translate faithfully into the user's language:

```text
NewsLiquid is a third-party service. OKX does not guarantee the accuracy of the information it provides. Please be aware of the risks.
```

When the platform analysis reminder is also due, combine both in one footer. The notice requires no separate acceptance.

### Service results

- Translate labels and prose into the user's language. Preserve actual values, IDs and protocol fields.
- Show title/content summary, actual source, publication time and available original link. Fall back from source to `newsType`, then “source not provided”; do not invent a URL.
- Label scores/directions as service analysis. Separate Meme opinion from evidence. Label report periods and holdings snapshot dates; snapshots are not live positions or individual trades.
- Show actual `usage.cost` when returned. Quota is points, not dollars or an estimated number of messages. Show held-but-exhausted categories as 0; omit categories never held. Total quota cannot pay a cost by combining categories.
- Use the user's known timezone; otherwise label UTC. Do not promise the free-quota refresh in a local timezone that the service has not verified.
- Distinguish no results, partial results, extraction failure, pending payment, accepted subscription and received business messages.
- Purchase selection and payment confirmation use the billing templates. Do not replace them with a compact summary of raw quote fields.
