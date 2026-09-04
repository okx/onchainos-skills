# 合并冲突决策矩阵（简体中文）

合并方向：将 `origin/codex/a2a-skill-cli-contract` 合入 `origin/codex/a2a-skill-cli-contract-elvis`

- **Elvis / ours / HEAD：** `e69c6fd05c83ac1090d8f4c9d4e883666ef3eda8`
- **Contract / theirs：** `6bb02d665c7d`（最新刷新；原始审计基于 `c7e071bb58b906a51498be105163f739e387d6df`）
- **共同祖先：** `6ff8fc6b093f04cfe80c66248f7cf8f3bb7c39e9`
- **文本冲突：** 13 个文件，58 个冲突块

## 最终解决状态（2026-09-04）

原始 58 个文本冲突块，以及 Contract 最新刷新新增的 2 个冲突块均已解决，当前没有 unmerged 文件。首个合并提交为 `966e03a21`；最新刷新尚待提交和 push。下方 C01–C58 保留为原始冲突逻辑审计，行号是解决前的 review 行号。

- 新增冲突 N01：`job_asp_selected` 保留 Elvis v2 `provider_assignment_playbook`，不恢复旧的内联 apply/二次接受流程；同时保留 Contract 的标题模板注入防护。
- 新增冲突 N02：附件保存同时保留 Elvis 的源文件预检/manifest 返回，以及 Contract 的 jobId 路径组件校验。

- 创建 API：保留 Elvis 的 fixed-price v2 `create-task` / `create-subscribe`，附加 Contract 的 `serviceGuide`、可选 hash 和 `guideConsentJson`。
- Subscription provider：继续必填，确保 Guide 与本地执行合约绑定到明确 ASP。
- 执行授权：Guide + Consent 是唯一权威；删除创建入口的 `copyTrade`、`serviceDescription` 授权含义和全部固定字段 `autotrade-*` 参数。
- 保存时机：v2 executor 得到真实 `jobId` 后、broadcast 前写入 Guide 和 prepared Consent；broadcast 成功后激活，失败或未知结果时中止 prepared Consent。
- 输出：继续使用 Elvis structured envelope，并返回 `guideStatus`、`consentStatus`、`executionProfileSaved`。
- 生命周期：不存在 `sub_open`；User 在 `sub_created` 建立/恢复 A2A session 并转发附件；ASP 在 `sub_asp_selected` 直接开始服务，没有二次接受。
- 防重放：Guide/Consent eligibility 与 trade-record / direct-claim / direct-finalize 幂等同时保留。

## 阅读方式

- **核心决策：** 必须判断最终产品或协议行为，不能机械选择 ours/theirs。
- **建议合并：** 两边解决的是不同问题，通常都应该保留。
- **机械同步：** 没有独立业务含义，必须跟随前面的核心决策。
- **测试/文档：** 应按最终代码契约重建，不能直接整块选择一边。

你可以按这种格式回复：`C01 合并：保留 v2 字段并加入 Guide bundle`、`C19 选 Contract`。

## A. 对外 CLI 命令：`cli/src/commands/agent_commerce/mod.rs`

### C01 — 第 121–145 行 — `create-task` 参数尾部 — 核心决策

**Elvis 逻辑**

- `service_token_amount` 是必填 `String`，保留精确小数，避免浮点精度问题。
- 增加 `category_code`、`min_credit_score`、语义化 `visibility` 和 `chain_id`。
- 属于 v2 fixed-price create-and-fund 契约；前面的字段已经是 `provider_agent_id`、`payment_token_symbol`、`payment_token_amount`。

**Contract 逻辑**

- `service_token_amount` 改为可选。
- 增加 `service_guide`、Guide hash 和用户确认的 Guide Consent JSON。
- 接受隐藏兼容参数 `agentId`/`agent-id`，但忽略它，因为用户身份自动解析。

**只选 Elvis 的后果**

- 保留 v2 交易字段，但没有 Guide bundle；冲突外已经自动合入的 Guide 方法仍会访问这些字段，代码无法编译。

**只选 Contract 的后果**

- 会形成混合 API：前半部分仍是 Elvis v2，尾部却变成 Contract 的可选旧字段，整体不一致。

**真正要决定的内容**

- fixed-price v2 是否是最终主契约。
- 是否在 v2 上附加 Guide + Consent。
- 我的建议：保留全部 Elvis v2 字段，再增加三个可选 Guide 字段；除非外部调用方依赖，否则不保留被忽略的 `agentId`。

**关联：** C03、C04、C09–C17、C42、C44、C45、C50、C51、C55。

### C02 — 第 171–191 行 — `create-subscribe` provider 与执行字段 — 核心决策

**Elvis 逻辑**

- `provider_agent_id` 必填。
- 保留显式 `copy_trade` 标记。
- `service_description` 只作为受限的路由/工具提示，不作为执行授权。

**Contract 逻辑**

- provider 可选，可根据 Service 推导。
- 删除 `copy_trade` 和 `service_description` 执行配置。
- 使用 provider 的原始 Guide、hash 和用户确认的 Guide Consent 作为执行契约。

**关键影响**

- Elvis 是平台预定义的 autotrade 配置模型。
- Contract 是 provider 定义的 Guide 模型。
- 两套授权源同时生效会有安全风险：它们可能对同一交易给出相反结论。

**建议**

- 只选一个权威授权源。如果 Guide 是权威来源，`service_description` 最多保留为展示/搜索元数据，绝不能授权交易。
- provider 是否必填需要单独按 v2 backend 契约确认。

**关联：** C05、C06、C19–C37、C43、C46–C48、C52、C53、C56。

### C03 — 第 1449–1459 行 — 对外 `CreateTask` 解构 — 机械同步

- Elvis 解构 category、credit、visibility、chain。
- Contract 解构 Guide/hash/Consent，并丢弃隐藏 `_agent_id`。
- 必须严格跟随 C01。若 C01 选择组合方案，这里两组字段都要解构。

### C04 — 第 1474–1483 行 — 转发到内部 `TaskCommand::Create` — 机械同步

- Elvis 转发 v2 元数据。
- Contract 转发 Guide bundle。
- 必须与 C01、C03、C42、C44 完全一致，否则公开参数可能失效或直接编译失败。

### C05 — 第 1501–1508 行 — 对外 subscription 解构 — 机械同步

- Elvis 解构 `copy_trade`、`service_description`。
- Contract 解构 Guide/hash/Consent。
- 跟随 C02。如果保留 description 元数据并使用 Guide 授权，可以同时转发；`copy_trade` 不应独立开启执行。

### C06 — 第 1524–1531 行 — 转发到内部 subscription 命令 — 机械同步

- Elvis 转发 `copy_trade` 和 `service_description`。
- Contract 转发 Guide bundle。
- 必须与 C02、C05、C43、C46 一致。

## B. Service 查询：`task/common/mod.rs`

### C07 — 第 548–576 行 — `service-list` 子进程参数 — 建议合并

**Elvis 逻辑**

- 增加 `spawn_service_list_filtered(agent_id, service_id)`。
- 有 service ID 时使用 backend `--service-id` 精确过滤。
- 没有显式分页，依赖默认页大小。

**Contract 逻辑**

- 固定请求 `--page 1 --page-size 100`。
- 这个区块里没有 service-ID 过滤。

**建议结果**

- 保留 Elvis helper，同时加入 Contract 分页参数，然后按需加入 `--service-id`。
- 如果单个 Agent 允许超过 100 个 Service，而且旧 backend 不支持 service-ID 过滤，还需要真正翻页。

## C. 单次任务创建：`task/user/create.rs`

### C08 — 第 3–9 行 — imports — 建议合并/机械同步

- Elvis 只需要 `bail`、`Result`。
- Contract 额外使用 `Context`、`BTreeMap`、`Write`。
- 最终按实际代码保留；使用 Guide 时至少需要 `Context` 和 `BTreeMap`，只有保留旧的 stderr 写法才需要 `Write`。

### C09 — 第 36–51 行 — `CreateTaskParams` 数据模型 — 核心决策

**Elvis 逻辑**

- `service_params`、token address、token amount 全部是必填精确字符串。
- 直接传给 `v2::CreateAndFundInput`。
- 包含 category、score、visibility、chain。

**Contract 逻辑**

- Service 字段可选。
- 增加 Guide/hash/Consent。
- 适配旧 create body，可省略 Service 价格和地址。

**建议**

- 保留 v2 必填字段并增加可选 Guide bundle。
- 如果还要支持非 Service 的通用任务，建议做成明确的另一种模式或命令，不要在 funded-service 流程里用大量 optional 表达。

### C10 — 第 56–67 行 — 校验结果结构 — 建议合并

- Elvis 保存标准化 token symbol 和数值 visibility。
- Contract 保存已经解析校验的 `GuideConsentInput`。
- 三者用途独立，建议一起保留：sanitized title、token/visibility、可选 Guide Consent。

### C11 — 第 135–166 行 — `CreateTaskParams::validate` — 建议合并

**Elvis 逻辑**

- 精确校验 payment/service 小数字符串。
- 只允许 X Layer。
- 校验 credit score、visibility、附件，并清理 title。

**Contract 逻辑**

- 返回标准化旧 currency/title 和 Guide Consent。
- 依赖旧 budget/payment-mode 校验。

**建议**

- 保留 Elvis 校验，再增加 `validated_guide_consent()`。
- 除非 C01 明确选旧 API，否则不要恢复浮点 budget 校验。

### C12 — 第 171–216 行 — 小数校验器与 Guide 持久化 helper — 两边都保留

- Elvis 的 `validate_decimal_amount` 拒绝正负号、科学计数法、空小数、非数字和超过 6 位小数。
- Contract 的 `prepare_guide_consent` 在拿到真实 job ID 后、广播前写入 Guide 和 prepared Consent。
- 两个函数没有逻辑冲突，只是被 Git 判定为同一位置插入；组合方案必须全部保留。

### C13 — 第 289–328 行 — 实际远端创建流程 — 核心/高风险

**Elvis 逻辑**

- 只调用一次 `v2::execute(CreateAndFundInput)`。
- v2 helper 负责创建和注资、附件、runtime binding、签名/广播，并返回 receipt。

**Contract 逻辑**

- 内联调用旧 `/task/create`。
- 获取 job ID 后复制附件、保存指定 provider、准备 Guide Consent、预绑定 provider，然后签名广播。

**绝对不能做的事**

- 不能简单同时保留两条 remote create 路径，否则可能创建两次任务。

**建议**

- 保留 v2 executor，扩展其 hook/callback：获取 job ID 后、广播前准备 Guide；不要并排保留旧 inline create。

### C14 — 第 351–382 行 — 广播成功/失败生命周期 — 核心/高风险

- Elvis 等待 v2 receipt，从 `receipt.broadcast.txHash` 读取 hash，缺失时显示 `pending`。
- Contract 在广播失败时撤销 prepared Guide Consent 和 provider prebind；广播成功后才激活 Guide Consent。
- 建议保留 v2 receipt，同时把 rollback/activate 放到 v2 executor 内或紧邻它的位置。
- 还需判断缺少 tx hash 时是否能简单视为 `pending`，还是应该返回明确的 unknown/reconciliation 状态。

### C15 — 第 391–418 行 — audit 字段 — 建议合并

- Elvis 记录 payment symbol/amount、provider、`bizType=201`。
- Contract 记录旧 currency/budget/maxBudget/paymentMode 和 Guide/Consent 状态。
- v2 + Guide 方案应保留 Elvis payment/biz 字段并增加 Guide/Consent；不要虚构已经不存在的旧 budget/paymentMode。

### C16 — 第 424–486 行 — 成功输出契约 — 核心决策

**Elvis 逻辑**

- 输出 `phase=creation`、`decision=ready`、`reason=broadcast_submitted`。
- 返回机器可执行 `nextAction=[watch_task]`。
- `payload` 包含 job、biz type、provider、payment、runtime、附件和完整 broadcast receipt。

**Contract 逻辑**

- 返回更平的 JSON 和人类 guidance。
- CLI 模式使用 `[Watch]` 文本 handoff。
- 增加 Guide/Consent 状态，并警告不要调用 `set-payment-mode`。

**建议**

- 以 Elvis structured envelope 为唯一机器契约。
- 将 Guide 状态和展示文字放进 payload/display 字段，保留 `watch_task` 作为权威机器动作。

### C17 — 第 594–609 行 — 单测 fixture — 测试重建

- Elvis fixture 使用 fixed-price、category、score、visibility、chain。
- Contract fixture 使用 optional service 字段和空 Guide bundle。
- 按 C09 重建；Guide 可选时分别测试无 Guide、合法 Guide、hash 不匹配、缺少 Consent。

## D. Subscription 创建：`task/user/create_subscribe.rs`

### C18 — 第 11–21 行 — imports — 建议合并/机械同步

- Elvis 导入 A2A 模块、subscription identity selector 和 debug flag。
- Contract 额外导入 `OfflineReplayCapability` 和 `common` namespace。
- 保留最终组合流程真实使用的 imports，格式差异没有业务意义。

### C19 — 第 38–49 行 — Subscription 参数模型 — 核心决策

**Elvis 逻辑**

- provider 是必填 `String`。
- `service_description` 是非授权的路由提示。

**Contract 逻辑**

- provider 是 `Option<String>`。
- Guide/hash/Consent 是执行输入。

**需要确认**

- v2 backend 是否要求指定 provider。
- Guide 是否是唯一交易授权。
- description 是否只保留为展示/发现元数据。

**建议组合**

- provider 按 v2 backend 要求决定是否必填。
- description 仅作元数据。
- Guide bundle 是唯一自动执行授权。

### C20 — 第 75–109 行 — validate 返回类型与 Guide 解析 — 核心决策

**Elvis 逻辑**

- `validate()` 返回 `Result<()>`，不产生 Guide 对象。

**Contract 逻辑**

- 将 Consent JSON 解析为 map。
- 提供 hash/Consent 时必须同时有 Guide。
- 有 Guide 时必须显式传 Consent，包括空对象 `{}`。
- 拒绝 credential-like Consent 字段。
- 返回 `Result<Option<GuideConsentInput>>`。
- `exclude_device` 仍可被 CLI 解析，但在本地返回明确的“不支持”错误。

**建议**

- Guide 为权威模型时采用 Contract 返回类型，同时保留 Elvis 的其他校验。
- `exclude-device` 是否保留 parse-then-error 兼容行为，需要单独决定。

### C21 — 第 143–178 行 — 旧 autotrade 校验与 Guide 校验 — 核心决策

**Elvis 逻辑**

- 使用共享的附件安全校验。
- 配置执行字段时强制显式 mode。
- notify-only 不允许携带自动执行配置。
- 校验所有声明的 required fields。

**Contract 逻辑**

- 只直接检查附件路径存在。
- 返回已解析校验的 Guide Consent。
- 删除平台 `autotrade-*` 策略模型。

**建议**

- 使用 Elvis 的共享附件校验，加 Contract 的 Guide Consent 校验。
- 只有明确保留旧参数兼容 adapter 时，才保留旧 autotrade 校验。

### C22 — 第 203–295 行 — Contract 新增 helper 集合 — 拆分保留

Contract 增加：

- Guide Consent 激活 helper。
- 含 `deviceList:null` 的旧 create-body builder。
- 含 Guide/Consent、offline replay 的成功 payload builder。
- duplicate-subscription block，只在可恢复时提供 restore-listening。

**建议判断**

- 保留 Guide 激活和 duplicate block。
- 保留 offline-replay 数据，但合入 structured payload。
- 如果 v2 负责远端创建，不保留旧 create-body helper；应确认 v2 发出的 terms/device routing 等价。

### C23 — 第 302–316 行 — 登录与 session 前置检查 — 建议合并

- Elvis refresh token 后要求非空 `sessionCert`，因为后续 A2A/v2 操作需要它。
- Contract 初始化 `json_mode` 后 refresh token，不在这里校验 sessionCert。
- 建议同时初始化输出模式并保留 sessionCert 检查；session 现由 `sub_created` 建立，仍需确认创建动作本身是否真的必须被 sessionCert 阻断。

### C24 — 第 324–468 行 — Subscription 远端写入流程 — 核心/高风险

**Elvis 逻辑**

- 解析 wallet 后只调用一次 `v2::execute_create_subscription`。

**Contract 逻辑**

- 写入前再次检查 duplicate，关闭“确认后到创建前”的竞态窗口。
- 检查资金并返回结构化 funding block。
- 获取 provider 的权威 renewal terms。
- 对 `typedData` 做 EIP-712 签名。
- 使用 backend 返回的 `useTrial`，而不是盲信用户输入。
- 创建后复制附件、准备 Guide、预绑定 provider、签名广播。

**特别注意**

- Elvis 的 `81a2de7c` 明确把 duplicate/balance 检查移回 prepare。
- Contract 又明确把 duplicate 检查放回最终写入前来防竞态。

**建议**

- 保留最终 pre-write duplicate/idempotency 检查，但集成进 v2 executor，确保只创建一次。

### C25 — 第 502–536 行 — 广播后处理 — 建议合并/高风险

- Elvis 从 v2 receipt 读取 tx hash，并查询 offline-replay capability。
- Contract 在失败时 rollback Guide/prebind，成功后才 activate Guide Consent。
- 两者应同时保留；capability probe 必须在创建成功后执行，而且不能影响创建结果。

### C26 — 第 549–557 行 — Subscription audit — 核心/建议合并

- Elvis 记录 `copyTrade`、autotrade 是否请求/配置、`bizType=204`。
- Contract 记录 Guide/Consent 是否 active。
- Guide 替换旧模型时，应保留 `bizType=204`，删除误导性的 copyTrade/config 状态，增加 Guide/Consent 状态。
- 如果存在兼容 adapter，要分别记录原始输入模式与归一化后的 Guide 状态。

### C27 — 第 585–640 行 — Subscription 成功输出 — 核心决策

- Elvis 总是返回 structured creation state、rich payload 和 `watch_task`。
- Contract 的 JSON 模式返回较平的成功字段；text 模式打印人类文本；CLI 模式打印 `[Watch]`。
- 建议与 C16 一致：一个 structured result 作为权威，包含 v2 receipt、Guide 状态、offline replay；文字只从它派生。

### C28 — 第 663–681 行 — 默认 CLI parse fixture — 测试/跟随 provider 决策

- Elvis 测试传入必填 provider。
- Contract 故意省略 provider 来证明其可选。
- 按 C19 决定。provider 必填时保留 Elvis 并增加缺失拒绝测试；可选时保留 Contract 并增加推导测试。

### C29 — 第 694–701 行 — 测试中的字段解构 — 测试/机械同步

- Elvis 读取 `copy_trade`、`service_description`。
- Contract 读取 Guide/hash/Consent。
- 必须匹配 C19/C21 的最终字段。

### C30 — 第 714–723 行 — 默认值断言 — 测试/机械同步

- Elvis 断言 provider 已提供、copyTrade 默认 0、description 默认空。
- Contract 断言 provider 和 Guide bundle 全部为空。
- 按最终 provider 和授权模型重建。若 description 仍是元数据且 Guide 可选，可同时测试两组默认值。

### C31 — 第 770–821 行 — duplicate-subscription 响应测试 — 建议保留

- Elvis 没有对应测试，因为 create-time duplicate 检查被删除。
- Contract 校验：不泄露 raw status、显示已有 job、只为 active/restorable subscription 提供 restore-listening。
- 如果最终保留任何 pre-write duplicate 检查，建议保留这些安全的用户展示测试，不受具体检查位置影响。

### C32 — 第 857–881 行 — boolean parse fixture — 测试/跟随 provider 决策

- 两边都在测试 `--auto-renew true`。
- 唯一区别是 Elvis 提供 provider，Contract 省略 provider。
- 按 C19 处理，boolean 行为本身一致。

### C33 — 第 912–961 行 — `--exclude-device` 兼容行为 — 核心兼容决策

- Elvis 从 Clap 删除该参数，因此解析阶段直接失败。
- Contract 保留隐藏参数，让解析通过，再返回明确错误，指导用户创建后使用 device update。
- 两边都不允许 create-time device selection，只是错误体验不同。
- 若旧客户端仍会发送该参数，建议 Contract 的 parse-then-specific-error；否则完全删除。

### C34 — 第 1031–1039 行 — 测试 fixture 的 provider/Guide — 测试/机械同步

- Elvis 把缺失 provider 转为空字符串，并初始化空 service description。
- Contract 保持 `Option`，初始化空 Guide bundle。
- 按 C19 重建；如果 provider 必填，不建议用空字符串代表合法缺失值，除非 validate 明确拒绝。

### C35 — 第 1064–1214 行 — copy-trade 测试与 Contract helper 测试 — 测试拆分

**Elvis 测试：** `--copy-trade` 可以解析。

**Contract 测试：**

- 默认向全部设备路由。
- provider 有无时的序列化。
- offline replay 状态和升级命令。
- funding block 契约。
- `--copy-trade` 已被删除，解析应失败。
- create body 不包含旧 delivery marker。

**处理方式**

- 不能整块选边。
- Contract 的 routing/offline/funding 测试应随对应功能保留。
- copy-trade 接受还是拒绝，跟随 C02/C21。

### C36 — 第 1222–1263 行 — 自动执行 CLI fixture — 核心/测试

- Elvis 传 mode、amount、cap、quote、environment、margin、order、auth 和 required-field。
- Contract 传原始 Guide 和用户确认 Consent JSON。
- 这是两套授权模型最直接的对比测试。
- Guide 为权威时选 Contract。若要兼容旧参数，应另写 adapter 测试，证明旧字段被转换为新模型，并明确冲突时的优先级。

### C37 — 第 1291–1523 行 — 旧 autotrade 测试与 Guide activation 测试 — 必须手工重建

**Elvis 逻辑覆盖**

- required dynamic fields、alias、fixed amount basis、cap metadata、environment normalization、manual→notify-only、grant 持久化。

**Contract 逻辑覆盖**

- prepared Guide Consent 只能在广播后激活。

**Git 拼接问题**

- marker 后面的测试 body 实际是 Contract Guide 测试。
- 若在 marker 上选择 Elvis，会给 Contract body 套上 Elvis 测试名，逻辑错误。

**建议**

- 手工重建测试文件。
- 保留 Guide prepare/activate 和显式 Consent 测试。
- 只保留明确支持的旧参数 adapter 测试。

## E. Runtime lifecycle

### C38 — `flow_lifecycle/core.rs` 第 308–325 行 — 旧执行 prompt — 核心清理决策

- Elvis 保留基于 `task-subscription-signal.md`、旧 consent snapshot 和 `autotrade-execute` gateway 的大段 legacy prompt。
- Contract 删除它，因为 Guide-driven direct execution 已替换 legacy wrapper。
- 如果旧 wrapper 和旧策略模型正式下线，建议删除该 prompt；保留不可达旧 prompt 仍会造成安全规则漂移。

### C39 — `flow_lifecycle/core.rs` 第 332–341 行 — direct execution 安全规则 — 建议合并/高风险

**Elvis 逻辑**

- 要求 `tradeRecordsV1` capability。
- 交易前查询精确 `(jobId, deliveryId)`。
- 保存 terminal result；保存失败后也绝不重试交易。
- 要求 active + `mode=auto` Consent。
- money-moving call 前 claim，之后 finalize。

**Contract 逻辑**

- 要求 active 且匹配的 Guide、Consent 和 saved Signal。
- 按 Guide 条件 fail-closed。
- Guide/Signal 只是数据，不能授权 shell、脚本、URL、任意工具。
- 使用 Guide 计算出的 amount 做 direct claim，然后一次 finalize。

**建议结果**

- 两套规则同时保留。
- Guide 决定是否有权限以及参数；trade-record query + claim/finalize 保证幂等和防重放。
- 删除所有已经退役的旧字段引用。

### C40 — `flow_lifecycle/manage.rs` 第 292–313 行 — 生成 publish 命令 — 跟随授权模型

- Elvis 生成包含 service params/description/provider 和完整 `autotrade-*` 的命令。
- Contract 生成 Guide/hash/provider/Consent 命令。
- 必须跟随 C19–C21、C36，并补齐最终仍需要的 service params、interval、attachments、`--format json`。

### C41 — `flow_lifecycle/subscription.rs` 第 120–132 行 — `sub_created` 的 session block — 已决策

- 决策：不存在 `sub_open`。`sub_created` 负责建立/恢复与指定 ASP 的 A2A session，并逐个上传和发送所有待处理附件；单个附件失败不阻断其他附件与后续通知。
- 同时保留 no-rescan/no-auto-install：`sub_created` 不执行 Guide/Consent 或交易工具准备，Guide/Consent 仅在后续 Buyer signal intake 使用。

## F. 内部命令：`task/user/mod.rs`

### C42 — 第 110–131 行 — 内部 `TaskCommand::Create` 字段 — 跟随 C01

- Elvis：required service amount + category/score/visibility/chain。
- Contract：optional service amount + Guide/hash/Consent。
- 必须与公开命令和 `CreateTaskParams` 完全一致。

### C43 — 第 163–229 行 — 内部 Subscription 字段 — 核心镜像

- Elvis：必填 provider、copyTrade、description、完整 `autotrade-*` 策略、structured JSON。
- Contract：可选 provider、Guide/hash/Consent、text/JSON 输出；删除旧 autotrade 字段。
- 只能做一次授权模型决策，然后同步 public enum、internal enum 和 params；不能只在某一层保留旧字段。

### C44 — 第 1922–1931 行 — 内部 create 解构 — 机械同步

- Elvis 解构 v2 元数据。
- Contract 解构 Guide bundle。
- 跟随 C42。

### C45 — 第 1947–1956 行 — 构造 `CreateTaskParams` — 机械同步

- Elvis 传入 v2 元数据。
- Contract 传入 Guide bundle。
- 跟随 C09/C42，所有字段必须且只能赋值一次。

### C46 — 第 1971–1978 行 — Subscription 授权字段解构 — 机械同步

- Elvis 解构 copyTrade、description。
- Contract 解构 Guide bundle。
- 跟随 C19/C43。

### C47 — 第 1981–1985 行 — device/attachment 解构位置 — 机械/兼容

- Elvis 侧为空，是因为 Elvis 在自己的字段顺序中已经处理 attachments，并删除 `exclude_device`。
- Contract 在这里解构 `exclude_device` 和 attachments。
- attachments 必须保留且不能重复 binding；`exclude_device` 只在 C33 选择兼容错误时保留。

### C48 — 第 2006–2012 行 — 构造 `CreateSubscribeParams` 尾部 — 必须重建

- Elvis 这里传 `autotrade_auth_mode`、settings JSON；其他旧 autotrade 字段位于前面。
- Contract 这里传 `exclude_device`、attachments；Guide 字段位于前面。
- 冲突外 initializer 已经自动拼接了两套字段，不能单独选边。必须按 C19–C21、C33、C43 重写整个 initializer。

## G. Skill 文档

### C49 — `task-action-routing.md` 第 13–19 行 — action route — 建议合并

- Elvis 把 `open_create_playbook` 定义为 Step 3，并新增 provider clarification 的 `send_task_params_response` 和持续 Watch 的 `watch_task`。
- Contract 只让 `open_create_playbook` 委托给 playbook，不绑定具体 step。
- 建议保留 Elvis 新增的两个 action；重新定义 `open_create_playbook` 的责任，避免 centralized router 和 playbook 重复确认。

### C50 — `task-cli-reference.md` 第 168–186 行 — `create-task` 命令文档 — 跟随核心 API

- Elvis 文档是 fixed-price v2、精确小数字符串、必填 provider/service price 和 v2 metadata。
- Contract 文档是旧 budget/max-budget/currency/provider/payment-mode 加 Guide bundle。
- 严格跟随 C01/C09，不能发布 CLI 实际无法解析的混合示例。

### C51 — `task-cli-reference.md` 第 198–213 行 — `create-task` 字段语义 — 跟随核心 API

- Elvis：service params 默认 `{}`，token address/amount 必填，记录 v2 metadata。
- Contract：service values 可选，service params 被描述为自然语言，并增加 Guide 规则。
- 必须明确 service params 最终是 JSON 还是自然语言；当前两边不一致。

### C52 — `task-cli-reference.md` 第 627–642 行 — `create-subscribe` 命令文档 — 跟随授权模型

- Elvis：provider 必填，包含 description 和可选旧 autotrade 字段。
- Contract：provider 可选，guide-driven execution 要求 Guide/Consent。
- 跟随 C19–C21/C36；如果普通通知 subscription 不需要 Guide，应在文档中与自动执行 subscription 分开说明。

### C53 — `task-cli-reference.md` 第 658–678 行 — Subscription 字段语义 — 跟随授权模型

- Elvis 详细定义旧策略字段、typed `extra`、required-field mapping 和 alias。
- Contract 定义 provider 推导、Guide 存储和 Consent。
- 最终代码不支持的字段必须从文档彻底删除；若存在 adapter，要写清转换与优先级。

### C54 — `task-output-templates.md` 第 128–169 行 — structured phase mapping — 核心文档决策

- Elvis 记录 prepare/create、broadcast-submitted、`watch_task`、A2MCP routing 和完整 phase/decision/action 表。
- Contract 在 centralized routing 重构中删除整个区块。
- 如果没有在别处完整迁移，删除会使 Elvis structured response 失去执行含义。
- 建议保留一个 canonical phase mapping，可搬到 centralized router，但不能只删除。

### C55 — `task-user-actions-create.md` 第 183–210 行 — 普通任务创建 — 核心文档决策

- Elvis 调用 fixed-price v2 create，确认后的 Service 上下文原样传递，不重复确认，然后执行 structured `watch_task` 直到 `job_created`。
- Contract 调用旧 payment-mode create，传 Guide bundle，按 `data.guidance` 和 `[Watch]` 文字路由。
- 跟随 C13/C16/C50，机器 Watch 机制只能有一个权威来源。

### C56 — `task-user-actions-create.md` 第 240–253 行 — Subscription 结果与 Guide bundle 规则 — 建议合并

- Elvis 解释 v2 payload、type/bizType 204、autotrade configured 状态和 `watch_task`。
- Contract 要求 Guide/hash/Consent 必须作为一个整体传递；没有 Guide 时三个字段一起省略。
- 建议保留 structured receipt/watch 和原子 Guide-bundle 规则；若旧 autotrade 状态被删除，应替换为 Guide/Consent status。

### C57 — `task-user-playbook.md` 第 17–66 行 — deliverable intake 与 intent routing — 安全核心

**Elvis 逻辑**

- 严格 raw A2A envelope。
- 文件必须是 temp 目录中的普通 0600 文件，禁止 symlink。
- 校验 job/agent/intent。
- deliverable 必须先持久化，之后才能 review。
- 处理乱序 `job_submitted`。
- Subscription admission 和 trade-record 防重放。
- 同时包含一份 user-intent routing 表。

**Contract 逻辑**

- centralized routing 重构时删除整个区块。

**建议**

- routing 表可以去重并改成链接。
- intake、安全、持久化和顺序规则必须保留或完整迁移，不能随 routing 表一起删除。

### C58 — `task-user-playbook.md` 第 130–134 行 — Watch handoff/event ownership — 已决策

- 决策：删除 `sub_open`；将 Elvis 的 session 建立/恢复及附件转发能力合入 `sub_created`。
- `sub_created` 校验权威 Active 状态并展示试用/非试用固定成功文案，不表示“ASP 已接受”，也不做工具扫描、安装或 Guide fallback。

## 建议优先决定的 7 个跨区块问题

### X1 — 创建 API

- [ ] Elvis fixed-price v2
- [ ] Contract 旧 budget/payment-mode
- [x] 组合：v2 + optional Guide bundle

控制 C01、C03、C04、C09–C17、C42、C44、C45、C50、C51、C55。

### X2 — Subscription provider

- [x] 必须指定 provider
- [ ] provider 可选并自动推导

控制 C02、C19、C28、C30、C32、C34、C43、C46。

### X3 — 自动执行授权

- [ ] Elvis `autotrade-*`
- [x] Contract Guide + Consent
- [ ] Guide + Consent 为权威，旧参数只做 deprecated adapter

控制 C02、C19–C22、C26、C29、C30、C35–C40、C43、C46、C48、C52、C53、C56。

### X4 — Subscription remote-write owner

- [ ] Elvis v2 executor
- [ ] Contract inline 旧流程
- [x] 扩展 v2 executor，在现有 v2 流程中加入 Guide lifecycle hook

控制 C22–C25、C27、C31。

### X5 — 输出和 Watch

- [ ] Elvis structured `nextAction`
- [ ] Contract flat data + guidance/Watch 文本
- [x] structured envelope 为权威，文字仅用于展示

控制 C16、C22、C27、C49、C54–C56。

### X6 — Subscription lifecycle 事件职责 — 已决策

- [x] 不存在 `sub_open`
- [x] `sub_created` 建立/恢复 session、逐个转发附件、使用权威详情并展示固定文案
- [x] `sub_asp_selected` 由 ASP 直接开始服务，不进行二次接受

控制 C41、C58。

### X7 — 防重放和交易安全

- [ ] 只依赖 Guide/Consent eligibility
- [ ] 只依赖 Elvis trade-record checks
- [x] 两者同时：Guide 授权 + trade-record/claim/finalize 幂等

控制 C39、C57，以及 Elvis 新增但不在文本冲突中的 spool retirement 逻辑。

## 58 个 marker 之外的隐性冲突

极端 ours/theirs 编译检查还确认了以下问题：

1. Contract 删除了 `config::subscription_trade_path`，Elvis direct prompt 仍调用它。
2. Contract 给 `ConsentSnapshot` 增加必填 `guide_hash`，Elvis 测试初始化器没有提供。
3. Contract 删除 audit match arms，但 Elvis 仍保留旧 route/executor command variants。
4. Elvis 删除 `version_notice.rs`，Contract 选择结果仍声明该 module。
5. 已解决：删除 Elvis 遗留的 `sub_open` dispatch 与实现。
6. Elvis 最新提交删除 `route_subscription_delivery_to_skill` re-export，Contract 选择结果仍尝试 import。
7. Provider delivery command 的字段不一致：`message`、`autotrade`。
8. Elvis-only provider decision、service-param-update、subscription decision commands 仍被 audit/caller 引用，但 Contract command enum 没有这些 variants。
9. `asp_ops.rs` 测试构造 Elvis v2 create command，不能匹配 Contract 旧字段模型。

这些问题应该在 X1–X7 决定后处理，否则很容易先修出一套意外架构。
