---
name: okx-a2a-legacy-tool-bridge
description: Use when an OKX A2A / okx-agent-task legacy playbook expects native OpenClaw xmtp_* tools such as xmtp_send, xmtp_start_conversation, xmtp_prompt_user, xmtp_dispatch_user, xmtp_dispatch_session, xmtp_get_conversation_history, or xmtp_sessions_query, but the current AI environment does not expose those tools. This skill provides a compatibility bridge that preserves the legacy tool parameter shape and maps calls to the local okx-a2a CLI.
metadata:
  short-description: Bridge legacy xmtp_* tools to okx-a2a CLI
---

# OKX A2A Legacy Tool Bridge

Use this skill only as a compatibility shim for old `okx-agent-task` skills/playbooks that still mention native OpenClaw `xmtp_*` tools.

This bridge is a transport/runtime replacement for missing native `xmtp_*` tools. It does not change the role, authorization, or session-state legality of the original tool call.

## Activation Rule

1. If the current environment exposes native tools named `xmtp_send` and `xmtp_start_conversation`, use the native tools and do not use this bridge.
2. If a playbook says to call any supported native `xmtp_*` tool and that native tool is absent, unavailable, or fails with "unknown tool" / "not found", call the bridge script immediately with the same parameters.
3. Do not ask the user whether to use this bridge. If the original native tool call is legal for the current session state, the bridge call is the required replacement.
4. If the original native tool call would be forbidden for the current session state, the bridge call is also forbidden. Do not use this bridge to bypass user-session / sub-session boundaries.
5. Keep the original legacy parameter names. Pass them as JSON to the script.

Bridge script: `scripts/xmtp-tool.js`, resolved relative to this `SKILL.md`.

Bridge invocation, from this bridge directory:

```bash
node scripts/xmtp-tool.js <toolName> '<jsonParams>'
```

Example:

```bash
node scripts/xmtp-tool.js xmtp_start_conversation \
  '{"myAgentId":"1092","toAgentId":"956","jobId":"0xabc"}'

node scripts/xmtp-tool.js xmtp_send \
  '{"sessionKey":"job:0xabc:my:1092:to:956","content":"Hello"}'
```

## User Decision Relay

When `okx-agent-task` / `_shared/user-message-flow.md` says to relay a user's completed decision to a task session, the expected native tool is `xmtp_dispatch_session`.

If native `xmtp_dispatch_session` is unavailable, call the bridge immediately:

```bash
node scripts/xmtp-tool.js xmtp_dispatch_session \
  '{"sessionKey":"<target-sub-session-key>","content":"[USER_DECISION_RELAY] <verbatim user reply>"}'
```

This is not a new user decision and not a request to simulate the sub session. The user has already decided; the only remaining action is to relay the decision to the target session.

Provider ids and option numbers are still just the user's verbatim decision. If the user replies `956`, `1`, `选956`, `关闭`, or similar to an active decision card, do not reinterpret it as a fresh negotiation command.

Do not replace this relay with `xmtp_start_conversation`, `xmtp_send`, `okx-a2a session create`, `okx-a2a xmtp-send`, or `onchainos agent next-action`. Those tools are for task/session execution after the target sub session receives the relay, not for forwarding a user decision from the user session.

## Supported Mappings

Load [references/tool-mapping.md](references/tool-mapping.md) when you need exact parameter mapping or a legacy tool is not listed below.

- `xmtp_start_conversation` -> `okx-a2a session create`; the actual XMTP group is created/reused on the first `xmtp_send`.
- `xmtp_send` -> `okx-a2a xmtp-send`.
- `xmtp_prompt_user` -> `okx-a2a user decision-request`.
- `xmtp_dispatch_user` -> `okx-a2a user notify`.
- `xmtp_dispatch_session` -> `okx-a2a session send --no-wait`.
- `xmtp_get_conversation_history` -> `okx-a2a session get`.
- `xmtp_sessions_query` -> `okx-a2a session query`.
- `xmtp_delete_conversation` -> `okx-a2a session delete`; `jobId` mode queries and deletes all matching SQLite sessions plus `backup:<jobId>`.
- `xmtp_start_evaluate_conversation` -> `okx-a2a session create` with `toAgentId="_"` and `groupId="_"`.
- `xmtp_get_session_key` -> `okx-a2a session gen-key`.
- `xmtp_get_agent_list` -> `onchainos agent get`; default mode auto-pages and returns a flattened legacy-style agent array.
- `xmtp_get_pending_list` -> `okx-a2a task requests`.
- `xmtp_deny_pending_conversation` -> `okx-a2a task reject`.
- `xmtp_refresh_agents` -> `okx-a2a agent refresh`.
- `xmtp_file_upload` -> `okx-a2a file upload`.
- `xmtp_file_download` -> `okx-a2a file download`.
- `session_status` -> returns current `OKX_A2A_CURRENT_*` session context.
- `xmtp_runtime_env` -> returns `cli`.

## Rules

- Do not rewrite the whole legacy playbook just because this bridge is active.
- Do not read the CLI's private job/session store directly. Use `okx-a2a session get`.
- If a bridge call fails, stop and surface the error instead of inventing a direct XMTP implementation.
- If `okx-a2a` is not on PATH, set `OKX_A2A_BIN` to the command that launches the local `okx-a2a` CLI.

```bash
OKX_A2A_BIN="<okx-a2a-cli-command>" \
node scripts/xmtp-tool.js xmtp_sessions_query '{}'
```
