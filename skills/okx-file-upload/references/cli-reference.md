# CLI Reference: okx-file-upload

## 1. `onchainos file upload`

Upload an encrypted file attachment to the OKX CDN.

### Parameters

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--file <path>` | String | Yes | Path to the local file to upload |
| `--agent-id <id>` | String | Yes | Agent ID |
| `--job-id <id>` | String | Yes | Job ID |

### Authentication

Requires a valid JWT session. The CLI automatically refreshes expired tokens if a valid refresh token exists.

### Return Fields (Success)

```json
{
  "ok": true,
  "data": {
    "fileKey": "task_001-3f2a7b1c-8d4e-4a5f-9c6b-2e1d0f8a7b3c",
    "attachmentUrl": "https://xx.com/okx/web3/wallet/aieco/task_001-3f2a7b1c-8d4e-4a5f-9c6b-2e1d0f8a7b3c",
    "fileSize": 524288
  }
}
```

| Field | Type | Description |
|---|---|---|
| `fileKey` | String | Unique identifier for the uploaded attachment |
| `attachmentUrl` | String | Publicly accessible CDN URL for the uploaded file |
| `fileSize` | Number | Size of the uploaded file in bytes |

### Return Fields (Error)

```json
{
  "ok": false,
  "error": "upload failed (code=130100010): Upload count limit exceeded for task: task_001"
}
```

### Examples

**Upload an encrypted file attachment:**
```bash
onchainos file upload --file /tmp/encrypted_photo.bin --agent-id agent_123 --job-id task_001
```

### Error Cases

| Error | Cause | Resolution |
|---|---|---|
| `file not found: <path>` | File does not exist at the given path | Verify the file path |
| `not a file: <path>` | Path points to a directory, not a file | Provide a file path |
| `not logged in` | No valid JWT session | Run `onchainos wallet login` first |
| `upload failed (code=130100010): ...` | Upload count limit exceeded for the task | Task has reached its attachment quota |
| `upload failed (code=...): ...` | Backend rejected the upload | Check the error message for details |
| `server error (HTTP 5xx)` | Backend server error | Retry after a moment |
| `upload request failed` | Network error or timeout | Check connectivity; retry |
