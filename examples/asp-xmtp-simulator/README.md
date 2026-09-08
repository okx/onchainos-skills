# ASP runtime read-only dashboard

This local dashboard observes ASP identities, subscription jobs, A2A sessions,
and historical diagnostics. It does not own any ASP lifecycle action.

Open `http://127.0.0.1:4319/` after starting it:

```bash
node examples/asp-xmtp-simulator/server.mjs
```

The read-only endpoints are:

- `GET /health`
- `GET /api/asps`
- `GET /api/jobs`
- `GET /api/events`

The former mutation endpoints `POST /dispatch`, `POST /signal`, and
`POST /api/subscriptions/accept` return HTTP 405. The compatibility
`codex-dispatcher.mjs` wrapper is now transparent and never synthesizes an A2A
message.

## Authoritative subscription lifecycle

The ASP runtime handles the backend events itself:

1. `sub_open`: Lark §1.3 provider decision. Fetch authoritative detail and run
   exactly one `ACCEPT`, `NEED_PARAMS`, or `REJECT` path.
2. After acceptance, the backend sends `sub_created` to the User runtime and
   `sub_asp_selected` to the ASP runtime.
3. `sub_asp_selected`: Lark §1.5 acceptance-confirmed stage. Start the exact
   registered Service workflow and pass ready output to formal §1.6 delivery.

The Lark document calls the §1.5 stage `sub_accepted`; the current backend event
name received by the ASP is `sub_asp_selected`, so the runtime must not wait for
a separate `sub_accepted` event.

Formal delivery must use the ASP runtime's `onchainos agent deliver` flow. The
dashboard must not call raw `okx-a2a xmtp-send`; doing so can duplicate messages
and unnecessarily consume XMTP identity API rate limits.

## Daemon setup

Run the local daemon directly with its normal Codex provider. Do not set
`OKX_A2A_AI_CODEX_COMMAND` to this example's dispatcher:

```bash
okx-a2a daemon restart --provider codex --no-autostart
```

If an older shell or service definition still exports
`OKX_A2A_AI_CODEX_COMMAND`, remove that override before restarting.
