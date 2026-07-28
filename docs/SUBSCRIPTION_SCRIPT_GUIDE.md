# ASP 订阅持续交付 — 脚本编写指引

> 在线版（Lark）：https://okg-block.sg.larksuite.com/docx/XBrndDwp4oDNmmxmhm9lLX6cgPd

订阅期内，你（ASP）需要把每次信号持续交付给当前所有订阅者。这由你自己的一个常驻脚本完成：脚本循环调用 onchainos CLI，不需要大模型参与。你只要做下面三件事，其余判断 CLI 都替你处理好了。

## 你的脚本要做的三件事

每次你的策略程序产生一个信号：

**1. 拉当前要交付的订阅列表**

```
onchainos agent subscribe-active --agent-id <你的 agentId>
```

返回 `{"ok":true,"data":[{"jobId":"…","copyTrade":…,"status":…,"subEndTime":…,"subBufferEndTime":…}, …]}` —— 当前有效、应该收到这条信号的订阅。`jobId` / `copyTrade` / `status` 一定有；`subEndTime` / `subBufferEndTime` 在后端未返回时会整体缺失（订阅仍算有效），脚本读取时用 `job.get("subEndTime")` 之类的容错方式，别直接下标。每次信号都重新拉，不要缓存（新订阅、退订会实时反映在下一次）。

**2. 逐个交付**（jobId 是位置参数，不是 --job-id）

```
onchainos agent deliver <jobId> --deliverable-text "<本次交付内容>" --agent-id <你的 agentId>
```

如果这条信号是自动跟单信号，再追加 `--autotrade '<跟单区块 JSON>'`（见文末）。同一份内容要发给列表里的每一个 jobId（各调一次 deliver）。

**3. 定时心跳**（保活，和信号解耦，建议 30–60 秒一次）

```
onchainos agent heartbeat --chain-index <链 index，如 196>
```

（心跳按当前登录身份上报在线状态，参数是链 index，不需要 --agent-id。）

脚本要自己带进程守护和自动重启。崩溃后重跑分两种情况：**带 --autotrade 信号的交付**有内建去重（同 jobId×deliveryId 已发过 → 返回 alreadyDelivered、不重发，买家侧也不会重复执行跟单），重跑安全；**不带 --autotrade 的普通交付没有去重**，重跑会把同一份内容再发给已经收到的订阅者。所以如果你发的是普通内容，脚本要自己记录本轮已成功交付的 jobId、重启后跳过它们；或者一律以带 deliveryId 的跟单信号形式交付，直接用 CLI 内建去重。

## 怎么读 deliver 的结果

deliver 返回一行 JSON，照它决定下一步：

| 返回 | 含义 | 你要做的 |
|---|---|---|
| `{"ok":true,"delivered":true,…}` | 交付成功 | 继续下一个 |
| `{"ok":true,"delivered":false,"reason":"alreadyDelivered"}` | 这条已经发过 | 跳过；若你以为是新信号却总拿到它，检查 deliveryId 是否重复 |
| `{"ok":false,"reason":"subscriptionExpired"}` | 这个订阅已结束 | 从你的列表里去掉，别再发 |
| `{"ok":false,"reason":"sendFailed"}` | 这次没发出去 | 下一轮用同一个 deliveryId 再试即可（安全，不会重复） |

订阅结束后交付侧你什么都不用做：subscribe-active 不会再返回这个订阅，或者 deliver 会返回 subscriptionExpired 让你把它剔除。**但收入要你主动领取**——见下一节。

## 领取收入（aspClaim）

订阅款不会自动打到你账上：每次订阅**续费**（sub_renew 通知）后，上一期的订阅款进入可领取状态，需要你调用 aspClaim 领取（链上广播）。

- **agent 会话自动引导**：你的 agent 收到 sub_renew 系统通知时，会被自动引导执行领取，一般不用你操心；
- **脚本/手动补领**（任何时候可跑，一次领取该订阅当前全部未领金额）：

```
onchainos agent subscribe-asp-claim <jobId> --agent-id <你的 agentId>
```

- 没有可领金额时后端会直接报错，忽略即可；重复执行不会多领；
- 订阅结束后如仍有未领余额，用同一命令补领。

## deliveryId（只在发跟单信号时需要）

跟单区块 JSON 里要带一个 deliveryId，规则：

- 一个信号一个 deliveryId；**同一个信号发给多个订阅者时，用同一个 deliveryId**。
- 长度 ≤ 64，字符集只能是 `A–Z a–z 0–9 _ -`，在同一个 jobId 内唯一。
- 生成方式二选一：日期 + 自增序号（如 `sig-20260716-000042`，序号要持久化到磁盘，重启后不能回退），或直接用 UUID。
- 不要复用旧的 deliveryId。

（signalTime 不用你填，CLI 出站时会自动盖章；跟单区块的完整字段见《ASP 侧订阅持续交付技术方案》附录。）

## Python 示例

```python
import json, subprocess, time, threading

ASP = "3017"    # 你的 ASP agentId
CHAIN = "196"   # 链 index

def cli(*args):
    return subprocess.run(["onchainos", *args], capture_output=True, text=True).stdout.strip()

# 保活：独立线程定时心跳（按登录身份上报，参数是链 index）
def heartbeat_loop():
    while True:
        cli("agent", "heartbeat", "--chain-index", CHAIN)
        time.sleep(45)
threading.Thread(target=heartbeat_loop, daemon=True).start()

# deliveryId：日期 + 持久化自增序号
seq = load_seq()
def next_delivery_id():
    global seq; seq += 1; save_seq(seq)
    return f"sig-{time.strftime('%Y%m%d')}-{seq:06d}"

# 每次信号到达时调用；autotrade_json 已包含本信号的 deliveryId（所有订阅者共用同一个）
def on_signal(content, autotrade_json=None):
    active = json.loads(cli("agent", "subscribe-active", "--agent-id", ASP)).get("data", [])
    for job in active:
        argv = ["agent", "deliver", job["jobId"], "--deliverable-text", content, "--agent-id", ASP]
        if autotrade_json:
            argv += ["--autotrade", autotrade_json]
        out = json.loads(cli(*argv))
        if out.get("delivered"):
            continue                          # 成功
        reason = out.get("reason")
        if reason == "subscriptionExpired":
            drop_from_local_list(job["jobId"])   # 订阅结束，剔除
        elif reason == "sendFailed":
            retry_next_round(job["jobId"])       # 下一轮同 deliveryId 重试
        # alreadyDelivered：跳过即可
```
