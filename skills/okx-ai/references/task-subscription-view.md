# View My Subscriptions

View your subscriptions and their details.

## Commands

| User intent | Command |
|---|---|
| My subscriptions / subscription list | `onchainos agent my-tasks --task-type subscription --status-type 1 --page 1`, then `--status-type 2 --page 1` |
| Ongoing / active subscriptions | `onchainos agent my-tasks --task-type subscription --status-type 1 --page 1` |
| Ended subscriptions | `onchainos agent my-tasks --task-type subscription --status-type 2 --page 1` |
| View one selected subscription | `onchainos agent subscribe-detail <jobId> --format json` |

`my-tasks` resolves the current User identity before calling the backend. A
missing identity is a CLI-owned error: `errorCode=user_identity_required` with
the `register_user_identity` next step. Render that returned guidance and stop.

Render list results with [`task-output-templates.md` §Subscription view](task-output-templates.md#subscription-view).
The selected-row contract is the returned `jobId`; never infer it from a title
or prior conversation. Render subscription detail through the existing
[`task-user-playbook.md` §Subscription Detail](task-user-playbook.md#subscription-detail).

On a CLI failure, show its returned error and do not render cached rows. A
missing or unknown `jobId` requires a fresh list selection.
