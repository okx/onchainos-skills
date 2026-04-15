import WebSocket from "ws";

const SERVER = "ws://127.0.0.1:9000";
const BUYER_ADDR = "0x86c0ba14e37406373de0fce5fe8d961dc53ef6f1";
const SELLER_ADDR = "0xSeller333333333333333333333333333333333";
const CONV_ID = `conv-e2e-${Date.now()}`;

function connectAndRegister(addr) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(SERVER);
    ws.on("open", () => ws.send(JSON.stringify({ action: "Register", addr })));
    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg.type === "registered") { console.log(`✓ ${addr} registered`); resolve(ws); }
    });
    ws.on("error", reject);
    setTimeout(() => reject(new Error("connect timeout")), 5000);
  });
}

async function main() {
  console.log(`CONV: ${CONV_ID}`);
  const seller = await connectAndRegister(SELLER_ADDR);

  // Set up message listener that resolves on first REPLY
  const replyReceived = new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("timeout waiting for reply")), 90000);
    seller.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg.type === "error") {
        console.log(`[seller] server error: ${msg.msg}`);
        return;
      }
      if (msg.from) {
        console.log(`✓ 收到 AI 回复 from=${msg.from}`);
        console.log(`  content: ${String(msg.payload?.content ?? "").slice(0, 200)}`);
        clearTimeout(timer);
        resolve(msg);
      }
    });
  });

  seller.send(JSON.stringify({
    action: "JoinConversation",
    conversation_id: CONV_ID,
    participants: [SELLER_ADDR, BUYER_ADDR],
  }));
  await new Promise(r => setTimeout(r, 300));

  console.log(`→ Sending TASK_INQUIRE...`);
  seller.send(JSON.stringify({
    action: "Send",
    conversation_id: CONV_ID,
    payload: { type: "TASK_INQUIRE", task_id: "task-001", content: "你好，我是卖家。请问这个任务是什么内容？预算多少？" }
  }));

  await replyReceived;
  console.log(`\n✅ 端到端测试通过！AI 买家正常接收并回复了卖家消息。`);
  seller.close();
  process.exit(0);
}

main().catch(e => {
  console.error(`❌ 失败: ${e.message}`);
  process.exit(1);
});
