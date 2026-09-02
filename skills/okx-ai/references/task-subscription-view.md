# View My Subscriptions

View your subscriptions and their details.

## Intent and command

| User intent | Command |
|---|---|
| My subscriptions / subscription list | `onchainos agent my-tasks --task-type subscription --status-type 1 --page 1`, then `--status-type 2 --page 1` |
| Ongoing / active subscriptions | `onchainos agent my-tasks --task-type subscription --status-type 1 --page 1` |
| Ended subscriptions | `onchainos agent my-tasks --task-type subscription --status-type 2 --page 1` |
| View one selected subscription | `onchainos agent subscribe-detail <jobId> --format json` |

`my-tasks` resolves the current User identity before it calls the backend. If
it reports that no User identity exists, show the identity guidance and stop;
do not fall back to provider data or another account's history.

For the unfiltered list, render the Active result first and the ended result
second. They are separate fact reads, so retain pagination independently for
each section. A user asking for the next page must specify the section when
both have another page.

## List result

Pass each command's structured result to
[`task-output-templates.md` §Subscription view](task-output-templates.md#subscription-view).
Use only the returned `jobId` to identify a selected row; titles and prior
conversation text are never identifiers.

The list is display-only. Its optional next steps are:

1. View a selected subscription detail.
2. Manage message-receipt devices for a selected Active subscription.
3. View latest signals for a selected Active subscription.
4. View copy-trade status for a selected subscription.

Only step 1 is executed from the list selection itself. Steps 2–4 require a
new explicit user request and must load their owning reference. In particular,
viewing latest signals uses the existing scoped signal-receipt flow, and must
not begin a global watch or infer a `jobId`.

## Detail result

Call `subscribe-detail` with the selected row's `jobId` and render the current
facts with the existing Subscription Detail template. A detail read is not
authorization to modify the subscription. If it is no longer Active, do not
offer signal receipt or device-management actions.

## Recovery

- List/detail transport or parsing failure: explain that current subscription
  data could not be read and offer a retry; do not use cached rows as current
  state.
- An unknown or missing `jobId`: ask the user to choose a row from a fresh
  list; never guess from a title.
- An empty section: render the section's empty-state copy only. Do not invent
  subscriptions or action choices.
