# MQTT、Webhook 与 HTTP Client

2026-10-26。连接页管理消息连接；已有可暴露为 MCP 工具的 HTTP 操作保留在「HTTP 工具」区域。

## 配置流程

1. 在「连接 → 添加连接」选择 MQTT Client、Webhook 或 HTTP Client，填写名称和协议参数。新连接默认启用。
2. 在「路由」选择接收端作为来源、发送端作为目标。MQTT 同一份 Broker 配置可同时出现在两侧；两个方向分别建立路由。
3. 新路由也默认启用，保存后开始处理匹配消息；需要暂存时可以关闭启用。配置在保存后生效，订阅监督器约一秒内启动或停止；无需为配置改动重启服务。环境变量变化仍需要重启后端。

## 运行状态与统计

连接与路由页每五秒刷新运行数据；打开编辑窗口时暂停刷新，浏览器标签隐藏时跳过请求。

- MQTT 显示连接中、订阅中、Broker 已连接、重连中或连接异常。只有收到成功 CONNACK，且配置了订阅时收到成功 SUBACK，才显示已连接。仅发布连接也保持一个独立监测会话，不会自动发送测试消息；发送仍使用独立短连接。已连接说明 Broker 会话就绪，不保证发布 Topic 的 ACL 或设备执行成功。
- Webhook、QQ 显示「接收就绪」，表示本地接收入口已启用，不代表 QQ 已验证回调或授予权限。HTTP Client 显示「按需发送」，不伪造持续在线状态。停用连接统一显示「已停止」。
- 「已接收」统计成功落库的新事件，已识别的重复事件不增加；MQTT 没有业务事件 ID 时无法识别重投，见下文限制。「已发送」统计已记录为 submitted 的投递，每个 delivery ID 只计一次；第三方接收成功不等于设备执行成功。
- 路由「已匹配」统计成功提交投递队列的输入消息，每条路由每个 dispatch 计一次，多出口不重复累计。未匹配或生成投递失败的不计入；已提交、待发送、需处理仍是保留中的投递状态数量。
- 累计统计存放 SQLite v19 的 `message_totals`，重启、停用、历史消息清理不会清零。升级时从仍保留的事件、投递与当前路由映射回填；升级前已删除的历史无法补算。作用域组合主键 `(workspace, kind, id)` 覆盖查询，无额外冗余索引。

`GET /api/message-connections` 在配置 snapshot 之外返回 observations，按连接 ID 包含 status、received、sent；路由列表接口返回按路由 ID 索引的 matched。计数不混入配置版本和写入请求。

## 重连与重试

MQTT 接收／监测会话保持重连，停用、删除或服务关闭立即取消等待；普通 TCP 与 TLS 使用相同策略，每次连接超时五秒。不会降级 TLS 或绕过证书验证。重连成功还须重新确认订阅。

HTTP、QQ 是按请求建立连接；Webhook 和 QQ 回调入口由发送方发起连接，不存在本地主动重连。发送端明确在 DNS / TCP / TLS 建连阶段失败、MQTT 发布尚未排入发送队列、QQ 取 token 失败时，可在持久化投递队列自动重试；HTTP / QQ 明确限流拒绝也按同一机制等待。每条投递最多尝试八次，等待时间使用相同的指数退避与随机抖动，且不早于服务端 Retry-After（支持秒数和 HTTP 日期；超过 24 小时则停止自动重试并保留失败任务，不擅自提前发送）。QQ HTTP 401 会清除缓存 token，下一次有界重试重新取 token。

等待同时约束整个出口，避免队列后续消息绕过限流；复用每个出口的连续失败熔断（默认三次失败、冷却三十秒）、全局并发限制和持久化 due 时间。等候期间不占发送并发许可，重启保留每条投递的尝试次数与到期时间；进程内的连接退避和出口熔断窗口会重新计算。八次仍失败后保留失败任务供处理，不丢弃消息。

发送后超时、断线、5xx 等结果不明的消息进入待核查，不因“重连”自动重放。无权限、无效参数等永久错误停止该投递。HTTP 持久化任务在重试时保留同一 delivery ID 和 Idempotency-Key；不承诺第三方一定执行幂等。

## 消息链路日志

默认 info 记录 `message.received`（新事件或重复事件）、`route.matched`、`route.dispatched`、`message.send_started`、`message.send_completed`、`connection.state_changed`、`connection.retry_scheduled`；warn 记录接收失败、发送失败、重试与待核查结果。debug 额外记录接收开始、路由求值、目标匹配与领取投递。

通过 event_id、delivery_id、route_id / route_rule_id、connection_id、correlation_id 关联链路；发送结果包含耗时与尝试次数，重试结果包含延迟和是否已保存状态。日志不记录完整消息正文、AppSecret、token 或第三方响应正文。路由匹配日志在数据库提交后输出，发送完成日志要求投递结果已落库。

Debian 服务日志为 `/var/log/synquay/synquay.log`，每日轮转、保留 14 份，压缩旧档；详见 [日志与归档](debian.md#日志与归档)。开发运行仍输出 stderr。

## MQTT Client

- MQTT 3.1.1，普通 TCP 和 TLS；默认端口 1883 / 8883。
- 支持用户名、密码环境变量；TLS 默认使用公共 CA，或填写包含 PEM 的 CA 环境变量。支持客户端证书与私钥环境变量，必须成对配置。始终验证服务端证书与主机名。
- 一个订阅过滤器，支持 `+`、`#`；一个固定发布 Topic，不允许发布通配符。两者可单独或同时启用。发布 Topic 不得命中同一连接的订阅过滤器。
- 支持 QoS 0、1；QoS 2、MQTT 5 和 WebSocket 未实现。Retain 是显式的发布选项，默认关闭。
- 订阅保持稳定 client ID 和持久会话；断线后使用带随机抖动的指数退避：1–2 秒、2–4 秒、4–8 秒，逐步增加至 30–60 秒；持续就绪 60 秒后重置退避，避免短暂连上又断开时高频重试。Broker 必须允许持久会话。QoS 1 在 SQLite 确认接收后才 PUBACK；接收失败断开，让 Broker 后续重投。QoS 0 无补投保证。
- 默认的界面配置勾选「附带 Topic 信息」，接收内容为 `{"topic":"home/heater/state","qos":1,"retain":false,"payload":{...}}`；取消勾选时只保留原载荷。非 JSON 载荷转换为 `{"encoding":"base64","data":"..."}`，不做有损字符串转换。
- 当前没有将 MQTT packet ID 当成业务事件 ID。断线、崩溃、重订阅与 retained 消息可能产生重复事件；不保证 exactly-once。跨连接、跨系统循环需要通过不同 Topic 和路由约束避免，内部跳数无法跟踪外部重新注入的消息。
- 发布采用独立短连接，QoS 1 等待 PUBACK，QoS 0 等待 socket 写出。回执均为 Accepted，不能证明设备执行。写出后断线或超时进入待核查，不在后台自动重发。高吞吐场景的复用发布会话可在后续优化，需同时保持这项不确定结果约束。

## Webhook

地址自动生成：`/api/webhooks/{connection_id}`。每个 Webhook 独立配置 Bearer 或 HMAC SHA-256 认证，密钥引用后端环境变量（32–4096 个可见 ASCII 字符）。

接收格式：

- JSON：配置事件类型和事件编号请求头（默认 `x-event-id`），请求体即原始内容。事件编号必填，用于重试去重。
- Synquay Event：请求体包含 `source_event_id`、`event_type`、`payload`。

须使用 `Content-Type: application/json`。成功落库后返回 202；相同来源与事件 ID 的重复内容返回原事件 ID，不再次分发；不同内容复用同一 ID 返回 409。HMAC 使用既有签名契约 `v1\n{connection_id}\n{timestamp}\n{delivery_id}\n{raw_body}`，请求头为 `x-synquay-timestamp` 和 `x-synquay-signature: v1=<hex>`，有效时间窗五分钟。

停用后该接收地址返回 404。HTTP 客户端断开不会打断已接纳的落库与配置发布保护。

## HTTP Client

- 固定 HTTP/HTTPS URL，POST、PUT、PATCH。普通 HTTP 需要显式允许。
- 无鉴权、Bearer 或 Basic。密码/令牌引用环境变量，不存储凭据明文。
- 「原始内容」发送路由事件中的 payload；「完整事件」发送包含事件编号、类型和 payload 的信封。MQTT 开启 Topic 信息时，该信息也是 payload 的一部分。
- 发送稳定 `Idempotency-Key`，但不能假设远端实现了幂等。2xx 记为 Accepted；429 遵守 Retry-After；发送后的超时、断线、408、5xx 进入待核查。禁止跟随重定向，不将模型输入用作请求 URL。
- 连接超时五秒，总请求时限十五秒。当前不是通用 OpenAPI 客户端，不提供任意自定义请求头或响应映射。

## 限额、持久化与安全更新

当前最多 32 个在线消息连接；MQTT 单包载荷与 HTTP 消息体上限 64 KiB，最终事件还受存储编码预算约束。大附件继续使用已有内容引用流程，这些新增适配器暂不发送附件。订阅按落库回执施加背压；出口独立调度，并沿用全局并发上限、发送间隔、熔断和持久化 Outbox。

`GET/POST /api/message-connections` 使用既有管理台来源与工作区校验；管理台完整登录鉴权仍遵循项目当前阶段的约定。修改请求含 `operation_id`、`expected_revision`、`connection_id`、`connection`，其中 `connection:null` 表示删除。相同操作编号与相同内容可安全重试；版本冲突返回 409。浏览器在请求前保存操作内容，未知结果保留原编号重试，不自动重复创建。

删除连接会在同一事务中清理路由引用：删去对应来源的路由、去掉对应目标，最后一个目标被移除时删除该路由。历史消息保留。有未分发/未解决消息时，协议配置修改及删除被拒绝，启停和名称修改仍允许；出口停止期间积压保留，恢复后继续。不会把旧队列静默改投新的地址。

SQLite v16 包含作用域主键、不可变操作审计回执和未完成来源索引；目标检查复用现有未解决投递索引。备份、恢复和检查脚本已兼容 v19。配置保存前验证密钥引用和 TLS PEM；不连接真实设备进行隐式试发。

测试覆盖本机 TCP/TLS、受信 CA 与主机名验证、PUBACK 顺序、断线结果、Webhook 去重、双向 MQTT 路由、工作区/Origin 隔离、配置重启恢复和原子删除引用。运行：`cargo test --workspace --locked`、`npm run test:frontend`。

## 指令与自定义消息映射

接收消息后，可在路由中按正文匹配指令，提取参数并生成 JSON，再发送至 MQTT、HTTP 或 Agent HTTP 接收端。配置、匹配规则及 MCP Agent 部署模式见 [消息指令映射](message-commands.md)。

## QQ 官方机器人

管理台可添加 QQ Bot 双向连接，通过官方 Webhook 接收指定群消息并发送文本回复。配置、指令映射、Agent 往返与平台权限限制见 [QQ 机器人](qq-bot.md)。

路由的协议专属范围与目标配置见 [连接专属路由](connection-routing.md)。

## MQTT tools and resource state

The MQTT editor now includes **Tools** and **Resource state**. Each connection owns
its capability configuration (`config.capabilities`); credentials and Broker settings
remain on the connection. The smart-switch example fills four tools (on, off,
query basic information, query power information) and one resource with two groups.
It only fills the form and does not send commands.

Tools declare an ID, description, JSON input schema, exact publish topic and a
structured payload mapping. Fields use literals, JSON Pointer arguments or the
invocation ID. No shell, JavaScript or interpolated JSON is executed. Tools publish
QoS 1 with retain disabled. Optional device acknowledgements specify a reply topic,
correlation ID pointer, result pointer, expected scalar and timeout (100–15000 ms).
The executor subscribes and waits for SUBACK before publishing. Retained replies and
unrelated correlation IDs cannot confirm an invocation. `submitted` means Broker
PUBACK; `confirmed` means a matching successful device response; `unconfirmed` means
the command may have executed. Unknown outcomes are not automatically retried.
Calls are bounded to eight concurrent executions and 64 KiB payloads.

A resource contains up to eight named state groups, with up to 32 fields each. Groups
independently configure exact report topic, optional identity/report discriminator,
field mappings, merge/replace behavior, freshness and an optional refresh tool.
`max_age_ms: 0` means no age expiry (useful for model/firmware identifiers); time-sensitive
power data should use a finite age. A mapping's JSON Pointer addresses the device's
JSON body, regardless of the connection's `include_metadata` setting. Boolean equality
mapping supports e.g. `/key == 1`. Omitted or wrongly typed fields do not refresh old
values. Merge is the default; replace clears missing fields within that group only.

State is persisted with each field's receipt time, optional device timestamp and
retained provenance. Device timestamps are Unix milliseconds; invalid/future timestamps
are ignored. Out-of-order reports with source timestamps cannot replace newer group
observations. Retained data without a source timestamp is unknown-age and cannot
replace existing live observations. State projection and event ingestion share a
transaction before the source sends PUBACK. Reports still enter the existing routing
pipeline, so users can forward them to another connection or an Agent as usual.
Raw report forwarding is supported; automatic state-change-derived events are not
introduced by this release.

Resource reads are exposed as `state_<resource-id>` tools, with optional `group` and
`refresh` arguments. Without refresh they only read stored observations. Refresh of a
multi-group resource requires selecting one group and uses its configured tool plus
matching acknowledgement. The UI displays all groups and per-field freshness and
receipt time. This release exposes state through query tools and the console API,
not through native MCP `resources/read` subscriptions. No command writes desired state
into observed state; only device reports do.

In each MCP service's editor, select the connection tools to publish. Public names
include the connection ID to avoid collisions. Route commands can also invoke the
same tools and send the result back to the originating QQ conversation; sender
allowlists still apply. Merely defining a tool does not grant MCP access.
Changing MQTT bindings revokes previous MCP publications and disables affected
command routes until reviewed; removing an action removes routes invoking it.
Connection metadata and enabled-state changes preserve bindings and observations.

SQLite schema v20 adds `connection_devices` and `resource_state`, indexed by scoped
primary keys: workspace/connection and workspace/connection/resource/group/field.
Only configured resources and fields are retained; changing transport or mappings
invalidates old observations. The console reads state using
`GET /api/message-connections/{id}/states` with the existing console access checks.
