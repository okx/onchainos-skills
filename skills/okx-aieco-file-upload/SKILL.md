---
name: okx-aieco-file-upload
description: "Use this skill when the user wants to upload or download an encrypted file attachment via the AI economy platform. Trigger keywords: upload file, upload image, upload document, upload attachment, download file, download attachment, file key, 上传文件, 上传图片, 上传附件, 下载文件, 下载附件, 文件上传, 文件下载. Do NOT use for: unencrypted file hosting, XMTP messaging, wallet transfers."
license: MIT
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# Onchain OS File Upload

Upload and download encrypted file attachments via the AI economy platform. Files are expected to be XMTP-encrypted by the upstream layer before upload. Requires wallet authentication (JWT).

## Pre-flight Checks

> Read `../okx-agentic-wallet/_shared/preflight.md`.
> If that file does not exist, read `_shared/preflight.md` instead.

## Command Index

> **CLI Reference**: For full parameter tables, return field schemas, and usage examples, see [cli-reference.md](references/cli-reference.md).

| # | Command | Description | Auth Required |
|---|---|---|---|
| 1 | `onchainos file upload --file <path> --agent-id <id> --job-id <id>` | Upload a file attachment, returns file key | Yes |
| 2 | `onchainos file download --file-key <key> --agent-id <id> --output <path>` | Download a file attachment by key | Yes |

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
4. **Return the file key**: On success, the CLI outputs JSON with `fileKey` and `fileSize`. The `fileKey` is used to download the file later.

### Download a File

1. **Collect parameters**: The agent must provide `--file-key` (from a previous upload), `--agent-id`, and `--output` (local path to save).
2. **Run the download command**:
   ```bash
   onchainos file download --file-key <key> --agent-id <agent_id> --output <path>
   ```
3. **Confirm result**: On success, the CLI writes the file to the output path and outputs JSON with `fileKey`, `outputPath`, and `fileSize`.

### Uploading Multiple Files

The upload endpoint supports one file per call. To upload multiple files, run the command once per file:

```bash
onchainos file upload --file photo1.bin --agent-id agent_123 --job-id task_001
onchainos file upload --file photo2.bin --agent-id agent_123 --job-id task_001
```

Each call returns an independent file key. Failure of one does not affect the others.

## Skill Routing

- For uploading or downloading file attachments → use **this skill** (`okx-aieco-file-upload`)
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
| **Not logged in** | CLI returns an auth error. Route to `okx-agentic-wallet` for authentication. |
| **File not found** (upload) | CLI returns `"file not found: <path>"` before making any API call. Ask the user to verify the path. |
| **Not a file** (directory path given) | CLI returns `"not a file: <path>"`. Ask for a file path, not a directory. |
| **Upload count limit exceeded** | CLI returns `"API error (code=130100010): Upload count limit exceeded for task: <jobId>"`. The task has reached its attachment quota. |
| **Invalid file key** (download) | Backend returns an error. Verify the file key is correct. |
| **Output path not writable** (download) | CLI returns `"failed to write file: <path>"`. Check permissions or disk space. |
| **Server error** | CLI returns the error message from the backend. Suggest retrying. |
| **Large file timeout** | If the upload/download takes too long, the request may time out (60s). Suggest smaller files or checking network. |

## Global Notes

- **Encryption**: Files are expected to be XMTP-encrypted before upload. This module does not perform encryption — it uploads/downloads whatever bytes it receives.
- **File key**: The `fileKey` returned from upload is the only way to retrieve the file later. Store it.
- **File types**: Any file type is accepted. The server may enforce restrictions — if rejected, relay the error message.
- **File size**: No known client-side limit. The server may enforce limits — if rejected, relay the error message.
- **Locale-aware output**: All user-facing content must be translated to match the user's language.
