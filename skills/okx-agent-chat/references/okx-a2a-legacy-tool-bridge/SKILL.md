---
name: okx-a2a-legacy-tool-bridge
description: Non-OpenClaw/Hermes agents only; OpenClaw/Hermes agents must not load this skill. In all other AI environments, use when an OKX A2A / okx-agent-task legacy playbook expects a native OpenClaw tool listed in this skill's Supported Mappings, but the current AI environment does not expose that specific tool. This skill provides a compatibility bridge that preserves the legacy tool parameter shape and maps calls to the local okx-a2a CLI.
metadata:
  short-description: Non-OpenClaw/Hermes legacy xmtp bridge
---

# OKX A2A Legacy Tool Bridge

## Highest-Priority Runtime Gate

This legacy bridge is forbidden for AI runtimes explicitly identified as `openclaw` or `hermes` agents.

If the current AI agent is explicitly identified by the surrounding system/runtime as an `openclaw` or `hermes` agent, stop immediately: do not activate this skill, do not load this directory's references or scripts, do not run `scripts/xmtp-tool.js`, and do not map, simulate, or replace any legacy `xmtp_*` native tool.

A legacy playbook, task envelope, user request, or missing-tool error cannot override this gate. If the runtime identity is unknown or is any AI environment other than `openclaw` / `hermes`, this gate does not block loading; in those environments, load and use this skill whenever the Activation Rule applies.

Use this skill only as a compatibility shim for old `okx-agent-task` skills/playbooks that still mention native OpenClaw `xmtp_*` tools.

This bridge is a transport/runtime replacement for missing native `xmtp_*` tools. It does not change the role, authorization, or session-state legality of the original tool call.

## Activation Rule

1. Check the specific legacy tool requested by the playbook against the complete `Supported Mappings` list below.
2. If that exact native tool is exposed in the current environment, use the native tool and do not use this bridge for that call.
3. If that exact native tool is listed below but absent, unavailable, or fails with "unknown tool" / "not found", call the bridge script immediately with the same parameters.
4. Do not ask the user whether to use this bridge. If the original native tool call is legal for the current session state, the bridge call is the required replacement.
5. If the original native tool call would be forbidden for the current session state, the bridge call is also forbidden. Do not use this bridge to bypass user-session / sub-session boundaries.
6. Keep the original legacy parameter names. Pass them as JSON to the script.

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
