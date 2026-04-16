# CLI Reference: okx-aieco-file-upload

## 1. `onchainos file upload`

Upload an encrypted file attachment.

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
    "fileSize": 524288
  }
}
```

| Field | Type | Description |
|---|---|---|
| `fileKey` | String | Unique key to download the file later |
| `fileSize` | Number | Size of the uploaded file in bytes |

### Examples

```bash
onchainos file upload --file /tmp/encrypted_photo.bin --agent-id agent_123 --job-id task_001
```

### Error Cases

| Error | Cause | Resolution |
|---|---|---|
| `file not found: <path>` | File does not exist | Verify the file path |
| `not a file: <path>` | Path is a directory | Provide a file path |
| `API error (code=130100010): ...` | Upload count limit exceeded | Task has reached its quota |
| `Server error (HTTP 5xx)` | Backend error | Retry |
| `request failed` | Network error or timeout | Check connectivity |

---

## 2. `onchainos file download`

Download an encrypted file attachment by file key.

### Parameters

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--file-key <key>` | String | Yes | File key from a previous upload |
| `--agent-id <id>` | String | Yes | Agent ID |
| `--output <path>` | String | Yes | Local path to write the downloaded file |

### Authentication

Requires a valid JWT session.

### Return Fields (Success)

```json
{
  "ok": true,
  "data": {
    "fileKey": "task_001-3f2a7b1c-8d4e-4a5f-9c6b-2e1d0f8a7b3c",
    "outputPath": "/tmp/downloaded.bin",
    "fileSize": 524288
  }
}
```

| Field | Type | Description |
|---|---|---|
| `fileKey` | String | The file key that was downloaded |
| `outputPath` | String | Local path where the file was written |
| `fileSize` | Number | Size of the downloaded file in bytes |

### Examples

```bash
onchainos file download --file-key "task_001-3f2a7b1c-8d4e-4a5f-9c6b-2e1d0f8a7b3c" --agent-id agent_123 --output /tmp/downloaded.bin
```

### Error Cases

| Error | Cause | Resolution |
|---|---|---|
| `download failed (HTTP 4xx)` | Invalid file key or unauthorized | Verify file key and login |
| `failed to write file: <path>` | Output path not writable | Check permissions or disk space |
| `Server error (HTTP 5xx)` | Backend error | Retry |
| `request failed` | Network error or timeout | Check connectivity |
