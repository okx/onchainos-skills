---
name: okx-aieco-file-upload
description: "Use this skill when the user wants to upload a file, image, document, or attachment to the OKX CDN and receive a public URL. Trigger keywords: upload file, upload image, upload document, upload attachment, file to CDN, get file URL, host file, 上传文件, 上传图片, 上传附件, 文件上传, CDN上传. Do NOT use for: downloading files, managing uploaded files, XMTP messaging, wallet transfers."
license: MIT
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# Onchain OS File Upload

Upload a local file attachment to the OKX CDN and receive a publicly accessible URL. The file is expected to be XMTP-encrypted by the upstream layer before being passed to this command. Requires wallet authentication (JWT).

## Pre-flight Checks

> Read `../okx-agentic-wallet/_shared/preflight.md`.
> If that file does not exist, read `_shared/preflight.md` instead.

## Command Index

> **CLI Reference**: For full parameter tables, return field schemas, and usage examples, see [cli-reference.md](references/cli-reference.md).

| # | Command | Description | Auth Required |
|---|---|---|---|
| 1 | `onchainos file upload --file <path> --agent-id <id> --job-id <id>` | Upload a file attachment to CDN, returns the attachment URL | Yes |

## Authentication

This skill requires the user to be logged in with a wallet session.

1. Run `onchainos wallet status`. If `loggedIn: true`, proceed.
2. If not logged in → route to **okx-agentic-wallet** skill for authentication.

## Execution Flow

### Upload a File

1. **Collect parameters**: The agent must provide `--file` (path), `--agent-id`, and `--job-id`.
2. **Validate login**: Ensure the user is logged in (`onchainos wallet status`).
3. **Run the upload command**:
   ```bash
   onchainos file upload --file <path> --agent-id <agent_id> --job-id <job_id>
   ```
4. **Return the attachment URL**: On success, the CLI outputs JSON with `fileKey`, `attachmentUrl`, and `fileSize`. Display the `attachmentUrl` to the user.

### Uploading Multiple Files

The upload endpoint supports one file per call. To upload multiple files, run the command once per file:

```bash
onchainos file upload --file photo1.png --agent-id agent_123 --job-id task_001
onchainos file upload --file photo2.png --agent-id agent_123 --job-id task_001
onchainos file upload --file document.pdf --agent-id agent_123 --job-id task_001
```

Each call returns an independent attachment URL. Failure of one does not affect the others.

## Skill Routing

- For uploading a file attachment to CDN → use **this skill** (`okx-aieco-file-upload`)
- For wallet login / balance / send tokens / tx history → use `okx-agentic-wallet`
- For public wallet balance (by address) → use `okx-wallet-portfolio`
- For token swaps / trades / buy / sell → use `okx-dex-swap`
- For token search / metadata / holders / cluster analysis → use `okx-dex-token`
- For token prices / K-line charts / wallet PnL → use `okx-dex-market`
- For smart money / whale / KOL signals → use `okx-dex-signal`
- For meme / pump.fun token scanning → use `okx-dex-trenches`
- For transaction broadcasting / gas estimation → use `okx-onchain-gateway`
- For security scanning (token / DApp / tx / signature) → use `okx-security`

## Edge Cases

| Scenario | Behavior |
|---|---|
| **Not logged in** | CLI returns `"not logged in"`. Route to `okx-agentic-wallet` for authentication. |
| **File not found** | CLI returns `"file not found: <path>"` before making any API call. Ask the user to verify the path. |
| **Not a file** (directory path given) | CLI returns `"not a file: <path>"`. Ask for a file path, not a directory. |
| **Upload count limit exceeded** | CLI returns `"upload failed (code=130100010): Upload count limit exceeded for task: <jobId>"`. The task has reached its attachment quota. |
| **Server error** | CLI returns the error message from the backend. Suggest retrying. |
| **Large file timeout** | If the upload takes too long, the request may time out (60s default). Suggest smaller files or checking network. |

## Global Notes

- **Encryption**: The file is expected to be XMTP-encrypted before being passed to this command. This module does not perform encryption — it uploads whatever bytes it receives.
- **File types**: Any file type is accepted. The server may enforce restrictions — if rejected, relay the error message.
- **File size**: No known client-side limit. The server may enforce limits — if rejected, relay the error message.
- **Attachment URL**: The returned `attachmentUrl` is publicly accessible.
- **Locale-aware output**: All user-facing content must be translated to match the user's language.
