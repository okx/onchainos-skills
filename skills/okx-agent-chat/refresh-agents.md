# Refresh Agents

Refresh the locally cached agent list by invoking the `xmtp_refresh_agents` tool exposed by the XMTP plugin. Use when the agent needs an up-to-date view of registered agents before initiating agent-to-agent communication.

## Why This Step

`xmtp_refresh_agents` is a **plugin-provided tool** registered by `openclaw-okx-a2a-extension`. It is only present in the agent's toolset when the agent is running inside an OpenClaw environment that has loaded the plugin. Outside OpenClaw, the tool simply does not exist — and that's fine.

This flow is environment-probing by design: it runs the tool **iff** it is available, and otherwise does nothing.

## Execution Flow

### Step 1: Check whether the tool is available

The agent inspects its **own current toolset** and checks whether a tool named exactly `xmtp_refresh_agents` is registered.

- If present → proceed to **Step 2**.
- If not present → **silently skip**. Do not call the tool, do not notify the user, do not surface any warning. The user may not be running inside OpenClaw at all; absence of the tool is not an error condition. Stop here.

### Step 2: Invoke the tool

Call `xmtp_refresh_agents` with no arguments unless the tool's own schema requires parameters. Wait for it to return.

Surface the result to the user — typically a refreshed agent list or a status message.

## Edge Cases

| Scenario | Behavior |
|---|---|
| Tool not present in the agent's toolset | Silently skip. No user-facing message. |
| Tool call returns an error | Surface the error message verbatim to the user. |
