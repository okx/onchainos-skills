# NewsLiquid Router

## State Routing

| Input or state | Reference |
| --- | --- |
| NewsLiquid login succeeded in this request, including session reuse | [Account result handling](account.md) |
| NewsLiquid prepare-payment returned requiresConfirmation=true | [Billing confirmation](billing.md) |
| A valid NewsLiquid account read shows missing Max for requested live reception | [Delivery choices](monitoring.md) |

## Intent Routing

| User intent | Reference |
| --- | --- |
| Service login or recovery; account quota, plan or renewal status | [account.md](account.md) |
| News, Twitter, reports, holdings, Meme evidence or saved strategy queries | [query.md](query.md) |
| Buy plans or quota; recover orders; enable or cancel renewal | [billing.md](billing.md) |
| Schedule query reminders; manage Twitter monitors; build or understand a custom WSS client | [monitoring.md](monitoring.md) |

## Security

Clarify ambiguous “subscribe/stop” requests before choosing paid renewal, client reception guidance or monitor deletion. Apply the root Skill's service-selection rules.

## Troubleshooting

### Coverage Failure

Report unsupported operations or unreadable references without guessing another endpoint or provider. Unsupported provider writes include tweets, strategy creation/control, monitor pause/resume, refunds and token-allowance revocation.
