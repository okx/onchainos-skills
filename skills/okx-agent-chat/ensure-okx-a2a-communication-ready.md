# Ensure OKX A2A Communication Ready

**Mandatory communication-init flow** — ensures OKX A2A communication is ready for the current runtime. Designed to be **auto-triggered by the LLM itself**, without waiting for the user to ask.

Runtime families:

- **OpenClaw**: uses the OpenClaw OKX A2A plugin and native `xmtp_*` tools.
- **Hermes agent**: uses the Hermes OKX A2A plugin and native `xmtp_*` tools when already loaded.
- **Node environment**: Claude Code, Codex, and other non-OpenClaw/non-Hermes environments use the `okx-a2a` Node CLI.

This file is the **router**: it owns the readiness self-check and runtime detection. The per-runtime flows live in `references/comm-init/` and are loaded one at a time, on demand.

## When To Run (Auto-Trigger Contract)

The LLM **must** invoke this flow **on its own**, immediately after any of the following just completed successfully — even if the user did not explicitly ask to "sync" or "refresh":

| Upstream action | Where it typically happens |
|---|---|
| Agent registered / created | `okx-agent-identity` register flow |
| Agent metadata updated (name, avatar, endpoint, capabilities, etc.) | `okx-agent-identity` update flow |
| Agent deactivated / re-activated | `okx-agent-identity` deactivate / activate flow |
| Any other operation that mutates the local a2a agent list | — |

**Recognition cues** (Chinese / English) that should trigger this hook after the upstream action returns: `创建 agent`, `注册 agent`, `更新 agent`, `修改 agent 信息`, `注销 agent`, `停用 agent`, `agent 列表变更`, `agent registered`, `agent created`, `agent updated`, `agent deactivated`, `agent list changed`.

The flow is safe to invoke unconditionally. It first checks whether communication is already ready in the current toolset, then self-routes by deterministic shell/runtime signals only when setup is still required. The LLM does **not** need to confirm with the user before running Step 0.

## Runtime Decision Tree

```
Step 0  Toolset self-check
  ├─ xmtp_refresh_agents is present
  │    └─ Call it directly and end
  └─ Tool is absent
       └─ Step 1 Runtime detection
            ├─ HERMES_SESSION_ID is set and HERMES_DESKTOP_CWD is not set
            │    └─ Read references/comm-init/hermes.md   (Step 3 Hermes flow)
            ├─ OPENCLAW_SHELL or OPENCLAW_CLI is set
            │    └─ Read references/comm-init/openclaw.md (Step 2 OpenClaw flow)
            ├─ An OpenClaw process is found in the parent-process chain (up to 8 levels)
            │    └─ Read references/comm-init/openclaw.md (Step 2 OpenClaw flow)
            └─ Otherwise
                 └─ Read references/comm-init/node.md     (Step 4 Node flow)
```

## Step 0: Toolset Self-Check

<MUST>
Inspect the LLM's current toolset before running any shell command. This is the authoritative readiness check and is independent of runtime detection.
</MUST>

- **`xmtp_refresh_agents` is present** -> call it directly (no arguments unless its schema requires them). If it succeeds, surface only user-relevant output and end the flow.
- **`xmtp_refresh_agents` returns an error** -> surface the error verbatim and stop.
- **`xmtp_refresh_agents` is absent** -> continue to Step 1.

Do not run runtime detection, installation checks, or gateway health checks when the tool is already present.

## Step 1: Runtime Detection

<MUST>
When Step 0 does not find `xmtp_refresh_agents`, run the shell function below. Do not ask the model or the user to self-report whether the runtime is OpenClaw, Hermes, Claude, or Codex.
</MUST>

Run:

```bash
detect_runtime() {
  # Hermes first: this signal shape is the most specific.
  if [ -n "${HERMES_SESSION_ID:-}" ] && [ -z "${HERMES_DESKTOP_CWD:-}" ]; then
    echo "hermes"
    return
  fi

  # Preserve legacy OpenClaw environment hints as the cheap path.
  if [ -n "${OPENCLAW_SHELL:-}" ] || [ -n "${OPENCLAW_CLI:-}" ]; then
    echo "openclaw"
    return
  fi

  # Cover newer OpenClaw/Codex launch shapes by walking at most 8 parents.
  pid=$PPID
  for _ in 1 2 3 4 5 6 7 8; do
    if [ -z "$pid" ] || [ "$pid" = "0" ] || [ "$pid" = "1" ]; then
      break
    fi
    comm=$(ps -p "$pid" -o comm= 2>/dev/null | tr -d ' ')
    case "$comm" in
      *openclaw*|*OpenClaw*)
        echo "openclaw"
        return
        ;;
    esac
    pid=$(ps -p "$pid" -o ppid= 2>/dev/null | tr -d ' ')
  done

  echo "node"
}

runtime=$(detect_runtime)
echo "runtime=$runtime"
```

The PPID walk inspects process names only. Do not check socket files, use `lsof`, ask the LLM/user to declare the runtime, or use gateway status as runtime detection.

## Branch Routing (Steps 2–4)

Based on the `runtime=` output, load exactly one branch file and continue its Execution Flow:

| `runtime=` output | Read this file |
|---|---|
| `openclaw` | [references/comm-init/openclaw.md](references/comm-init/openclaw.md) — Step 2 OpenClaw flow |
| `hermes` | [references/comm-init/hermes.md](references/comm-init/hermes.md) — Step 3 Hermes flow |
| `node` | [references/comm-init/node.md](references/comm-init/node.md) — Step 4 Node flow |

<MUST>
Read exactly ONE branch file — the one matching the `runtime=` output. Do NOT read the other two branch files; they describe runtimes that are not present. Branch-specific edge cases live inside each branch file.
</MUST>

## Edge Cases (Routing)

| Scenario | Behavior |
|---|---|
| Tool `xmtp_refresh_agents` is present | Step 0 calls it immediately and ends without shell runtime detection. |
| `xmtp_refresh_agents` call returns an error | Surface the error verbatim and stop. |
| Runtime signals conflict | Hermes' specific signal shape wins first, then OpenClaw env hints, then the OpenClaw PPID fallback, then Node. |
| PPID walk reaches PID 0/1, an empty PID, or 8 levels without finding OpenClaw | Fall back to Node. |
