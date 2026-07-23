---
name: okx-device-id
description: "Explains the onchainos device-id — the stable per-machine identifier the CLI attaches as a `device-id` HTTP header on every outbound request (market, wallet, agent-commerce, task) across JWT, AK, and anonymous auth. Use when the user asks what the device-id is, how it is generated or derived, where it is stored, whether onchainos tracks or fingerprints their machine, what identifying data the CLI sends in request headers, how to reset or regenerate it, or about the privacy of this identifier. Trigger keywords: 'device id', 'device-id header', 'device identifier', 'machine id', 'machine identifier', 'am I being tracked', 'device fingerprint', 'what headers does onchainos send', 'reset device id', 'regenerate device id', '设备id', '设备标识', '机器id', '机器标识', '是否被追踪', '设备指纹', '重置设备id', '重新生成设备id'."
license: MIT
metadata:
  author: okx
  version: "4.3.0"
  homepage: "https://web3.okx.com"
---

# Onchain OS — Device ID

The `device-id` is a stable, per-machine identifier the onchainos CLI attaches as a `device-id` HTTP request header on every outbound call, so the backend can correlate requests coming from the same machine. It is informational only: there is no `onchainos device-id` subcommand — the CLI never exposes it to display, reset, or manage directly. Answer questions about it from this skill; there is no command that prints the value.

## Pre-flight Checks
Before your first onchainos command, read `../okx-agentic-wallet/_shared/preflight.md` once. If it does not exist, read `_shared/preflight.md` instead.

## Intent Routing

Match the user's question to a row and answer from the linked section. There is no CLI command that prints the device-id, so never fabricate one.

| User intent | Answer from |
|---|---|
| "what is the device-id" / "what data does onchainos send in request headers" | § What it is |
| "how is it generated / derived" / questions about sha256, machine-id, or the uuid fallback | § How it is derived |
| "where is it stored" / keyring / persistence across runs | § Storage |
| "am I being tracked or fingerprinted" / "is this a privacy risk" | § Privacy and security |
| "how do I reset or regenerate the device-id" | § Reset and regenerate |

## What it is

- A single machine-scoped value sent as the `device-id` request header on **every** outbound request, across all command areas (market, wallet, agent-commerce, task, swap, portfolio, defi) and all auth modes (JWT, AK, anonymous). It is injected once at the shared header builder, so no request path bypasses it.
- Format: either a 64-character lowercase-hex string (the normal, machine-derived path) or a 36-character UUIDv4 with hyphens (the fallback path). It is always pure ASCII.
- **MUST**: treat the device-id as the same value regardless of `--chain` or account — it is machine-level, not per-chain or per-account, so never imply a different id per network or per wallet.

## How it is derived

- Normal path: `device_id = lowercase_hex(sha256(machine-id + "onchainos"))`, where `machine-id` is the OS-provided machine identifier and `"onchainos"` is a fixed namespace suffix concatenated directly with no separator.
- Fallback path: when the OS machine-id is unreadable (headless host, container, unusual platform), a random UUIDv4 is used instead.
- **NEVER**: state or imply that the raw machine-id is sent to the backend — only the one-way sha256 derivative leaves the machine, and the raw machine-id is never logged, stored, or transmitted. Saying otherwise would misrepresent the privacy design.
- The `"onchainos"` namespace suffix is fixed. **NEVER**: tell a user to change it — it is a source-level constant with no user control, and changing it would re-derive every device-id globally and break backend correlation.

## Storage

- Persisted in the OS keyring under the key `device_id`, inside the same unified blob as other onchainos credentials. The keyring uses a three-tier fallback: OS keyring first, then an encrypted file under the app home, then RAM-only for the current process. The home directory follows `ONCHAINOS_HOME` (default `~/.onchainos`).
- Computed once per process and memoized: the first outbound request triggers computation, and later requests reuse the cached value with no extra work.

## Privacy and security

- **SHOULD**: answer a privacy-concerned user with the accurate design facts rather than speculation:
  - The backend receives only `sha256(machine-id + "onchainos")`, never the raw machine-id.
  - The namespace suffix isolates this hash from any other service that hashes the same machine-id, so the value cannot be cross-correlated with a service that does not use the same namespace.
  - The device-id is **not** a secret or a trust credential — it is forgeable by design and is used for identity reporting only, never for authentication or authorization.
- Best-effort by design: if computation fails, the header is simply omitted and the request still proceeds normally. **NEVER**: describe a missing device-id header as an error — omission is the intended fallback, not a fault, so the CLI never exits non-zero or prints a user-facing error because of it.
- Known limitation: two clones of the same disk or VM image can share a machine-id and therefore the same device-id. State this plainly if asked — it is a documented non-goal for this iteration, not a bug.

## Reset and regenerate

- There is no dedicated reset command. The device-id is regenerated only when the stored `device_id` keyring value is cleared (for example via `onchainos wallet logout`, or a manual keyring purge); the next request then re-derives and re-persists it. For wallet or keyring operations, route to the `okx-agentic-wallet` skill.
- **MUST**: set the right expectation about the regenerated value — on a machine whose OS machine-id is readable, the re-derived value is **identical** to the previous one because the derivation is deterministic, so clearing the keyring does not produce a new id there. Only the UUIDv4 fallback path yields a different value on regeneration. Do not promise a fresh id on reset.

## Global Notes

- The device-id never appears in stdout, `--format json` output, MCP tool envelopes, or the audit log — it is a request header only. **NEVER**: look for it in command output or claim a command can print it, because no such surface exists.
