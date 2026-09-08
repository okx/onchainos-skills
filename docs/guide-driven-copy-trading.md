# Guide-driven copy-trading flow

## Local records

For every subscription `jobId`, the CLI stores two Markdown documents under
`ONCHAINOS_HOME` before subscription broadcast:

- `autotrade/guide/<jobId>.md`: immutable service Guide text plus trusted metadata.
- `autotrade/consent/<jobId>.md`: user-confirmed values, bound to the Guide
  contract hash. It begins as `prepared` and becomes `active` only after
  broadcast succeeds.

Both documents use a machine-readable metadata comment and human-readable
Markdown body. The Guide contract hash covers the verbatim Guide text, so
changing it invalidates an existing Consent. The CLI writes them through the secure local-file helper; no
Guide or Consent contents are logged.

## Subscription

1. The caller provides the exact service Guide and explicit user-confirmed Consent with the subscription request.
2. No semantic declaration is created or persisted.
3. After `/create` returns `jobId`, but before signing/broadcasting, the CLI writes the local Guide and prepared Consent records.
4. A successful broadcast activates Consent. Broadcast failure marks the prepared Consent aborted.

The platform does not impose business fields. The runtime Agent reads the Guide,
Consent, and Signal together; Consent stores only its lifecycle, Guide hash,
expiry, and user-confirmed values.

## Signal handling

1. Persist the provider Signal exactly as received. It may be plain text,
   Markdown, or JSON; its raw format is not a CLI contract.
2. Load the saved Guide, active Consent, and raw Signal by `jobId`; verify that
   the Consent is bound to the current Guide contract.
3. The runtime Agent applies the Guide to the raw Signal using only the matching
   Consent. If information is incomplete, ambiguous, expired, duplicated, or
   outside the Guide's limits, it fails closed to notification.
4. Immediately before one documented trusted-tool call, direct claim verifies
   the active Guide+Consent contract and reserves the delivery exactly once.

The Guide may describe a documented trading tool, but cannot authorize a shell
command or arbitrary script path. This keeps provider policy flexible without
making provider text executable.
