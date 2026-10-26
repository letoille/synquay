# 持久化 HTTP 消息转发

新增签名认证与普通 JSON 接入，见 [Webhook 与 MCP 内容能力](webhook-mcp.md)。

现在可以运行第一条真实转发链：认证 webhook → SQLite 事件库 → 路由 → SQLite Outbox → HTTP 目标。默认未启用；没有配置文件时，仅原有健康/版本接口可用，webhook 返回 404。

当前支持静态多目标、目标级调度和独立管理权限，操作方法见[P0 调度、恢复与容量基线](architecture/p0-operations.md)。

## 本地运行

从仓库根目录开始。终端一启动本地接收示例：

```sh
python3 examples/http_receiver.py
```

终端二创建数据目录并设置环境变量：

```sh
mkdir -p data
export SYNQUAY_PIPELINE_CONFIG=examples/pipeline.example.json
export SYNQUAY_WEBHOOK_TOKEN="$(openssl rand -hex 32)"
cargo run -p synquay-server
```

第三个终端需设置与服务端相同的 `SYNQUAY_WEBHOOK_TOKEN`，然后发送事件：

```sh
curl --fail-with-body \
  -H "Authorization: Bearer $SYNQUAY_WEBHOOK_TOKEN" \
  -H 'Content-Type: application/json' \
  --data '{"source_event_id":"demo-001","event_type":"chat.message","payload":{"text":"你好，Synquay"}}' \
  http://127.0.0.1:8080/api/webhooks/00000000-0000-0000-0000-000000000002
```

接收成功返回 `202`，字段为 `event_id` 和 `disposition: stored`。相同来源的同一 `source_event_id` 再次提交返回原 event_id、`disposition: duplicate`，不会产生第二次转发。重复 ID 携带不同内容也不会覆盖原记录，调用方必须为不同事件分配不同 ID。

路由和投递分别每秒扫描一次，每批最多 100 条，慢 HTTP 目标不会阻塞路由扫描；每个 Pipeline 的单目标投递仍串行。示例接收端打印 `Accepted delivery <UUID>`。停止并重启 Synquay 后，未路由的已提交事件和待投递任务会继续处理。接收 202 代表事件已持久化，不能视为已转发给目标。

也可以直接运行自动冒烟测试，无需手动启动任何服务：

```sh
cargo build --locked -p synquay-server
python3 scripts/smoke_pipeline.py
```

该脚本只使用临时数据库和本机随机端口，启动真实二进制，测试投递、SIGTERM 退出、重启去重，最后关闭服务并清理临时数据。

## 配置

示例见 `examples/pipeline.example.json`。所有 UUID 是服务端固定配置，不能由 webhook 正文覆盖。

| 字段 | 含义 |
|---|---|
| database | SQLite 文件路径；相对配置文件所在目录解析，父目录须预先存在 |
| workspace_id | 本运行实例的工作区 |
| source_connection_id | webhook 的来源连接，也是接口路径 ID |
| target_connection_id / destination_id | 目标连接及目的地；来源和目标连接必须不同 |
| principal_id | 只允许在该来源与目标间转发的固定服务主体 |
| rule_id | 当前单条转发规则的稳定 ID，revision 固定为 1 |
| webhook_token_env | 存放入口令牌的环境变量名，令牌至少 32 个可见 ASCII 字符；建议随机生成 |
| destination_url | 操作者指定的固定 HTTP 目标 |
| destination_token_env | 可选环境变量名，向目标发送 Bearer 认证；值不写入数据库 |
| capacity | 可选容量配置；默认每工作区 10000 项、256 MiB 逻辑积压预算，80% 低水位恢复；磁盘低于 256 MiB 拒收，恢复要求 512 MiB，详见[容量说明](architecture/backpressure.md) |
| allow_loopback_http | 默认 false；只有显式 true 时允许使用数字回环 IP 的 HTTP 地址，供本地开发 |

常规目标须为 HTTPS。URL 不接受用户名、密码、query 或 fragment；不跟随重定向，也不使用环境变量代理。HTTPS 目标的网络访问范围由部署环境限制；本阶段没有管理 API 让外部用户选择任意 URL。

数据库绑定当前工作区、来源、目标、规则、主体及目标 URL。修改这些字段后复用原库会启动失败（`pipeline_binding_changed`），避免把旧任务发送到新目标。令牌可轮换；正式配置变更/迁移工具尚未实现，不应手工篡改绑定。每份数据库仅用于本配置的一条转发链。

## HTTP 契约

入口为 `POST /api/webhooks/{source_connection_id}`：

- 先检查连接与 Bearer 令牌，再读取 JSON；令牌使用恒定时间比较。
- `Content-Type` 必须为 `application/json`，可带 charset。
- 请求体上限 64 KiB，读取超时 5 秒。
- 每个 Pipeline 最多同时接纳 64 个正文读取/解析/入库请求，满载在读正文前返回 503 `ingress_busy` 与 `Retry-After: 1`；使用同一来源事件 ID 重试。
- 顶层接受 `source_event_id`、`event_type`、`payload` 以及可选 `content` 引用。ID 上限 256 字节，类型上限 128 字节，两者都不能空白。
- schema_version 在服务端固定为 1，工作区、主体及因果字段从服务端上下文生成。
- 无令牌返回 401；错误 JSON/字段返回 400；类型错误返回 415；读取失败/超限返回 413；读取超时返回 408。
- 积压或磁盘水位超限返回 503 `capacity_exceeded` 与 `Retry-After: 10`；已存在的事件优先去重确认，不重新计费。内部编码超限返回 413。
- 持久化结果无法确认时返回 503，调用方应使用相同 source_event_id 重试。错误响应带服务端 request_id，不包含底层 SQL 或消息正文。

向目标的 POST 正文为独立 v1 DTO，示例：

```json
{
  "schema_version": 1,
  "delivery_id": "<稳定的投递 UUID>",
  "parts": [{
    "type": "json",
    "value": {
      "event_id": "<内部事件 UUID>",
      "source_event_id": "demo-001",
      "event_type": "chat.message",
      "payload": {"text": "你好，Synquay"}
    }
  }]
}
```

请求带相同 delivery_id 的 `Idempotency-Key`，但仅发送该头不能证明目标支持幂等，因此这个通用 HTTP sink 固定为 NonIdempotent。它也支持 Text 内容；附件及 reply_route_id 暂不支持。微信、QQ、钉钉、邮件等需要后续专用适配器或兼容此 DTO 的接收服务，不能直接套用任意平台的 webhook 格式。

## 持久化与状态

SQLite 使用 WAL、`synchronous=FULL`、外键与事务。新建数据库在 Unix 上使用 0600；已有文件权限不擅自修改。SQLite I/O 在 blocking worker 中执行，进程内连接受锁保护，多连接竞争使用 `BEGIN IMMEDIATE`；busy timeout 为 5 秒。内部记录序列化与 HTTP DTO 分开，数据库 `user_version=3`；v1/v2 在事务中迁移并回填容量、路由和目标字段，未知版本拒绝打开。升级前需做一致性备份，升级成功后的回退不能直接使用旧二进制。

每个共享 SqliteStore 普通操作最多接纳 64 个，发送/完成状态等关键操作另有 8 个名额；两类仍共享连接，不保证抢占执行。等待连接使用异步 Mutex、最长 5 秒，取得连接后才提交阻塞任务；满载返回 RateLimited。调用者取消不会释放尚在执行的 SQL 所占容量，也不代表提交被回滚。HTTP 输出借用现有消息直接编码，保持 v1 字段语义；详细约束见 [并发与内存设计](architecture/performance.md)。

| 表 | 保存内容 |
|---|---|
| events | 事件信封，按工作区/来源/source_event_id 唯一去重 |
| dispatches | 路由幂等标记和原任务 ID，空路由也有标记 |
| deliveries | 消息、目的地、状态、attempt、租约、下次执行时间、结果 |
| audit | 主体、资源、操作阶段、关联 ID；不含令牌或消息正文 |
| pipeline_binding | 当前单条转发配置的非秘密身份绑定 |
| routing_tasks | 路由 ready/retry_wait/blocked/completed 状态和退避时间 |
| capacity_policy / capacity_state / pending_work | 共享容量策略、计数和未完成逻辑工作 |

事件容量预留和入库同事务提交；全部目标 submitted 或空路由才释放，failed/reconcile/blocked 仍占额度。容量是逻辑工作预算，不是数据库体积上限。失败路由退避或阻塞后不再占据扫描窗口。只读检查可使用 `python3 scripts/inspect_backlog.py <数据库路径>`，细节见[积压容量与异常恢复](architecture/backpressure.md)。

路由标记与全部任务同事务插入；任何任务失败则整批回滚。并发 worker 只能领取一份有效租约，发送前必须将 Sending 状态落盘，过期或被替换的租约不能改写结果。

- pending：等待执行或下次重试时间。
- leased：已领取但未开始发送；过期可重新领取。
- sending：外部发送阶段；崩溃后过期进入 reconcile，当前存储实现对幂等目标也采用保守核查策略。
- submitted：目标返回 2xx，仅表示 Accepted，未证明收件人收到/已读。
- failed：明确拒绝或前置条件失败，不自动重试。
- reconcile：网络结果不确定、HTTP 5xx/408、非幂等目标的限流/连接失败等，等待核查。

存储端口支持 RetryAfter 的到期调度，应用服务只为可信幂等适配器安排自动重试；当前通用 HTTP sink 不启用自动重试。停止服务时不再领取新任务，当前 HTTP 请求等待其有界结果，已排队任务留在数据库。请求连接超时 5 秒，总时限至多 15 秒，并受调用 deadline 限制。

结构化日志记录投递 ID 和 `accepted / retry_pending / failed / reconcile`，状态或审计保存失败单独记录。健康接口仍只代表进程可响应。运维可用 SQLite 客户端只读查询 `deliveries` 的 state/attempt/result；管理台尚未提供；结果核查/授权恢复接口和 ops.py 已实现，使用独立管理令牌。没有自动过期删除或保留策略；数据库包含事件正文，需要按部署要求备份和保护。使用 SQLite 一致性备份方式，不能在运行中只复制主文件而忽略 WAL。

## 验证范围与后续

`backend/tests/sqlite.rs` 验证并发去重、重开恢复、事务回滚、空路由、租约独占、旧 worker 拒绝、发送中断恢复、重试时间、跨租户与配置绑定。`backend/tests/http_pipeline.rs` 验证真实 TCP HTTP 认证、载荷限制、重启转发、去重、异常状态和重定向限制。

当前是单来源、静态多目标配置的自托管消息转发能力；OAuth、动态路由与管理台、多渠道/多租户配置、自动渠道核查、CLI/MQTT、MCP tools/resources 入口及 AI 原路回复协议尚待接入。此实现不承诺 exactly-once 或高吞吐能力。

## P1 内容与维护

可选 content 配置开启流式上传/下载和 v2 二进制目标协议；可选 retention 配置开启分离保留和每分钟维护。配置、端点、限制和离线备份方式见[内容存储、数据库调度与运行契约](architecture/p1-storage.md)。旧 JSON 目标 DTO 保持 v1；二进制内容必须由支持 v2 的接收服务处理。
