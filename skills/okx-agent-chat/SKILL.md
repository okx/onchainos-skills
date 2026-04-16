---
name: okx-agent-chat
description: >
  Agent-to-agent communication module: XMTP messaging, encrypted file attachments, and plugin management.
  Use for: agent communication, agent commerce, talk to agent, message agent, send file to agent,
  upload attachment, download attachment, XMTP extension, install xmtp, check xmtp version, update xmtp,
  agent间通信, agent交互, 上传附件, 下载附件, 安装XMTP插件, 更新XMTP插件.
  Do NOT use for: token swaps, wallet balance, market data, DeFi protocols, security scanning,
  task marketplace (use okx-agent-task).
license: MIT
metadata:
  author: okx
  version: "1.0.0"
---

# OKX Agent Chat

Agent-to-agent communication — XMTP plugin management and encrypted file attachment upload/download.

## Pre-flight Checks

<MUST>
Before any agent-to-agent communication, **always** run the XMTP plugin check first:
> Read `ensure-installed.md`
</MUST>

For file upload/download commands that use the `onchainos` CLI:
> Read `../okx-agentic-wallet/_shared/preflight.md`.
> If that file does not exist, read `_shared/preflight.md` instead.

## Capabilities

| # | Capability | File | When to Use |
|---|-----------|------|-------------|
| 1 | Ensure XMTP plugin installed | `ensure-installed.md` | **Mandatory** before any agent communication. Auto-triggers version check. |
| 2 | Check XMTP plugin version | `check-version.md` | Check for updates, can run independently or auto-triggered after ensure-installed. |
| 3 | Upload/download file attachments | `file-attachment.md` | Upload encrypted files to CDN or download by file key. Requires wallet auth (JWT). |

## Routing Logic

When the agent encounters a chat-related request:

1. **Agent wants to communicate with another agent** → Load `ensure-installed.md` first (mandatory safeguard), then proceed with communication.
2. **User asks to install or check XMTP** → Load `ensure-installed.md` or `check-version.md` directly.
3. **User asks to upload or download a file attachment** → Load `file-attachment.md`.

## Skill Routing

- For agent-to-agent communication / XMTP / file attachments → **this skill** (`okx-agent-chat`)
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
