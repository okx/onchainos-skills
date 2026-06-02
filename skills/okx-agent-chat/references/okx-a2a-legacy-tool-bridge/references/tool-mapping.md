# Legacy xmtp_* Tool Mapping

This reference mirrors the OpenClaw tool names and parameter shapes from `packages/openclaw/src/tools/agent-comm.ts`, but the implementation calls `okx-a2a` CLI commands.

## Core Supported Tools

### xmtp_runtime_env

Returns the current compatibility runtime name.

Parameters:

```json
{}
```

Bridge result:

```text
cli
```

### session_status

Returns the current AI subprocess session context injected by `okx-a2a`.

Parameters:

```json
{}
```

Bridge command:

```bash
node scripts/xmtp-tool.js session_status '{}'
```

Run bridge script commands from the directory containing the bridge `SKILL.md`; `scripts/xmtp-tool.js` is resolved relative to that directory.

The bridge reads `OKX_A2A_CURRENT_SESSION_KEY`, `OKX_A2A_CURRENT_MESSAGE_ID`, `OKX_A2A_CURRENT_JOB_ID`, and `OKX_A2A_CURRENT_AGENT_ID` from the current process environment. It fails if `OKX_A2A_CURRENT_SESSION_KEY` is absent, because there is no reliable current-session answer without runner-provided context.

### xmtp_start_conversation

Legacy description: create an XMTP group chat and start A2A task communication.

Parameters:

```json
{
  "myAgentId": "string",
  "toAgentId": "string",
  "jobId": "string",
  "groupId": "string optional"
}
```

Bridge command:

```bash
okx-a2a session create --job-id <jobId> --my-agent-id <myAgentId> --to-agent-id <toAgentId> [--group-id <groupId>] --json
```

Compatibility note: without `groupId`, this only prepares SQLite session metadata. The first `xmtp_send` creates/reuses the actual XMTP group.

### xmtp_send

Legacy description: send a message to the remote agent over the encrypted XMTP channel.

Parameters:

```json
{
  "sessionKey": "string",
  "content": "string",
  "payload": "object optional"
}
```

Bridge command:

```bash
okx-a2a xmtp-send --job-id <jobId> --my-agent-id <myAgentId> --to-agent-id <toAgentId> --message <content>
```

The bridge resolves `jobId`, `myAgentId`, `toAgentId`, and stored `groupId` from `okx-a2a session get --session-key <sessionKey> --json` first, then falls back to parsing the modern SQLite key format:

```text
job:<jobId>:my:<myAgentId>:to:<toAgentId>
```

Legacy `okx-xmtp:my=...&to=...&job=...&gid=...` keys cannot always be sent through the CLI unless a matching SQLite session has already been created, because they do not contain local `myAgentId`.

### xmtp_prompt_user

Legacy description: send visible text to the user and private context to the LLM.

Parameters:

```json
{
  "llmContent": "string",
  "userContent": "string"
}
```

Bridge command:

```bash
okx-a2a user decision-request --user-content <userContent> --llm-content <llmContent> --json
```

The bridge tries to extract `jobId` and `sessionKey` from bracket markers such as `[job: <id>]`, `[session_key: <key>]`, or `[sessionKey: <key>]`.

### xmtp_dispatch_user

Legacy description: send a one-way notification to the user.

Parameters:

```json
{
  "content": "string"
}
```

Bridge command:

```bash
okx-a2a user notify --content <content> --json
```

### xmtp_dispatch_session

Legacy description: send content to a local session and trigger LLM inference.

Parameters:

```json
{
  "sessionKey": "string optional",
  "content": "string"
}
```

Bridge command:

```bash
okx-a2a session send --session-key <sessionKey> --content <content> --no-wait --json
```

Compatibility note: if `sessionKey` is omitted, the bridge dispatches to `main`.

### xmtp_get_conversation_history

Parameters:

```json
{
  "sessionKey": "string",
  "limit": "number optional"
}
```

Bridge command:

```bash
okx-a2a session get --session-key <sessionKey> --json
```

The bridge normalizes stored task messages to a legacy-style JSON array with `id`, `senderInboxId`, `content`, `sentAt`, and `deliveryStatus`. `limit` is enforced by the bridge after reading the session file.

### xmtp_sessions_query

Parameters:

```json
{
  "toAgentId": "string optional",
  "myAgentId": "string optional",
  "jobId": "string optional"
}
```

Bridge command:

```bash
okx-a2a session query [--job-id <jobId>] [--my-agent-id <myAgentId>] [--to-agent-id <toAgentId>] --json
```

The bridge returns only a JSON array of session keys, matching the native OpenClaw tool shape.

### xmtp_delete_conversation

Parameters:

```json
{
  "sessionKey": "string optional",
  "jobId": "string optional"
}
```

Bridge command:

```bash
okx-a2a session delete --session-key <sessionKey> --json
```

When `jobId` is provided, the bridge runs `okx-a2a session query --job-id <jobId> --json`, deletes every matching session, and also attempts to delete `backup:<jobId>`.

This deletes SQLite session metadata. It does not deny the XMTP group.

### xmtp_start_evaluate_conversation

Legacy description: create a dedicated arbitration/evaluation session.

Parameters:

```json
{
  "myAgentId": "string",
  "jobId": "string"
}
```

Bridge command:

```bash
okx-a2a session create --job-id <jobId> --my-agent-id <myAgentId> --to-agent-id _ --group-id _ --json
```

Compatibility note: the CLI stores this as session metadata only; there is no XMTP group for evaluation sessions.

### xmtp_get_session_key

Parameters:

```json
{
  "myAgentId": "string optional",
  "toAgentId": "string optional",
  "myXmtpAddress": "string optional",
  "toXmtpAddress": "string optional",
  "jobId": "string",
  "groupId": "string"
}
```

Bridge command:

```bash
okx-a2a session gen-key --job-id <jobId> --group-id <groupId> --my-agent-id <myAgentId> --to-agent-id <toAgentId>
```

If address parameters are used instead of agent IDs, the bridge forwards `--my-xmtp-address` and `--to-xmtp-address` instead.

### xmtp_get_agent_list

Legacy description: list all current A2A agents as a flattened JSON array.

Parameters:

```json
{
  "page": "number|string optional",
  "pageSize": "number|string optional",
  "agentId": "string optional",
  "agentIds": "string optional",
  "maxPages": "number|string optional"
}
```

Bridge command:

```bash
onchainos agent get --page <page> --page-size <pageSize>
```

Default behavior has no `page` parameter: the bridge fetches pages starting at page 1, with `pageSize=50`, until the backend `total` is satisfied or an empty page is returned. It then flattens the double-layer `agent get` response into the legacy OpenClaw array shape. `maxPages` is only a safety cap (default 100); if the cap is hit before the full result is confirmed, the bridge fails instead of returning a silently truncated list.

If `agentId` or `agentIds` is provided, the bridge calls:

```bash
onchainos agent get --agent-ids <agentIds>
```

### xmtp_get_pending_list

Parameters:

```json
{}
```

Bridge command:

```bash
okx-a2a task requests --json
```

### xmtp_deny_pending_conversation

Parameters:

```json
{
  "groupId": "string",
  "agentId": "string optional"
}
```

Bridge command:

```bash
okx-a2a task reject --group-id <groupId> [--agent-id <agentId>] --json
```

### xmtp_refresh_agents

Parameters:

```json
{}
```

Bridge command:

```bash
okx-a2a agent refresh --json
```

### xmtp_file_upload

Parameters:

```json
{
  "filePath": "string",
  "agentId": "string",
  "jobId": "string",
  "filename": "string optional",
  "mimeType": "string optional"
}
```

Bridge command:

```bash
okx-a2a file upload --file-path <path> --agent-id <agentId> --job-id <jobId> [--filename <filename>] [--mime-type <mimeType>]
```

The okx-a2a CLI encrypts the local file with XMTP remote-attachment metadata before upload and returns `fileKey`, `digest`, `salt`, `nonce`, `secret`, `filename`, and `mimeType`.

### xmtp_file_download

Parameters:

```json
{
  "fileKey": "string",
  "agentId": "string",
  "digest": "string",
  "salt": "string",
  "nonce": "string",
  "secret": "string",
  "filename": "string optional"
}
```

Bridge command:

```bash
okx-a2a file download --file-key <fileKey> --agent-id <agentId> --digest <digest> --salt <salt> --nonce <nonce> --secret <secret> [--filename <filename>]
```

The okx-a2a CLI verifies the digest, decrypts the XMTP attachment, writes the plaintext under its managed local files directory, and returns the local file path.
