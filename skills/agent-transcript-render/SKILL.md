---
name: agent-transcript-render
description: "FOR INTERNAL DEBUGGING ONLY. Converts exported Claude Code transcript zip or JSONL files into a chronological, human-readable conversation log for debugging. Use after running agent-transcript-exporter. Use when: 'render transcript', 'beautify transcript', 'make transcript readable', 'show me the conversation', 'parse the export', 'read the zip', 'format the logs', 'render the jsonl', '渲染对话记录', '美化日志', '让对话可读', '解析导出文件', '查看对话历史', '格式化日志', '读取压缩包', '展示会话内容'. Accepts a zip file path (from agent-transcript-exporter), a .jsonl file path, or no argument to auto-use the most recent export. Requires only node — no extra dependencies. Do NOT use for any production or privacy-sensitive context."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.0"
---

# agent-transcript-render

**⚠️ 仅限内部调试使用 / Internal debugging skill only.** 将 `agent-transcript-exporter` 导出的原始 JSONL 会话文件转换为清晰的时序对话日志，展示每条用户提问、代理思考过程、工具调用（含输入输出）及最终回复。

Converts raw JSONL session files into a clean, chronological conversation log — showing every user prompt, agent thinking block, tool call with input/result, and agent response.

需要 / Requires: `node`, `unzip` — Claude Code 安装环境自带，无需额外安装。

配套技能 / Companion to: `agent-transcript-exporter`

## Usage

```
/agent-transcript-render [<path-to.zip> | <path-to.jsonl>]
```

| 参数 / Argument | 行为 / Behaviour |
|----------------|----------------|
| *(无)* | 自动使用 `~/Desktop/agent-transcripts/` 中最新的 zip / Auto-detect most recent zip |
| `path/to/transcript.zip` | 解压并渲染其中所有会话 / Unzip and render all sessions inside |
| `path/to/session.jsonl` | 直接渲染单个会话文件 / Render a single session file directly |

## Output Format

A plain-text `.txt` file written alongside the input, e.g. `render_20260318_143022.txt`:

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
TURN 3  ·  2026-03-18 01:33:54 UTC  ·  4054a3ea...
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

👤 USER

   does this doc cover how we can retrace conversations?

────────────────────────────────────────────────────────────────────────
💭 THINKING

   The user wants to check a Lark document. Let me fetch it...

────────────────────────────────────────────────────────────────────────
🔧 TOOL: mcp__claude_ai_OKEngine_LARK_MCP__lark_wiki_get_node

   Input:
   {
     "token": "S3aAwe10ZirZDZkfPdLlugz3goc"
   }

   Result:
   OnchainOS CLI Audit Log 方案设计...

────────────────────────────────────────────────────────────────────────
🤖 AGENT

   Short answer: Partially — it covers command-level retracing...
```

## 实现步骤 / Implementation

按以下步骤依次执行 / Run the following steps exactly.

### Step 1 — 解析输入文件 / Resolve the input file

**无参数时自动检测最新导出 / No argument (auto-detect latest export):**
```bash
INPUT=$(ls -t "$HOME/Desktop/agent-transcripts"/transcript_*.zip 2>/dev/null | head -1)
if [[ -z "$INPUT" ]]; then
    echo "未找到导出文件 / No exports found in ~/Desktop/agent-transcripts/"
    echo "请先运行 / Run /agent-transcript-exporter first."
    exit 0
fi
echo "Using: $INPUT"
```

**Argument provided:** use the given path directly as `INPUT`.

### Step 2 — Unzip if needed

```bash
if [[ "$INPUT" == *.zip ]]; then
    WORK_DIR=$(mktemp -d)
    trap 'rm -rf "$WORK_DIR"' EXIT
    unzip -q "$INPUT" -d "$WORK_DIR"
    JSONL_FILES=("$WORK_DIR"/*.jsonl)
    OUTPUT_DIR=$(dirname "$INPUT")
else
    JSONL_FILES=("$INPUT")
    OUTPUT_DIR=$(dirname "$INPUT")
fi
```

### Step 3 — Write and run the renderer

```bash
RENDER_SCRIPT=$(mktemp /tmp/render_XXXXXX.js)

cat > "$RENDER_SCRIPT" << 'JSEOF'
'use strict';
const fs   = require('fs');
const path = require('path');

const inputFiles = process.argv.slice(2).filter(f => f.endsWith('.jsonl'));
const outputDir  = process.argv[process.argv.length - 1];

if (!inputFiles.length) { console.error('No .jsonl files provided'); process.exit(1); }

const allEntries = [];
const toolResults = {};

for (const file of inputFiles) {
  const sessionId = path.basename(file, '.jsonl');
  const lines = fs.readFileSync(file, 'utf8').split('\n');
  for (const line of lines) {
    if (!line.trim()) continue;
    let entry;
    try { entry = JSON.parse(line); } catch { continue; }
    if (entry.isSidechain) continue;
    const ts = entry.timestamp ? new Date(entry.timestamp) : null;
    if (!ts) continue;
    if (entry.type === 'user' || entry.type === 'assistant')
      allEntries.push({ ts, sessionId, entry });
    if (entry.type === 'user' && Array.isArray(entry.message?.content)) {
      for (const block of entry.message.content) {
        if (block.type !== 'tool_result' || !block.tool_use_id) continue;
        if (typeof block.content === 'string')
          toolResults[block.tool_use_id] = block.content;
        else if (Array.isArray(block.content))
          toolResults[block.tool_use_id] = block.content
            .filter(c => c.type === 'text').map(c => c.text).join('\n');
      }
    }
  }
}

allEntries.sort((a, b) => a.ts - b.ts);

const turns = [];
let current = null;
function flush() { if (current) { turns.push(current); current = null; } }

for (const { ts, sessionId, entry } of allEntries) {
  const content = entry.message?.content;
  if (entry.type === 'user') {
    let text = '';
    if (typeof content === 'string') text = content.trim();
    else if (Array.isArray(content)) {
      if (content.some(c => c.type === 'tool_result')) continue;
      text = content.filter(c => c.type === 'text').map(c => c.text).join(' ').trim();
    }
    if (!text) continue;
    flush();
    current = { ts, sessionId, userPrompt: text, steps: [] };
  } else if (entry.type === 'assistant' && current) {
    if (!Array.isArray(content)) continue;
    for (const block of content) {
      if (block.type === 'thinking' && block.thinking?.trim())
        current.steps.push({ type: 'thinking', content: block.thinking.trim() });
      else if (block.type === 'text' && block.text?.trim())
        current.steps.push({ type: 'text', content: block.text.trim() });
      else if (block.type === 'tool_use')
        current.steps.push({ type: 'tool_call', tool: block.name || '',
          input: block.input || {}, result: toolResults[block.id] ?? null });
    }
  }
}
flush();

const THICK   = '━'.repeat(72);
const DIVIDER = '─'.repeat(72);
const out = [];

out.push('DEBUG TRANSCRIPT RENDER');
out.push(`Generated : ${new Date().toUTCString()}`);
out.push(`Sessions  : ${[...new Set(turns.map(t => t.sessionId))].length}`);
out.push(`Turns     : ${turns.length}`);
out.push('');

for (let i = 0; i < turns.length; i++) {
  const t = turns[i];
  const ts = t.ts.toISOString().replace('T', ' ').slice(0, 19) + ' UTC';
  out.push(THICK);
  out.push(`TURN ${i + 1}  ·  ${ts}  ·  ${t.sessionId.slice(0, 8)}...`);
  out.push(THICK);
  out.push('');
  out.push('👤 USER');
  out.push('');
  const words = t.userPrompt.split(' ');
  let acc = '   ';
  for (const w of words) {
    if (acc.length + w.length > 80) { out.push(acc); acc = '   ' + w + ' '; }
    else acc += w + ' ';
  }
  if (acc.trim()) out.push(acc);
  out.push('');
  for (const step of t.steps) {
    if (step.type === 'thinking') {
      out.push(DIVIDER);
      out.push('💭 THINKING');
      out.push('');
      for (const l of step.content.split('\n')) out.push('   ' + l);
      out.push('');
    } else if (step.type === 'tool_call') {
      out.push(DIVIDER);
      out.push(`🔧 TOOL: ${step.tool}`);
      out.push('');
      for (const l of JSON.stringify(step.input, null, 2).split('\n')) out.push('   ' + l);
      out.push('');
      if (step.result !== null) {
        const preview = step.result.length > 500
          ? step.result.slice(0, 500) + '\n   ... (truncated)' : step.result;
        out.push('   Result:');
        for (const l of preview.split('\n')) out.push('   ' + l);
      } else {
        out.push('   Result: (not captured)');
      }
      out.push('');
    } else if (step.type === 'text') {
      out.push(DIVIDER);
      out.push('🤖 AGENT');
      out.push('');
      for (const l of step.content.split('\n')) out.push('   ' + l);
      out.push('');
    }
  }
}
out.push(THICK);
out.push('END OF TRANSCRIPT');
out.push(THICK);

const nowStr  = new Date().toISOString().replace(/[^0-9]/g, '').slice(0, 14);
const outFile = require('path').join(outputDir, `render_${nowStr}.txt`);
fs.writeFileSync(outFile, out.join('\n'), 'utf8');
console.log(`OUTPUT=${outFile}`);
console.log(`TURNS=${turns.length}`);
JSEOF

node "$RENDER_SCRIPT" "${JSONL_FILES[@]}" "$OUTPUT_DIR"
rm -f "$RENDER_SCRIPT"
```

### Step 4 — Report results

Parse the output lines and display:

```
✓ Rendered <TURNS> turns

Output  <path-to-render_<ts>.txt>
```

Then display the **first 60 lines** of the rendered file inline so the user can see the format immediately without opening the file:

```bash
head -60 "<OUTPUT_PATH>"
```

## 隐私与清理 / Privacy & Cleanup

- 渲染文件保存在桌面 `~/Desktop/agent-transcripts/`，方便上传共享 / Rendered files saved to Desktop for easy sharing
- 分析完成后删除 / Delete after analysis: `rm -rf ~/Desktop/agent-transcripts/`
- 请勿分享 — 包含用户原始提问、代理推理及工具调用结果 / Do not share — contains verbatim prompts, reasoning, and tool results
