import assert from "node:assert/strict";
import http from "node:http";
import test from "node:test";

import {
  buildSignalMessage,
  buildXmtpSendArgs,
  createRequestHandler,
  defaultSignalForService,
  acceptSubscription,
  listAspAgents,
  listSubscriptionJobs,
  messageEligibilityForStatus,
  parsePeerFromSessionKey,
  subscriptionStatusLabel,
} from "./server.mjs";

test("provides a service-specific default signal and eligibility label", () => {
  const marriage = defaultSignalForService("5ebbf120-a8bf-419f-925b-a94af41cb5fa");
  assert.equal(marriage.serviceName, "姻緣命理解讀");
  assert.match(marriage.signal, /姻緣命理解讀｜模擬信號/);
  assert.match(marriage.signal, /不構成真實命理解讀/);
  assert.equal(messageEligibilityForStatus(1).eligible, true);
  assert.equal(messageEligibilityForStatus(7).eligible, false);
  assert.equal(messageEligibilityForStatus(null, false).eligible, false);
});

test("parses the peer from an ASP session key", () => {
  assert.equal(
    parsePeerFromSessionKey("job:job-1:my:11926:to:11040", "11926"),
    "11040",
  );
  assert.equal(parsePeerFromSessionKey("job:job-1:my:7:to:11040", "11926"), null);
});

test("marks outbound messages as simulations", () => {
  assert.equal(
    buildSignalMessage({ jobId: "job-1", signalText: "BTC-USDT BUY" }),
    "jobId: job-1\ndeliverableType: text\n- - -\n[SIMULATED_SIGNAL]\nBTC-USDT BUY\nsimulation: true\n- - -\n[intent:deliver]",
  );
});

test("builds inspectable xmtp-send arguments", () => {
  assert.deepEqual(
    buildXmtpSendArgs({
      jobId: "job-1",
      agentId: "11926",
      toAgentId: "11040",
      message: "payload",
    }),
    [
      "xmtp-send",
      "--job-id",
      "job-1",
      "--session-agent-id",
      "11926",
      "--to-agent-id",
      "11040",
      "--message",
      "payload",
      "--json",
    ],
  );
});

test("renders subscription status zero as waiting for ASP acceptance", () => {
  assert.equal(subscriptionStatusLabel(0, "UNKNOWN_0"), "CREATED — 等待 ASP 接受");
  assert.equal(subscriptionStatusLabel(1, "ACTIVE"), "ACTIVE — 可以發送訂閱交付");
});

test("lists only ASP identities from agent get", async () => {
  const asps = await listAspAgents({
    onchainosBin: "onchainos",
    run: async () => ({
      ok: true,
      data: { list: [{ agentList: [
        { agentId: "11926", name: "玄策閣", role: 2, roleLabel: "ASP" },
        { agentId: "11040", name: "Buyer", role: 1, roleLabel: "User" },
      ] }] },
    }),
  });
  assert.deepEqual(asps.map((asp) => asp.agentId), ["11926"]);
});

test("accepts a created subscription only after service ownership validation", async () => {
  const calls = [];
  const result = await acceptSubscription({
    onchainosBin: "onchainos",
    jobId: "job-created",
    agentId: "11926",
    run: async (_binary, args) => {
      calls.push(args);
      if (args[1] === "subscribe-detail") {
        return { ok: true, data: { jobId: "job-created", providerAgentId: "11926", serviceId: "service-1", status: 0 } };
      }
      if (args[1] === "service-list") {
        return { ok: true, data: [{ list: [{ serviceId: "service-1" }] }] };
      }
      return { ok: true, data: { txHash: "0xaccept" } };
    },
  });
  assert.equal(result.ok, true);
  assert.deepEqual(calls.map((args) => args[1]), ["subscribe-detail", "service-list", "accept-subscription"]);
});

test("joins active subscriptions with matching ASP A2A sessions", async () => {
  const result = await listSubscriptionJobs({
    a2aBin: "okx-a2a",
    onchainosBin: "onchainos",
    agentId: "11926",
    buyerAgentId: "11040",
    run: async (binary, args) => binary === "onchainos" && args[1] === "subscribe-active"
      ? { ok: true, data: [{ jobId: "active-job", status: 1 }] }
      : binary === "onchainos" && args[1] === "my-subscriptions"
        ? { ok: true, data: { list: [{ jobId: "active-job", providerAgentId: "11926", buyerAgentId: "11040", serviceId: "service-1", status: 1, statusName: "ACTIVE" }] } }
        : binary === "onchainos"
          ? { ok: true, data: { jobId: args[2], status: 0, statusName: "UNKNOWN_0" } }
      : {
        ok: true,
        sessions: [
          { jobId: "active-job", myAgentId: "11926", toAgentId: "11040", codexSessionId: "codex-1" },
          { jobId: "session-only", myAgentId: "11926", toAgentId: "11040", codexSessionId: "codex-2" },
          { jobId: "other-buyer", myAgentId: "11926", toAgentId: "99999" },
        ],
      },
  });

  assert.deepEqual(result.jobs.map((job) => job.jobId), ["active-job", "session-only"]);
  assert.equal(result.jobs[0].codexSessionId, "codex-1");
  assert.equal(result.jobs[0].sendable, true);
  assert.equal(result.jobs[0].messageEligibility.eligible, true);
  assert.equal(result.jobs[1].statusName, "UNKNOWN_0");
  assert.equal(result.jobs[1].statusLabel, "CREATED — 等待 ASP 接受");
  assert.equal(result.jobs[1].sendable, false);
  assert.equal(result.jobs[1].messageEligibility.eligible, false);
});

test("dashboard is a read-only ASP runtime observer", async (t) => {
  const server = http.createServer(createRequestHandler({
    agentId: "11926",
    buyerAgentId: "11040",
    jobId: "job-subscription",
    dryRun: true,
    send: async () => ({ ok: true, messageId: "xmtp-message-1" }),
  }));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise((resolve) => server.close(resolve)));
  const { port } = server.address();

  const page = await fetch(`http://127.0.0.1:${port}/`);
  assert.equal(page.status, 200);
  const html = await page.text();
  assert.match(html, /ASP Runtime 唯讀觀察台/);
  assert.match(html, /不接受訂閱、不生成交付、不發送 XMTP/);
  assert.match(html, /本帳戶的 ASP/);
  assert.match(html, /details class=\"service-group\"/);

  const signal = await fetch(`http://127.0.0.1:${port}/signal`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ messageId: "subscription-1", signalText: "BTC BUY" }),
  });
  assert.equal(signal.status, 405);
  assert.match((await signal.json()).error, /read-only dashboard/);

  const history = await (await fetch(`http://127.0.0.1:${port}/api/events`)).json();
  assert.equal(history.config.mode, "read-only");
  assert.equal(history.events.length, 0);
});

test("read-only dashboard blocks subscription acceptance", async (t) => {
  const calls = [];
  const server = http.createServer(createRequestHandler({
    agentId: "11926",
    accept: async (input) => {
      calls.push(input);
      return { ok: true, result: { txHash: "0xaccept" } };
    },
  }));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise((resolve) => server.close(resolve)));
  const { port } = server.address();

  const response = await fetch(`http://127.0.0.1:${port}/api/subscriptions/accept`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jobId: "job-created", agentId: "11926" }),
  });
  assert.equal(response.status, 405);
  assert.deepEqual(calls, []);

  const history = await (await fetch(`http://127.0.0.1:${port}/api/events`)).json();
  assert.equal(history.events.length, 0);
});

test("read-only dashboard blocks dispatcher sends", async (t) => {
  const calls = [];
  const server = http.createServer(createRequestHandler({
    agentId: "11926",
    dryRun: true,
    resolvePeer: async () => "11040",
    send: async (input) => {
      calls.push(input);
      return { ok: true };
    },
  }));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise((resolve) => server.close(resolve)));
  const { port } = server.address();

  const request = () => fetch(`http://127.0.0.1:${port}/dispatch`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jobId: "job-1",
      agentId: "11926",
      sessionKey: "job:job-1:my:11926:to:11040",
      messageId: "message-1",
    }),
  });

  assert.equal((await request()).status, 405);
  assert.equal((await request()).status, 405);
  assert.equal(calls.length, 0);
});

test("read-only dashboard blocks direct signal sends", async (t) => {
  const calls = [];
  const server = http.createServer(createRequestHandler({
    agentId: "11926",
    buyerAgentId: "11040",
    jobId: "job-subscription",
    dryRun: true,
    send: async (input) => {
      calls.push(input);
      return { ok: true };
    },
  }));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise((resolve) => server.close(resolve)));
  const { port } = server.address();

  const response = await fetch(`http://127.0.0.1:${port}/signal`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ messageId: "subscription-1", signalText: "今日訂閱內容" }),
  });

  assert.equal(response.status, 405);
  assert.equal(calls.length, 0);
});

test("read-only rejection happens before recipient validation", async (t) => {
  const server = http.createServer(createRequestHandler({
    agentId: "11926",
    buyerAgentId: "11040",
    jobId: "job-subscription",
    dryRun: true,
  }));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise((resolve) => server.close(resolve)));
  const { port } = server.address();

  const response = await fetch(`http://127.0.0.1:${port}/signal`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ toAgentId: "99999" }),
  });

  assert.equal(response.status, 405);
  assert.match((await response.json()).error, /read-only dashboard/);
});
