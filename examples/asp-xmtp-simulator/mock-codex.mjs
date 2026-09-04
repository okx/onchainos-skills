#!/usr/bin/env node

import { randomUUID } from "node:crypto";

function emit(event) {
  process.stdout.write(`${JSON.stringify(event)}\n`);
}

function resumedThreadId(args) {
  const resumeIndex = args.indexOf("resume");
  if (resumeIndex < 0 || args.length < 2) return null;
  return args.at(-2) || null;
}

async function main() {
  const args = process.argv.slice(2);
  const prompt = args.at(-1) || "";
  const jobId = process.env.OKX_A2A_CURRENT_JOB_ID || process.env.OKX_AGENT_TASK_CURRENT_JOB_ID;
  const agentId = process.env.OKX_A2A_CURRENT_AGENT_ID || process.env.OKX_AGENT_TASK_CURRENT_SESSION_AGENT_ID;
  const sessionKey = process.env.OKX_A2A_CURRENT_SESSION_KEY || "";
  const messageId = process.env.OKX_A2A_CURRENT_MESSAGE_ID || process.env.OKX_AGENT_TASK_CURRENT_MESSAGE_ID || randomUUID();
  const serverUrl = process.env.ASP_SIMULATOR_URL || "http://127.0.0.1:4319/dispatch";

  if (!jobId || !agentId) {
    throw new Error("okx-a2a did not provide jobId/agentId dispatch context");
  }

  const headers = { "content-type": "application/json" };
  if (process.env.SIMULATOR_AUTH_TOKEN) {
    headers.authorization = `Bearer ${process.env.SIMULATOR_AUTH_TOKEN}`;
  }
  const response = await fetch(serverUrl, {
    method: "POST",
    headers,
    body: JSON.stringify({ jobId, agentId, sessionKey, messageId, prompt }),
  });
  const result = await response.json();
  if (!response.ok || result.ok !== true) {
    throw new Error(result.error || `simulator server returned HTTP ${response.status}`);
  }

  const threadId = resumedThreadId(args) || randomUUID();
  emit({ type: "thread.started", thread_id: threadId });
  emit({
    type: "item.completed",
    item: {
      id: randomUUID(),
      type: "agent_message",
      status: "completed",
      text: `Simulated XMTP signal queued for job ${jobId} to Agent #${result.toAgentId}.`,
    },
  });
  emit({ type: "turn.completed", usage: { input_tokens: 0, output_tokens: 0 } });
}

main().catch((error) => {
  process.stderr.write(`[mock-codex] ${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
});
