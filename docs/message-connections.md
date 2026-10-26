# MQTT、Webhook 与 HTTP Client

2026-10-26。连接页管理消息连接；已有可暴露为 MCP 工具的 HTTP 操作保留在「HTTP 工具」区域。

## 配置流程

1. 在「连接 → 添加连接」选择 MQTT Client、Webhook 或 HTTP Client，填写名称和协议参数。新连接默认停止。
2. 在「路由」选择接收端作为来源、发送端作为目标。MQTT 同一份 Broker 配置可同时出现在两侧；两个方向分别建立路由。
3. 启用需要的连接与路由。配置在保存后生效，订阅监督器约一秒内启动或停止；无需为配置改动重启服务。环境变量变化仍需要重启后端。

「已启用」表示配置允许运行，不表示 Broker 当前已连通。连接故障由日志和投递状态反映。

## MQTT Client

- MQTT 3.1.1，普通 TCP 和 TLS；默认端口 1883 / 8883。
- 支持用户名、密码环境变量；TLS 默认使用公共 CA，或填写包含 PEM 的 CA 环境变量。支持客户端证书与私钥环境变量，必须成对配置。始终验证服务端证书与主机名。
- 一个订阅过滤器，支持 `+`、`#`；一个固定发布 Topic，不允许发布通配符。两者可单独或同时启用。发布 Topic 不得命中同一连接的订阅过滤器。
- 支持 QoS 0、1；QoS 2、MQTT 5 和 WebSocket 未实现。Retain 是显式的发布选项，默认关闭。
- 订阅保持稳定 client ID 和持久会话；断线后按 1、2、4…30 秒重连。Broker 必须允许持久会话。QoS 1 在 SQLite 确认接收后才 PUBACK；接收失败断开，让 Broker 后续重投。QoS 0 无补投保证。
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

SQLite v16 包含作用域主键、不可变操作审计回执和未完成来源索引；目标检查复用现有未解决投递索引。备份、恢复和检查脚本已兼容 v16。配置保存前验证密钥引用和 TLS PEM；不连接真实设备进行隐式试发。

测试覆盖本机 TCP/TLS、受信 CA 与主机名验证、PUBACK 顺序、断线结果、Webhook 去重、双向 MQTT 路由、工作区/Origin 隔离、配置重启恢复和原子删除引用。运行：`cargo test --workspace --locked`、`npm run test:frontend`。

## 指令与自定义消息映射

接收消息后，可在路由中按正文匹配指令，提取参数并生成 JSON，再发送至 MQTT、HTTP 或 Agent HTTP 接收端。配置、匹配规则及 MCP Agent 部署模式见 [消息指令映射](message-commands.md)。

## QQ 官方机器人

管理台可添加 QQ Bot 双向连接，通过官方 Webhook 接收指定群消息并发送文本回复。配置、指令映射、Agent 往返与平台权限限制见 [QQ 机器人](qq-bot.md)。

路由的协议专属范围与目标配置见 [连接专属路由](connection-routing.md)。
