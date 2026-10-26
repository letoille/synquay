# 模块抽象与对接

已实现业务契约和应用服务编排，并新增 SQLite 持久化与可选 HTTP 转发运行链；OAuth、MCP 入口、消息平台及 CLI/MQTT 驱动待接入。运行方式见 [持久化 HTTP 转发](http-relay.md)。所有外部依赖通过 `Arc<dyn Trait>` 注入，服务之间使用领域模型，不交换 HTTP DTO、SDK 对象或数据库连接。

## 模块与职责

| 模块 | 外部依赖 trait | 应用层负责 |
|---|---|---|
| identity | Authenticator | 从验证后的身份绑定主体，检查租户、有效期和运行上下文 |
| policy | PolicyEngine | 在列举、执行、路由、投递与回复时进行权限决策 |
| profiles | ProfileRepository、SchemaValidator | 使用已发布快照，映射公开工具名、校验版本和参数 |
| audit | AuditLog | 操作开始前持久化审计，分别报告执行结果与完成审计结果 |
| events / routing | EventStore、EventReader、RoutingRules、MessageRenderer | 可靠接收、读取已提交事件、匹配规则、生成消息并防止转发循环 |
| delivery | DestinationRepository、DeliveryOutbox、MessageSink | 解析目的地、原子排队、租约领取、发送前落盘、按结果决定恢复方式 |
| replies | ReplyRouteStore | 从服务端保存的路线解析原目的地，检查所有者与有效期后复用投递链 |
| connectors | ToolExecutor、EventSource 等 | 提供具体协议能力，保持与业务编排解耦 |

代码入口为 `backend/src/application/modules.rs`。`ModuleDependencies` 要求显式提供所有依赖，`BackendModules::new` 组装 identity、tools、routing、delivery、replies，并共享 Governance、注册表、目的地仓储和 Outbox。没有默认放行策略或自动注入的内存数据库。

`lib.rs` 的 `AppState` 包含系统信息服务及可选 Pipeline。静态单路由 Pipeline 直接装配所需路由/投递/治理服务，复用同一 SQLite 存储和真实 HTTP sink，不为未使用的工具发布依赖填入假实现。完整 BackendModules 继续用于未来 MCP 等完整服务入口。测试替身不进入运行时。

## 工具发布与调用

```text
HTTP / MCP 入口
  → IdentityService.authenticate → AccessContext
  → ToolService.list(profile_id)
      → UseProfile 授权 → 读取发布快照 → 按 InvokeTool 权限过滤
      → 返回 profile_id、revision、公开工具定义
  → ToolService.invoke(profile_id, revision, public_name, arguments)
      → 再次授权 → 校验版本 → 解析内部连接/工具 → Schema 校验
      → Started 审计落盘 → ToolExecutor.invoke → 输出校验
      → 完成审计 → ToolInvocationReport
```

- 列举和执行复用同一个权限入口，不能通过猜测内部工具 ID 绕过 Profile。旧 revision 返回 Conflict，调用方需重新读取发布目录。
- 快照检查工作区隔离、公开名称唯一性、合法版本和 Schema 基本形状；完整 JSON Schema 语义由 SchemaValidator 实现。发布编辑、编译 Schema 和管理 API 尚未实现。
- 身份包含主体、工作区、客户端、scope 和有效期。Authenticator 实现须验证签名或 introspection、issuer、audience、有效期及租户绑定。PolicyEngine 默认拒绝，综合授权范围与资源权限；当前没有真实 OAuth server 或 token 验证器。
- 参数和凭据不进入审计记录。Started 审计失败时禁止执行；执行后的审计失败通过 `audit_recorded=false` 返回，保留实际执行结果，避免诱发重复副作用。
- `ToolInvocationReport.outcome` 表示已经进入执行阶段后的结果。外层 Err 表示前置校验失败。输出不符合 Schema 也不能解释为工具未执行。
- `invocation_id` 用于关联和适配器幂等能力，本阶段没有持久化工具调用账本，不能承诺任意工具调用去重。应用层不自动重放工具。
- 当前发布模型覆盖 tools；resources、prompts、订阅和异步任务将在对应协议实现时增加独立端口。

## 事件转发

```text
HTTP / CLI / MQTT / SDK
  → IngressService → EventStore.append → 持久化回执
  → 后台扫描 EventReader.unrouted
  → RoutingService.route
      → 读取已提交事件 → RoutingRules.matching
      → 校验规则/目标/权限/转发路径 → MessageRenderer.render
      → DeliveryOutbox.enqueue（路由标记 + 全部投递任务，同一事务）
  → DeliveryService.run_one
      → claim 租约 → 校验当前目的地和权限 → Started 审计
      → mark_sending 落盘 → MessageSink.deliver
      → finish + 完成审计 → DeliveryRunReport
```

EventStore 和 EventReader 的具体实现应读取同一个事件库。`unrouted` 必须从持久化的路由标记查询待处理事件，支持在接收成功、尚未路由时崩溃后的恢复。可选 Pipeline 已实现每秒一次的后台扫描。

Outbox 以完整工作区/来源连接/event_id 为路由幂等键；同一次事件的路由标记及全部任务原子提交，空路由计划也写标记。重复处理返回原任务 ID，不随规则变更覆盖已提交计划。每个任务保存 rule_id/revision，重试使用原 delivery_id 和原消息。任何一个规则校验失败时整批不排队，后续需由管理端修复或明确重放。

路由使用已存信封，不信任调用方重新提交的事件正文；保留 correlation_id、causation_id、route_trace。转发消息会清除载荷中的 reply_route_id。路径按连接限制循环和最多 32 跳，因此同一连接内不同频道互转也可能被拒绝；未来如要支持，应引入更细粒度目的地路径与明确规则。

## 投递状态与恢复契约

| 时点/结果 | 持久化实现必须保证 |
|---|---|
| enqueue | 唯一幂等键、原子标记与任务插入，返回原任务 ID |
| claim | 租户内原子领取，生成独占 lease_id 与过期时间，递增 attempt |
| mark_sending | 使用租约与有效期做 CAS，在任何外部 I/O 前持久化 Sending |
| Submitted(Accepted) | 平台已受理，不能视为收件人已收到或已读 |
| Submitted(Delivered) | 使用适配器明确提供的交付语义 |
| RetryAfter | 仅可信配置标为幂等、错误允许重试且 attempt < 5 时自动重排 |
| Failed | 终止自动重试，保留可检查的失败结果 |
| Reconcile | 结果不确定，等待查证或人工处理，不盲目重发 |

当前服务申请 60 秒租约，适配器 deadline 取原调用 deadline 与租约截止时间的较早值。适配器必须协作停止并报告不确定副作用；服务不通过丢弃 future 假装撤回已发出的请求。超时的旧 worker 不能覆盖新租约状态。生产实现需处理时钟误差、租约到期及幂等键有效窗口；长任务可后续增加续租端口。

过期的 Sending 行，只有确认上游可按同一 delivery_id 安全重放时才可重领，否则转 Reconcile。领取前尚未发送的过期租约可重新排队。幂等能力由可信驱动配置决定，不能来自用户消息或 AI 声明。

重试默认指数退避；适配器提供 retry_after 时优先遵从。`OutcomeUnknown` 与超时先核查；无法证明幂等的暂时错误也进入 Reconcile。发送完成但 finish 失败时，报告 `state_recorded=false` 并保留实际发送结果；完成审计失败独立报告，不能触发新建投递任务。

SQLite 实现已补齐事务竞争、重开恢复、租约接管、重试时间和状态迁移测试；发送中断统一进入 Reconcile，不自动重放。其他持久化实现仍需满足相同合同。

## AI 分析后原路返回

```text
消息接入并持久化
  → 服务端建立 ReplyRoute（未来接入流程负责）
  → 经授权的 MCP 工具/资源向 AI 提供消息与 route_id
  → AI 回传 route_id + reply_id + 内容
  → ReplyService 校验路线工作区、所有者、有效期、目的地权限
  → Outbox → DeliveryService → 原聊天渠道
```

ReplyService 不接受模型给出的 URL、邮箱或 topic。服务端路线绑定 destination_id、owner_principal_id、原事件与关联 ID；重复 reply_id 返回原投递任务，复用同 ID 提交不同内容必须返回 Conflict。明确授权的回复从新转发路径开始，因此可以回到原连接。

路线创建、AI 可读事件的权限过滤、MCP 客户端通知和 AI 执行器尚未实现。此调用链不代表可以主动唤醒所有 Agent 平台。附件引用访问、目的地地址解析和真实外部发送由相应驱动实现。

## 验证范围与下一步

`backend/tests/module_flows.rs` 通过依赖注入测试三条链：工具调用、持久化事件转发、原路回复。覆盖跨租户拒绝、过期身份、发布版本、隐藏工具绕过、Schema 错误、审计故障、事件重复、转发循环、投递前重新授权、发送前落盘、不确定结果、重放条件、回复所有者及过期租约。

测试中的认证器、策略、Schema 校验器和内存仓储仅用于契约验证，不具备生产鉴权、完整 JSON Schema 或持久性。当前整仓共 37 项 Rust 测试；`npm run check` 同时检查前端 TypeScript、Rust 格式、Clippy 与测试。

SQLite EventStore/EventReader/Outbox/AuditLog、HTTP webhook、HTTP MessageSink 和后台恢复调度已接通。下一步增加可管理的核查/重放流程与配置，再扩展 OAuth、MCP、CLI、MQTT 及其他渠道驱动。
