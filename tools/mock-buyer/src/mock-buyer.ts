import { WsMockClient } from "./ws-client.js";
import { BuyerSession, BUYER_COMM_ADDR, BUYER_AGENT_ID, WS_URL, TASK_TYPE, PAYMENT_MODE, callX402PayApi, callCompleteApi, startNegotiation } from "./buyer-session.js";

async function main() {
  const client = new WsMockClient(WS_URL, BUYER_COMM_ADDR);
  await client.connectAndRegister();
  await client.registerIdentity("REQUESTER", BUYER_AGENT_ID, BUYER_COMM_ADDR);
  console.log(`✓ 身份已注册: role=REQUESTER agentId=${BUYER_AGENT_ID} commAddr=${BUYER_COMM_ADDR}`);
  console.log(`[buyer] 无头模式运行中 taskType=${TASK_TYPE} paymentMode=${PAYMENT_MODE}，等待 job_created...\n`);

  const sessions = new Map<string, BuyerSession>();

  const envJobId = process.env.JOB_ID;
  if (envJobId) {
    if (TASK_TYPE === "x402") {
      console.log(`[buyer] JOB_ID=${envJobId}，x402 模式 → 直接调用支付 API...`);
      await callX402PayApi(envJobId);
    } else {
      console.log(`[buyer] JOB_ID=${envJobId}，直接启动协商...`);
      await startNegotiation(client, envJobId, sessions);
    }
  }

  client.start((envelope) => {
    const { conversation_id: convId, from, payload } = envelope;
    const type = String(payload.type ?? "");
    const jobId = String(payload.jobId ?? "");
    if (from === BUYER_COMM_ADDR) return;
    console.log(`[buyer] ← conv=${convId.slice(-30)} from=${from.slice(0, 20)} type=${type}`);

    if (type === "job_created" && jobId) {
      if (TASK_TYPE === "x402") {
        console.log(`[buyer] job_created jobId=${jobId}，x402 模式 → 跳过协商，直接支付...`);
        (async () => {
          await callX402PayApi(jobId);
          await callCompleteApi(jobId);
          console.log(`[buyer] x402 flow done ✅ jobId=${jobId}`);
        })().catch(console.error);
        return;
      }
      console.log(`[buyer] job_created jobId=${jobId}，启动协商...`);
      startNegotiation(client, jobId, sessions).catch(console.error);
      return;
    }

    const session = sessions.get(convId);
    if (!session) { console.log(`[buyer] unknown conv=${convId.slice(-20)}, ignoring`); return; }
    session.handle(envelope).catch((e) => console.error(`[buyer][session] error:`, e));
  });

  await new Promise(() => {});
}

main().catch(console.error);
