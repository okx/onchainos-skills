---
name: agent-transcript-exporter
description: "FOR INTERNAL DEBUGGING ONLY — not for production use. Exports raw Claude Code conversation transcript files for debugging agent misinterpretations, incorrect responses, wrong skill invocations, or any unexpected agent behavior. Bundles the raw JSONL session files into a zip archive for offline analysis. Use when: 'export agent transcript', 'export last hour conversations', 'debug agent behavior', 'export past N hours', 'export past N minutes', 'export today's sessions', 'grab the logs', 'zip the transcripts', '导出对话记录', '导出最近N分钟对话', '导出最近N小时', '导出今天的会话', '抓取日志', '压缩对话记录', '调试代理行为', '导出会话'. Supports time filters: past hour (default), past Nm (e.g. past 10m), past Nh (e.g. past 3h), today, or a specific session ID. Requires only bash and zip — no Python, Node, or jq needed. Do NOT use for any production, user-facing, or privacy-sensitive context."
license: Apache-2.0
metadata:
  author: okx
  version: "2.0.0"
---

# agent-transcript-exporter

**⚠️ 仅限内部调试使用 / Internal debugging skill only.** 将原始 Claude Code JSONL 会话文件打包成 zip 压缩包。无需解析 — 你拿到的就是 Claude Code 原始写入的日志，可用任意工具（jq、Python、Excel 等）离线分析。

Bundles raw Claude Code JSONL session files into a zip archive. No parsing — you get the raw logs exactly as Claude Code wrote them, ready for offline analysis with any tool you prefer.

需要 / Requires: `bash`, `zip` — macOS 和 Linux 均已预装，无需额外安装。

## Usage

```
/agent-transcript-exporter [--past-hour | --past <N>m | --past <N>h | --today | --session <uuid>]
```

## 配置选项 / Configuration Options

| 参数 / Flag | 说明 / Description |
|------------|-------------------|
| *(无)* | 默认：最近 60 分钟 / Default: last 60 minutes |
| `--past-hour` | 最近 60 分钟 / Last 60 minutes |
| `--past <N>m` | 最近 N 分钟，如 `--past 10m`、`--past 30m` |
| `--past <N>h` | 最近 N 小时，如 `--past 3h` |
| `--today` | 今天零点至今 / Since midnight local time |
| `--session <uuid>` | 指定特定会话 UUID / Specific session UUID only |

## 导出内容 / What Gets Exported

zip 压缩包包含 / A zip archive containing:
- 时间窗口内的原始 JSONL 会话文件 / The raw JSONL session file(s) matching the time window
- `README.txt`：描述文件格式与内容 / describing the archive contents and format

每个 JSONL 文件对应一个 Claude Code 会话，每行是一条 JSON 对象，代表对话中的一个事件。字段说明见下方**原始格式参考**章节。

## 实现步骤 / Implementation

按以下步骤依次执行 / Run the following steps exactly as described.

### Step 1 — Compute the cutoff timestamp (seconds since epoch for file comparison)

**Default / `--past-hour`:**
```bash
CUTOFF_EPOCH=$(date -u -v-1H +%s 2>/dev/null || date -u -d '1 hour ago' +%s)
SESSION_FILTER=""
```

**`--past <N>m`:**
```bash
CUTOFF_EPOCH=$(date -u -v-${N}M +%s 2>/dev/null || date -u -d "${N} minutes ago" +%s)
SESSION_FILTER=""
```

**`--past <N>h`:**
```bash
CUTOFF_EPOCH=$(date -u -v-${N}H +%s 2>/dev/null || date -u -d "${N} hours ago" +%s)
SESSION_FILTER=""
```

**`--today`:**
```bash
CUTOFF_EPOCH=$(date -u +%s -d "$(date +%Y-%m-%d) 00:00:00" 2>/dev/null || date -j -f "%Y-%m-%d %H:%M:%S" "$(date +%Y-%m-%d) 00:00:00" +%s)
SESSION_FILTER=""
```

**`--session <uuid>`:**
```bash
CUTOFF_EPOCH=0
SESSION_FILTER="<uuid>"
```

### Step 2 — Locate the transcript directory

Claude Code encodes the project path by replacing `/`, `.`, and `_` with `-`:

```bash
PROJECT_KEY=$(echo "$PWD" | sed 's|[/._]|-|g')
TRANSCRIPT_DIR="$HOME/.claude/projects/$PROJECT_KEY"

if [[ ! -d "$TRANSCRIPT_DIR" ]]; then
    echo "No transcript directory found at: $TRANSCRIPT_DIR"
    exit 0
fi
```

### Step 3 — Collect matching session files

Select files that were **last modified** at or after the cutoff. If `SESSION_FILTER` is set, select only that file.

```bash
MATCHED_FILES=()

for f in "$TRANSCRIPT_DIR"/*.jsonl; do
    [[ -f "$f" ]] || continue

    if [[ -n "$SESSION_FILTER" ]]; then
        [[ "$(basename "$f" .jsonl)" == "$SESSION_FILTER" ]] && MATCHED_FILES+=("$f")
        continue
    fi

    FILE_MTIME=$(date -r "$f" +%s 2>/dev/null || stat -c %Y "$f" 2>/dev/null)
    [[ -n "$FILE_MTIME" && "$FILE_MTIME" -ge "$CUTOFF_EPOCH" ]] && MATCHED_FILES+=("$f")
done

if [[ "${#MATCHED_FILES[@]}" -eq 0 ]]; then
    echo "No session files found in the specified time window."
    echo "Try widening the filter (e.g. --past 3h or --today)."
    exit 0
fi
```

### Step 4 — Create the zip archive

```bash
OUTPUT_DIR="$HOME/Desktop/agent-transcripts"
mkdir -p "$OUTPUT_DIR"
NOW=$(date +%Y%m%d_%H%M%S)
ARCHIVE="$OUTPUT_DIR/transcript_${NOW}.zip"

# Write a README into the archive describing the format
README_FILE=$(mktemp)
cat > "$README_FILE" << EOF
Debug Transcript Export
=======================
Generated : $(date -u +%Y-%m-%dT%H:%M:%SZ)
Project   : $PWD
Sessions  : ${#MATCHED_FILES[@]}

Each .jsonl file is one Claude Code session.
Each line is a JSON object with these key fields:

  type        "user" | "assistant" | "file-history-snapshot" | "progress"
  timestamp   ISO 8601 UTC
  isSidechain true for internal tool sub-chains (usually skip these)

For type "user" (real user message):
  message.content   string (plain text) OR array of content blocks

For type "assistant":
  message.content   array of blocks:
    { type: "thinking", thinking: "..." }        agent reasoning
    { type: "text",     text: "..." }            response text
    { type: "tool_use", id, name, input: {...} } tool invocation

Tool results appear as type "user" with:
  message.content[].type == "tool_result"
  message.content[].tool_use_id  matches the tool_use id above
  message.content[].content      the result text

Useful jq snippets:
  # Show all user prompts
  jq 'select(.type=="user") | select(.message.content|type=="string") | .message.content' session.jsonl

  # Show all tool calls
  jq 'select(.type=="assistant") | .message.content[]? | select(.type=="tool_use") | {name,input}' session.jsonl

  # Show agent final responses
  jq 'select(.type=="assistant") | .message.content[]? | select(.type=="text") | .text' session.jsonl
EOF

zip -j "$ARCHIVE" "${MATCHED_FILES[@]}" "$README_FILE" > /dev/null
rm "$README_FILE"

echo "SESSIONS=${#MATCHED_FILES[@]}"
echo "ARCHIVE=$ARCHIVE"
echo "SIZE=$(du -sh "$ARCHIVE" | cut -f1)"
```

### Step 5 — Report results

Display to the user:

```
✓ Exported <SESSIONS> session(s)

Archive  ~/Desktop/agent-transcripts/transcript_<ts>.zip
Size     <size>
```

List the session filenames included in the archive so the user knows which sessions were captured.

## 原始格式参考 / Raw Format Reference

Each line in a session JSONL is one of:

**User message** (real prompt from the user):
```json
{
  "type": "user",
  "timestamp": "2026-03-18T01:33:54.985Z",
  "isSidechain": false,
  "message": {
    "role": "user",
    "content": "does this doc cover how we can retrace conversations?"
  }
}
```

**Assistant turn** (agent response + tool calls):
```json
{
  "type": "assistant",
  "timestamp": "2026-03-18T01:33:58.745Z",
  "message": {
    "role": "assistant",
    "content": [
      { "type": "thinking", "thinking": "The user wants to check a Lark doc..." },
      { "type": "tool_use", "id": "toolu_01X", "name": "mcp__lark__get_node", "input": { "token": "abc" } },
      { "type": "text", "text": "Short answer: Partially..." }
    ]
  }
}
```

**Tool result** (returned to the assistant, appears as a user-role message):
```json
{
  "type": "user",
  "message": {
    "role": "user",
    "content": [
      { "type": "tool_result", "tool_use_id": "toolu_01X", "content": "Node title: OnchainOS CLI..." }
    ]
  }
}
```

## 隐私与清理 / Privacy & Cleanup

- 压缩包保存在本地 `~/Desktop/agent-transcripts/`，不会上传或提交 / Archives are stored locally, never committed or uploaded
- 确认 `~/Desktop/agent-transcripts/` 已加入 `.gitignore`
- 分析完成后删除 / Delete after analysis:
  ```bash
  rm -rf ~/Desktop/agent-transcripts/
  ```
- 请勿分享导出文件 — 包含用户原始提问与代理回复 / Do not share — contains verbatim user prompts and agent responses
