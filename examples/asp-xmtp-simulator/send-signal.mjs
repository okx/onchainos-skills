#!/usr/bin/env node

import { randomUUID } from "node:crypto";

function readArgument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

async function main() {
  const serverUrl = process.env.ASP_SIMULATOR_URL || "http://127.0.0.1:4319/signal";
  const body = {
    jobId: readArgument("--job-id"),
    toAgentId: readArgument("--to-agent-id"),
    signalText: readArgument("--message"),
    messageId: readArgument("--message-id") || randomUUID(),
  };
  const headers = { "content-type": "application/json" };
  if (process.env.SIMULATOR_AUTH_TOKEN) {
    headers.authorization = `Bearer ${process.env.SIMULATOR_AUTH_TOKEN}`;
  }

  const response = await fetch(serverUrl, {
    method: "POST",
    headers,
    body: JSON.stringify(body),
  });
  const result = await response.json();
  if (!response.ok || result.ok !== true) {
    throw new Error(result.error || `simulator server returned HTTP ${response.status}`);
  }
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
}

main().catch((error) => {
  process.stderr.write(`[send-signal] ${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
});
