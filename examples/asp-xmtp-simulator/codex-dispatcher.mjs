#!/usr/bin/env node

import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const THIS_FILE = fileURLToPath(import.meta.url);
function parsePrompt(prompt) {
  try {
    return JSON.parse(prompt);
  } catch {
    return null;
  }
}

function peerFromSessionKey(sessionKey) {
  if (typeof sessionKey !== "string") return null;
  const match = /^job:[^:]+:my:[^:]+:to:(.+)$/.exec(sessionKey);
  return match ? decodeURIComponent(match[1]) : null;
}

export function inspectDispatch({ args, env }) {
  const parsed = parsePrompt(args.at(-1) || "");
  const message = parsed?.message && typeof parsed.message === "object"
    ? parsed.message
    : parsed;
  const event = typeof message?.event === "string" ? message.event : "";
  const agentId = String(env.OKX_A2A_CURRENT_AGENT_ID || parsed?.agentId || "");
  const jobId = String(env.OKX_A2A_CURRENT_JOB_ID || message?.jobId || "");
  const sessionKey = String(env.OKX_A2A_CURRENT_SESSION_KEY || "");
  const toAgentId = peerFromSessionKey(sessionKey);
  return {
    // Compatibility wrapper only. The ASP runtime owns lifecycle handling.
    shouldSend: false,
    agentId,
    toAgentId,
    jobId,
    event,
    messageId: String(env.OKX_A2A_CURRENT_MESSAGE_ID || ""),
  };
}

function runRealCodex(args, env) {
  const command = env.ASP_REAL_CODEX_COMMAND || "codex";
  if (path.resolve(command) === path.resolve(THIS_FILE)) {
    throw new Error("ASP_REAL_CODEX_COMMAND points back to codex-dispatcher.mjs");
  }
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { env, stdio: "inherit" });
    child.once("error", reject);
    child.once("close", (code, signal) => {
      if (signal) {
        process.kill(process.pid, signal);
        return;
      }
      resolve(code ?? 1);
    });
  });
}

export async function main(args = process.argv.slice(2), env = process.env) {
  return runRealCodex(args, env);
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(THIS_FILE)) {
  main().then((code) => {
    process.exitCode = code;
  }).catch((error) => {
    process.stderr.write(
      `[asp-codex-dispatcher] ${error instanceof Error ? error.message : String(error)}\n`,
    );
    process.exitCode = 1;
  });
}
