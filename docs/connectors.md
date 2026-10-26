# 连接器抽象与接入契约

当前实现能力 trait、领域类型、注册表与事件接收用例。HTTP、CLI、MQTT 是协议类别，可以组合不同能力；HTTP webhook、HTTP sink、SQLite 存储和后台扫描已实现；MQTT Client（TCP/TLS、订阅/发布）与 HTTP Client 在线连接已实现，CLI 尚无真实驱动。运行说明见 [持久化 HTTP 转发](http-relay.md)。

## 模块归属

```text
domain/
  connector.rs       连接作用域、协议类别、能力与健康状态
  event.rs           事件输入、内部信封、持久化回执
  message.rs         类型化消息、附件引用、投递与回复路线引用
  tool.rs            工具描述、调用输入和结果
application/
  ports/
    context.rs       可信操作上下文、取消与截止时间
    error.rs         安全错误分类、重试建议
    events.rs        解码、事件接收、事件源、持久化接口
    tools.rs         工具发现与执行接口
    messages.rs      消息投递接口
    lifecycle.rs     健康检查与资源清理接口
  connectors/
    ingress.rs       连接绑定、元数据校验、事件封装和存储委托
    registry.rs      按工作区和连接注册可选能力
```

具体驱动位于 `infrastructure/connectors/`，当前包含 HttpSink、HttpClientSink 和 MqttConnector。在线配置说明见 [消息连接](message-connections.md)。协议入口负责把输入转换为应用调用，领域模型不依赖 HTTP 请求类型、CLI 子进程句柄或 MQTT 客户端。

## Trait 职责

| Trait | 方法 | 契约 |
|---|---|---|
| InboundDecoder | decode | 将原始载荷规范化为 IncomingEvent；鉴权及签名验证应先完成 |
| EventIngress | accept | 等待持久化接收回执，作为生产方的背压边界 |
| EventStore | append | 原子写入与去重；重复事件返回原记录 ID |
| EventSource | run | 持续订阅、流读取或周期采集，接受取消和截止时间 |
| ToolExecutor | discover、invoke | 发布配置好的工具并执行结构化参数调用 |
| MessageSink | deliver | 向授权目的地投递消息，区分 Accepted 与 Delivered |
| ConnectorLifecycle | health、shutdown | 检查健康和释放资源；shutdown 可重复调用 |

所有 trait 均为 `Send + Sync`，异步返回 `PortFuture<'_, T>`（boxed Send future），支持 `Arc<dyn Trait>`。这使运行时可以统一持有不同协议实现，不需要让只有事件订阅能力的连接器实现工具执行方法。

`ConnectorRegistration` 按需持有 decoder/source/tools/sink/lifecycle，能力列表从实际字段推导，不另维护容易失配的声明。`ConnectorRegistry` 以完整 `(workspace_id, connection_id)` 为键，拒绝空能力注册和重复覆盖。注册表支持在调用方发布锁下添加、替换与移除不可变实例；持久化、原子发布与权限决策由运行时和应用服务负责。

## HTTP、CLI 与 MQTT 如何组合

| 接入场景 | 能力组合 | 未来具体实现注意点 |
|---|---|---|
| HTTP webhook 收消息 | HTTP 路由 → InboundDecoder → EventIngress | 限制 body 大小、验证签名/时间窗/重放，再解码；收到持久化回执后才响应接收成功 |
| HTTP/OpenAPI 工具 | ToolExecutor | 调用配置好的 operation；执行输入校验、目标地址约束和授权 |
| HTTP 主动回调 | MessageSink | 使用授权目的地引用，不直接接受模型提供的任意 URL |
| CLI 单次工具 | ToolExecutor | 固定可执行程序与 argv 模板，受控环境、目录、时限和输出大小；不拼接 shell 命令 |
| CLI 持续 stdout 消息 | EventSource + 可选 InboundDecoder | 帧边界、编码、背压和 stderr 分离；取消时停止并回收子进程 |
| MQTT 订阅 | EventSource + 可选 InboundDecoder | 订阅配置、QoS、重连和会话恢复由驱动实现；收到回执后才能推进应用级检查点 |
| MQTT 发布 | MessageSink | destination_id 映射配置好的 topic；broker 接收通常只是 Accepted |
| SDK 直接上报 | EventIngress | 使用已认证且授权的上下文，不接受客户端自报 workspace_id 作为授权依据 |
| 上游 MCP 工具桥接 | ToolExecutor | 选定工具、稳定映射和调用权限检查；资源及提示词端口将在相应实现阶段另行定义 |

MQTT 的 QoS/PUBACK 不是 Synquay 数据持久化证明。有些客户端在应用处理前就确认消息；必须根据所选客户端核实手动确认能力或增加可靠缓冲，不能仅靠此 trait 声称无损或 exactly-once。

## 事件与确认顺序

```text
验证来源身份与权限
  → 规范化 IncomingEvent
  → 绑定连接的 IngressService
  → EventStore.append（原子持久化和去重）
  → IngestReceipt
  → 上游应用确认 / 检查点推进
```

IncomingEvent 只携带事件类型、schema 版本、源事件 ID、可选发生时间和载荷。workspace/connection、接收时间、内部 event_id、correlation_id、causation_id 与 route_trace 来自服务端上下文；载荷里的同名字段不会覆盖这些信封字段。内部信封使用 SystemTime 和类型化模型，公开 wire schema 与数据库编码需另做映射。

IngressService 已实现：作用域一致性检查、取消/超时的提交前检查、基本元数据校验、重复路线与 32 跳上限检查、信封生成及 tracing span。内容 schema、大小限制与授权检查由相应边界实现，当前并不具备完整策略引擎。

去重键为 `(workspace_id, connection_id, source_event_id)`。EventStore 的具体实现必须原子保证此约束；没有 source_event_id 时不默认按内容去重。返回 Duplicate 时使用原事件 ID。测试中的内存 StoreFixture 只验证契约，不是生产持久化实现。

提交开始后不简单用取消信号丢弃存储 future，避免数据库已经提交却被错误报告为未接收。存储实现应设置自身有界 I/O 超时；若最终提交状态未知，应返回 OutcomeUnknown 并由上层协调核查。消费者在等待期间必须保留任务，直到有回执或监督器按明确的恢复策略处理。

## 调用上下文与副作用

OperationContext 携带工作区/连接、内部主体 ID、请求和因果关联 ID、可选 deadline 与 CancellationToken。它由可信编排代码在完成授权后创建；拥有一个 context 对象不代表自动获得权限。当前 HTTP webhook 先完成认证再调用接收服务，不直接对外暴露 trait。

适配器在调用前使用 `check_active()`，持续运行时监听 `stopped()`。取消是协作信号，不等于撤销已发出的 HTTP 请求、回滚外部副作用或自动杀死 CLI 子进程。具体实现必须完成资源清理；连接关闭时给 shutdown 新的清理 deadline/token，不能复用已经取消的运行 token。

工具通过稳定 tool_id 和 JSON 参数调用配置好的能力，ToolEffect 只是提示。MessageSink 使用授权 destination_id 和稳定 delivery_id；reply_route_id 是服务器保存的原路线引用。DeliveryService 已通过 DestinationRepository 解析并校验目的地，ReplyService 已实现绑定路线、所有者和有效期校验。附件访问校验和真实地址解析由后续基础设施实现。

PortError 不携带自由格式的上游错误正文。错误重试建议为 Never、IfIdempotent 或 Reconcile。Unavailable/RateLimited 只有在可安全重放或上游支持幂等键时才重试；DeadlineExceeded/OutcomeUnknown 先核查结果。ToolResult.is_error 表示工具本身报告失败，与适配器/传输失败不同。

## 验证与后续实现

`backend/tests/connectors.rs` 使用模拟 HTTP decoder、CLI executor 和 MQTT source/sink，验证动态 trait 组合、作用域隔离、去重键、因果信息、存储失败、背压确认顺序、取消及超时。测试不执行真实命令、不连接 broker，也不发送外部消息。

发布、权限、审计、路由与投递应用服务已建立，详见 [模块抽象与对接](module-integration.md)。持久化 EventStore 与 HTTP webhook 垂直调用链已经接通，MQTT 订阅/发布驱动已接入同一持久化路由；接下来可增加受限 CLI executor。每个驱动必须补充自身的协议、限流、取消和失败恢复测试。

MQTT 协议测试使用本机 TCP/TLS 测试 Broker，验证手动确认、证书信任、断线后的未知结果；`backend/tests/message_connections.rs` 覆盖实际订阅 → HTTP 和 Webhook → MQTT 路由、在线配置与重启恢复。
