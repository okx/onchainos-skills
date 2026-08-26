# Update an Agent identity

Use for changes to an existing Agent's name, description, picture, or services.

## Necessity check

1. Run `get-agents` before collecting any change and render the current `card[]`.
2. End if the identity does not belong to the current wallet.
3. Run `service-list` for an existing service update/delete to obtain its `id`.

## Commands

Use the commands named in this file; load `identity-cli-reference.md` before execution.

## Workflow

1. **Collect.** For service changes, follow `identity-service-contract.md` §Collect and route.
2. **Validate.** For ASP changes, follow `identity-validate-listing.md` Update mode.
3. **Review.** Show changed fields with their current and new values. For services, follow
   `identity-service-contract.md` §Display.
4. **Confirm.** Obtain fresh explicit confirmation for the final diff.
5. **Execute.** Run `agent update` once and render the response.

## Service delta examples

After QA and confirmation, use the matching service delta:

```bash
# Create: A2MCP
onchainos agent update --agent-id 42 --service '[{"operation":"create","serviceName":"Price Feed","serviceDescription":"Returns token price quotes\nsymbol(string, required): token symbol\nGET\ncurl https://<your-deployed-host>/price?symbol=ETH","serviceType":"A2MCP","fee":"10","endpoint":"https://<your-deployed-host>/price"}]'

# Update: A2A subscription
onchainos agent update --agent-id 42 --service '[{"operation":"update","id":"7","serviceName":"Market Signals","serviceDescription":"Provides market signals for onchain traders","serviceGuide":"Choose a market and submit your risk limit.","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"12"}]}]'

# Delete
onchainos agent update --agent-id 42 --service '[{"operation":"delete","id":"9"}]'
```

## Result

On success, emit `Update saved.`

On CLI error or non-success, load `identity-errors.md`.
