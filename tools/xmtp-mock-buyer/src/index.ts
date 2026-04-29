// ─── XMTP mock buyer: 裸 XMTP 通信 ───
// 目标：打通 XMTP dev 网络的收发通路，不关心消息格式/业务协议。
// 用法：
//   XMTP_WALLET_KEYS=0x... npm start                       # 纯监听
//   XMTP_WALLET_KEYS=0x... TO=0xSeller INIT="hi" npm start # 启动后主动发一条
//   收到消息会打印到 stdout；在终端里敲字回车会发给当前活跃会话。

import { homedir } from "node:os";
import { mkdirSync } from "node:fs";
import readline from "node:readline";
import { Agent, IdentifierKind } from "@xmtp/agent-sdk";
import { createUser, createSigner } from "@xmtp/agent-sdk/user";

type XmtpEnv = "dev" | "production" | "local";

function requireEnv(name: string): string {
  const v = process.env[name]?.trim();
  if (!v) {
    console.error(`[mock-buyer] 缺少环境变量: ${name}`);
    process.exit(1);
  }
  return v;
}

async function main() {
  const env = (process.env.XMTP_ENV as XmtpEnv | undefined) ?? "dev";
  const walletKey = requireEnv("XMTP_WALLET_KEYS").split(",")[0]!.trim() as `0x${string}`;
  const recipient = process.env.TO?.trim() ?? "";
  const initMessage = process.env.INIT?.trim() ?? "";

  const dbDir = `${homedir()}/.xmtp-mock-buyer`;
  mkdirSync(dbDir, { recursive: true });

  console.log(`[mock-buyer] 连接 XMTP (env=${env})…`);
  const user = createUser(walletKey);
  const signer = createSigner(user);
  const agent = await Agent.create(signer, {
    dbPath: (inboxId: string) => `${dbDir}/${inboxId}-${env}.db3`,
    env,
  });

  const myInboxId = agent.client.inboxId;
  const myAddress = user.account.address;
  console.log("✓ 已连接");
  console.log(`  inboxId:  ${myInboxId}`);
  console.log(`  address:  ${myAddress}`);

  // 同步已有会话
  await agent.client.conversations.syncAll();
  const list = await agent.client.conversations.list();
  console.log(`[mock-buyer] 已同步 ${list.length} 个会话`);

  // 当前活跃会话（stdin 发送目标）
  let activeConvId: string | null = null;

  // 入站文本消息
  agent.on("text", async (ctx) => {
    const msg = ctx.message;
    if (msg.senderInboxId === myInboxId) return;
    const convId = msg.conversationId;
    const sender = msg.senderInboxId;
    const content = typeof msg.content === "string" ? msg.content : JSON.stringify(msg.content);
    activeConvId = convId;
    console.log(
      `\n[recv] conv=${convId.slice(0, 12)}… from=${sender.slice(0, 12)}…`
    );
    console.log(content);
    process.stdout.write("[mock-buyer] > ");
  });

  // 其他类型消息（group metadata / reactions / 未识别 content type）
  agent.on("unknownMessage", async (ctx) => {
    const msg = ctx.message;
    console.log(
      `\n[recv:unknown] conv=${msg.conversationId.slice(0, 12)}… contentType=${msg.contentType?.typeId ?? "?"}`
    );
  });

  agent.on("start", () => {
    console.log("[mock-buyer] agent 已启动，监听中…");
  });

  // 启动（返回 Promise 会在 stop 时 resolve，我们不 await）
  agent.start().catch((e: unknown) => console.error("[mock-buyer] agent 异常:", e));

  // 可选：主动发一条
  if (recipient && initMessage) {
    console.log(`[mock-buyer] 发起 DM → ${recipient.slice(0, 20)}…`);
    try {
      const conv = recipient.startsWith("0x") && recipient.length === 42
        ? await agent.client.conversations.newDmWithIdentifier({
            identifier: recipient,
            identifierKind: IdentifierKind.Ethereum,
          })
        : await agent.client.conversations.newDm(recipient); // 当成 inboxId
      await conv.send(initMessage);
      activeConvId = conv.id;
      console.log(`[send] conv=${conv.id.slice(0, 12)}… → ${JSON.stringify(initMessage)}`);
    } catch (e) {
      console.error("[mock-buyer] 建 DM / 发送失败:", e);
    }
  }

  // stdin 循环
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
    prompt: "[mock-buyer] > ",
  });
  rl.prompt();
  rl.on("line", async (line) => {
    const body = line.trim();
    if (!body) {
      rl.prompt();
      return;
    }
    if (!activeConvId) {
      console.log(
        "[mock-buyer] 没有活跃会话。启动时设 TO=0x…(或 inboxId) + INIT=\"hi\"，或等对方先发消息。"
      );
      rl.prompt();
      return;
    }
    try {
      const conv = await agent.client.conversations.getConversationById(activeConvId);
      if (!conv) {
        console.log(`[mock-buyer] 会话未找到: ${activeConvId}`);
      } else {
        await conv.send(body);
        console.log(`[send] conv=${activeConvId.slice(0, 12)}… → ${JSON.stringify(body)}`);
      }
    } catch (e) {
      console.error("[mock-buyer] 发送失败:", e);
    }
    rl.prompt();
  });

  rl.on("close", async () => {
    console.log("\n[mock-buyer] 退出…");
    process.exit(0);
  });
}

main().catch((e) => {
  console.error("[mock-buyer] fatal:", e);
  process.exit(1);
});
