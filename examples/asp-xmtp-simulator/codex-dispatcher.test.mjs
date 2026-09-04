import assert from "node:assert/strict";
import test from "node:test";

import { inspectDispatch } from "./codex-dispatcher.mjs";

function eventPrompt(event, agentId = "11926") {
  return JSON.stringify({
    agentId,
    message: { source: "system", event, jobId: "job-1" },
  });
}

function dispatch(event, overrides = {}) {
  return inspectDispatch({
    args: ["exec", "--json", eventPrompt(event, overrides.promptAgentId)],
    env: {
      OKX_A2A_CURRENT_AGENT_ID: overrides.agentId || "11926",
      OKX_A2A_CURRENT_JOB_ID: "job-1",
      OKX_A2A_CURRENT_SESSION_KEY:
        overrides.sessionKey || "job:job-1:my:11926:to:11040",
      OKX_A2A_CURRENT_MESSAGE_ID: "message-1",
    },
  });
}

test("never intercepts ASP subscription lifecycle events", () => {
  assert.equal(dispatch("sub_open").shouldSend, false);
  assert.equal(dispatch("sub_created").shouldSend, false);
  assert.equal(dispatch("sub_asp_selected").shouldSend, false);
});

test("passes through other agents, peers, and events", () => {
  assert.equal(dispatch("sub_created", { agentId: "11040" }).shouldSend, false);
  assert.equal(dispatch("sub_created", {
    sessionKey: "job:job-1:my:11926:to:99999",
  }).shouldSend, false);
  assert.equal(dispatch("sub_open").shouldSend, false);
});

test("reads the event from the daemon prompt envelope", () => {
  const result = dispatch("sub_asp_selected");
  assert.equal(result.event, "sub_asp_selected");
  assert.equal(result.jobId, "job-1");
  assert.equal(result.toAgentId, "11040");
  assert.equal(result.messageId, "message-1");
});
