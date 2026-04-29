---
name: okx-agent-chat
description: >
  Agent-to-agent communication module: XMTP messaging, encrypted file attachments, and plugin management.
  Use for: agent communication, agent commerce, talk to agent, message agent, send file to agent,
  upload attachment, download attachment, XMTP extension, install xmtp, check xmtp version, update xmtp,
  agent间通信, agent交互, 上传附件, 下载附件, 安装XMTP插件, 更新XMTP插件.
  AUTO-TRIGGER (no user request needed) immediately after any local a2a agent list mutation:
  agent registered, agent created, agent updated, agent deactivated, agent activated,
  agent list changed, post-create agent, post-update agent, after registering agent,
  after creating agent, after updating agent,
  创建agent后, 注册agent后, 更新agent后, 修改agent后, 注销agent后, 停用agent后, agent列表变更后.
  Do NOT use for: token swaps, wallet balance, market data, DeFi protocols, security scanning,
  task marketplace (use okx-agent-task).
  Do NOT use when the user says only a single word like 'chat' or 'message' without specifying an agent, file, or plugin action.
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# OKX Agent Chat

Agent-to-agent communication — XMTP plugin management and encrypted file attachment upload/download.

## Pre-flight Checks

<MUST>
Whenever the local a2a agent list changes (create / modify / deactivate), or before any agent-to-agent communication, **always** run the OpenClaw sync flow first:
> Read `after-agent-list-changed.md`
</MUST>

For file upload/download commands that use the `onchainos` CLI:
> Read `../okx-agentic-wallet/_shared/preflight.md`.
> If that file does not exist, read `_shared/preflight.md` instead.

## Capabilities

| # | Capability | File | When to Use |
|---|-----------|------|-------------|
| 1 | After: agent list changed | `after-agent-list-changed.md` | **Mandatory** post-hook after any local a2a agent list change (create / modify / deactivate), and pre-flight before any agent-to-agent communication. Detects whether the LLM session is in OpenClaw runtime; if so, refreshes the agent list (when the plugin is already loaded) or installs the plugin from `~/Downloads/openclaw-okx-a2a-extension-<version>.tgz` plus updates config (when not yet installed; the install command auto-restarts the gateway). Silently skips outside OpenClaw. |
| 2 | Check plugin version | `check-version.md` | Check for plugin updates. Run independently when the user explicitly asks. |
| 3 | Upload/download file attachments | `file-attachment.md` | Upload encrypted files to CDN or download by file key. Requires wallet auth (JWT). |

## Routing Logic

When the agent encounters a chat-related request:

1. **Local a2a agent list changed (create / modify / deactivate), or agent wants to communicate with another agent, or agent list might be stale** → Load `after-agent-list-changed.md`.
2. **User asks to install the plugin** → Load `after-agent-list-changed.md` (it handles install when the plugin is not yet present).
3. **User asks to check plugin version** → Load `check-version.md`.
4. **User asks to upload or download a file attachment** → Load `file-attachment.md`.

## Skill Routing

- For task marketplace / escrow / delivery → use `okx-agent-task`
- For wallet login / balance / send tokens / tx history → use `okx-agentic-wallet`
- For public wallet balance (by address) → use `okx-wallet-portfolio`
- For token swaps / trades / buy / sell → use `okx-dex-swap`
- For token search / metadata / holders / cluster analysis → use `okx-dex-token`
- For token prices / K-line charts / wallet PnL → use `okx-dex-market`
- For smart money / whale / KOL signals → use `okx-dex-signal`
- For meme / pump.fun token scanning → use `okx-dex-trenches`
- For transaction broadcasting / gas estimation → use `okx-onchain-gateway`
- For security scanning (token / DApp / tx / signature) → use `okx-security`
