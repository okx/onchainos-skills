# Guide-driven copy-trading flow

## Local records

For every subscription `jobId`, the CLI stores two Markdown documents under
`ONCHAINOS_HOME` before subscription broadcast:

- `autotrade/guide/<jobId>.md`: immutable service Guide text plus trusted metadata and a bounded semantic declaration.
- `autotrade/consent/<jobId>.md`: user-confirmed values, bound to the Guide
  contract hash. It begins as `prepared` and becomes `active` only after
  broadcast succeeds.

Both documents use a machine-readable metadata comment and human-readable
Markdown body. The Guide contract hash covers both the verbatim Guide text and
its semantic declaration, so changing either invalidates an existing Consent or
Signal resolution. The CLI writes them through the secure local-file helper; no
Guide or Consent contents are logged.

## Subscription

1. The caller provides the service Guide and a semantic declaration with the subscription request.
2. The declaration defines the service-specific Consent fields, Signal fields, bounded `toolId`, operation, conditions, and parameter bindings.
3. After `/create` returns `jobId`, but before signing/broadcasting, the CLI writes the local Guide and prepared Consent records.
4. A successful broadcast activates Consent. Broadcast failure marks the prepared Consent aborted.

The platform does not impose business fields. Field names, types, requirements,
and transformations come exclusively from the Guide semantic declaration. Consent
stores only its lifecycle, Guide hash, expiry, and the Guide-declared values.

## Signal handling

1. Persist the provider Signal exactly as received. It may be plain text,
   Markdown, or JSON; its raw format is not a CLI contract.
2. Load the saved Guide, active Consent, and raw Signal by `jobId`; verify that
   the Consent is bound to the current Guide contract.
3. Read the Guide's declared Signal fields and extract a small typed projection
   from the raw Signal. The projection is supplied to
   `autotrade-guide-intent-resolve` as JSON, but it is **not** a requirement on
   the raw Signal.
4. The resolver validates only the Guide-declared fields, applies its
   conditions and bindings, and stores
   `autotrade/guide-resolution/<jobId>/<deliveryId>.json`. That record is bound
   to the exact raw Signal bytes, active Consent, and Guide contract hash.
5. If ineligible or incomplete, fail closed to notification. Otherwise direct
   claim recomputes the exact resolved intent and binds its hash, selected tool,
   operation, and Guide-declared authorization parameter before the selected
   tool runs.

The Guide may select an existing tool (`onchainos`, `trade_kit`, `polymarket_plugin`, or `hyperliquid_plugin`), but cannot supply a shell command or arbitrary script path. This keeps service-defined semantics flexible without making provider text executable.
