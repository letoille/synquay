# P1 内容存储、数据库调度与运行契约

2026-10-24。P1 基础实现已接入现有单进程、单工作区、静态多目标 HTTP 链路。数据库升级至 v4；大内容独立存储、公平优先调度、保留清理、备份恢复、来源检查点和配置快照均有可执行实现。动态配置发布、OAuth/MCP 入口和真实 MQTT 驱动仍属于后续阶段。

## 模块边界和调用链

- domain：新增 ContentRef；EventPayload::Content、ContentPart::StoredContent 只携带受保护的内容引用。
- application/ports：ContentStore 以 AsyncRead 输入/输出；SourceCheckpoints 管理 generation/revision；EventStore.append_checkpoint 和 IngressService.accept_checkpoint 提交来源游标与事件。
- infrastructure/content：FileContentStore 管理私有文件、分块摘要、授权、配额及孤儿回收。
- infrastructure/storage：SQLite v4 提供引用绑定、检查点事务、版本化记录、保留清理与公平数据库调度。
- interfaces/http：认证后的流式内容上传/下载；既有 JSON webhook 可以提交内容引用。
- DeliveryService：发送前校验当前授权、目的地和适配器支持的消息形态；HttpSink 显式启用二进制线协议后才能发送 StoredContent。

内容链路为：预留配额 → 流式写文件并同步 → staged 元数据 → webhook 事务绑定事件/引用/路由任务 → Outbox 保存引用 → 发送时流式读取 → 全部投递结束后进入保留窗口 → 清正文及引用 → 回收文件。文件系统与数据库不伪装成一个事务；崩溃留下的预留或孤儿由状态机收敛。

## 大内容与流式传输

ContentRef 保存 UUID、来源工作区/连接、长度、media_type 和 SHA-256，不接受宿主路径或远程 URL。上传使用 64 KiB 工作缓冲，增量计算摘要，flush/sync 文件和目录后才发布引用；读取时增量校验摘要及长度，损坏数据返回错误。分发到多个目标时，只重复小引用，数据库不复制二进制正文。

默认未启用。设置 pipeline 的 content 即显式接受二进制目标协议：

```json
{
  "content": {
    "directory": "../data/content",
    "max_object_bytes": 67108864,
    "max_total_bytes": 1073741824,
    "max_objects": 10000,
    "concurrent_uploads": 4
  }
}
```

directory 相对配置文件解析；Unix 新建目录 0700、文件 0600，已有目录权限过宽时拒绝启动。目录规范路径固定绑定数据库，避免重启时悄悄切换内容位置；离线恢复脚本提供显式重定位。总配额包括上传中、staged、available 及待删除内容，由事务计数器维护；先预留完整长度，再读取正文。内容磁盘另检查剩余空间，并保守扣除未结束上传的预留字节。逻辑积压预算仍计事件/投递记录，内容磁盘配额独立计费。

HTTP 契约：

1. `POST /api/content/upload/{source_connection_id}`，使用来源 Bearer 令牌、Content-Length 和 Content-Type，正文为原始内容。返回 201 和 ContentRef。缺少长度返回 411；长度不符拒绝。HTTP 上传沿用 30 秒操作期限，ContentStore 自身最多允许 300 秒。
2. `POST /api/webhooks/{source_connection_id}`，提交原来的 source_event_id/event_type，加 `payload:null` 和 `content:<完整 ContentRef>`。仅与本来源匹配、元数据一致、尚可用的引用能原子绑定。原 JSON webhook 的 64 KiB 限制不变。
3. `GET /api/content/{id}` 使用相同来源认证，最多 16 个并发下载；返回 attachment、octet-stream、no-store，避免浏览器执行上传内容。
4. 二进制目标 POST 是原始字节，带 Content-Length、Content-Type、Idempotency-Key、X-Synquay-Schema-Version: 2、X-Synquay-Content-Id、X-Synquay-Content-Sha256。目标必须支持该契约。普通文本/JSON 继续使用 v1 JSON DTO。

目前支持一个内容引用构成一条二进制消息，不支持多附件 multipart、正文加附件、断点续传或任意引用的交互式回复。后者明确拒绝，不能绕开事件的内容保留关系。2xx 只代表目标接受；15 秒目标超时和未知结果核查规则不变。传输不是内核零拷贝，仍有磁盘、HTTP/TLS 缓冲和摘要计算。

取消上传触发尽力立即清理，配额在文件删除并确认元数据删除后释放。进程崩溃时，超过一小时的未绑定内容由每分钟清理任务处理，每轮最多 100 个；正在被事件引用的内容不会清理。失败清理保留 deleting 状态，后续继续重试，不丢失待清理账目。

## 数据库执行调度

保留普通 64、关键 8 个接纳名额。在连接锁之前增加真正的执行选择器：关键写入优先，最多连续 4 个，随后给一个等待的普通操作执行机会；同类 FIFO。等待超时 5 秒，取消时移除队列条目。已执行 SQL 的执行权和接纳许可一直保留到事务结束，不因调用者取消而提前释放。

调度不抢占正在运行的 SQL，也不跨多个进程协调。维护使用普通通道，不占用完成状态等关键写入的保留名额。公平选择不能保证磁盘卡顿时的绝对延迟。

## 保留策略与维护

默认不自动删除历史。显式配置 retention 后，每分钟执行一次有界维护：

```json
{"retention":{"payload_days":7,"audit_days":30,"dedup_days":90,"batch_size":100}}
```

正文和去重保留时间从整个工作组终结时开始，旧库已完成工作从升级时开始计时。一个目标仍未完成、failed/reconcile 未处理、blocked 路由未解决时，不进入清理。人工放弃视为已明确终结。

正文窗口到期后删除事件/投递正文；所有投递正文清完后才释放内容引用。事件 ID、来源去重键、投递回执与最小状态继续保留，重复 webhook 返回原 event_id，不会再次转发。达到 dedup_days 后，按外键依赖顺序分批删除最小记录；此后相同来源 ID 可以被当作新事件，使用方必须了解这个窗口。

每条清理 SQL 最多修改 batch_size 行（1–100），整个维护轮包含多条 SQL，并非总共仅 100 行。普通审计按 audit_days 独立清理；恢复操作的证据和命令幂等记录暂永久保留。保留窗口没有 TTL 丢弃未完成任务的语义。

维护执行 PASSIVE WAL checkpoint；活跃读者可使 WAL 页暂时保留。删除记录不会立即缩小主库文件。物理压缩使用离线 compact，不能在正常请求路径自动 VACUUM。

## 备份恢复

`scripts/backup.py` 使用 SQLite backup API，加复制受引用和 staged 文件、SHA-256 manifest、quick_check 和 foreign_key_check。必须停止服务及所有写入/清理进程，再显式传 --offline；脚本不自动停止生产服务，也不能检测远端进程。现有目标目录不覆盖，失败的半成品必须核查后处理。

```sh
python3 scripts/backup.py backup /absolute/data/synquay.db /absolute/backups/snapshot --offline
python3 scripts/backup.py verify /absolute/backups/snapshot
python3 scripts/backup.py restore /absolute/backups/snapshot /absolute/restored --offline
python3 scripts/backup.py compact /absolute/restored/database.sqlite3 --offline
```

恢复后将配置 database 指向 restored/database.sqlite3、content.directory 指向 restored/content，再启动。备份保留待投递状态、去重标记和内容引用；恢复数据库不能回滚外部已经发生的消息发送，未知结果仍需核查。

## 来源确认与存储版本

SourceCheckpoints.activate 使用预期 generation 做 CAS，接管后拒绝旧实例提交。append_checkpoint 同事务写事件、去重结果、容量预留、路由任务和 cursor；容量拒绝不推进游标。游标还携带预期 revision，重复的不确定提交只在游标和已提交事件一致时复用。来源需要稳定 source_event_id；适配器只能在收到提交回执后确认上游。

这些端口已由真实 SQLite 和 IngressService 实现，尚未接真实 MQTT broker、CLI 驱动或 SDK 网络确认协议，因此不宣称这些渠道已具备端到端确认保证。

schema v4 与 storage_version 分开：事件及投递新写入采用 storage_version=2 的 DTO 外壳，兼容读取旧无外壳记录；未知版本拒绝。v1→v2→v3→v4 在同一迁移事务，失败回滚，不修改用户已部署的数据库。

config_releases/active_release 持久化脱敏启动快照。事件记录当前快照引用，自动投递继承事件版本，人工重放继承原投递版本；相同快照复用版本。工作区、来源、目标 URL、主体与路由身份仍保持不可变绑定；限速/保留等运行策略可以随重启变化。当前实现用于追溯，不是完整的动态发布、审批、撤销或历史策略执行引擎。

## 按访问路径建立索引

索引由 v4 迁移创建，用到的热路径及原因如下。主键本身已经提供的索引不重复创建。

| 查询路径 | 索引/实现 | 作用 |
|---|---|---|
| 指定目标领取 pending | delivery_target_pending(workspace,target_id,due_ms)，部分索引 | 状态固定后按 due_ms,rowid 取首条，无全量排序 |
| 严格顺序检查前驱 | delivery_unresolved_target(workspace,target_id)，部分索引 | 只查同目标未解决工作及 rowid 范围 |
| 过期租约 | delivery_expired(workspace,state,lease_until_ms)，非空租约部分索引 | 过滤已结束的历史投递 |
| 未结束尝试 | attempt_open(workspace,delivery_id,lease_id)，部分索引 | 与过期投递对接 |
| 待路由任务 | routing_ready_order(workspace,due_ms,event_id)，部分索引 | ready/retry_wait 合并按到期顺序读取 |
| 普通审计保留清理 | audit_retention(workspace,occurred_ms) | 工作区内按时间有界删除 |
| 工作组保留清理 | retired_age 与 retired_payload_due | 到期范围与尚未清正文的独立路径 |
| 内容待删除 | content_age(state,created_ms) | 先重试删除失败的对象 |
| 上传/staged 到期 | content_staged_age(created_ms)，部分索引 | 排除正在被引用的 available 内容，无多状态排序 |
| 内容反向引用 | event_content_reverse(content_id) | 防止回收仍有事件引用的文件 |
| 来源检查点 | 原主键(workspace,source,partition) | 精确查询与 CAS，不加重复索引 |
| 内容配额 | content_usage 事务计数器 | 不逐次 SUM 全部文件记录 |

领取 SQL 区分有目标与无目标路径，避免可选目标 OR 妨碍复合索引。旧 delivery_target_due 被更小的 pending/unresolved 索引替代。报告 `docs/benchmarks/p1-query-plans.json` 使用各 10000 条投递、事件和审计的临时样本，记录 Python SQLite 3.50.4 的 EXPLAIN QUERY PLAN；Rust 集成测试另验证项目 bundled SQLite 的热点索引。报告证明查询计划，不代表吞吐或尾延迟收益。

复现：`python3 scripts/query_plans.py`。每次改变查询过滤、顺序、租户分区或清理方式，都应重新检查计划；不为 JSON 正文或每个普通字段盲目加索引。

## 验证与剩余边界

76 项 Rust 测试通过，1 项编码微基准默认忽略；fmt、Clippy 全目标检查及真实进程启动/转发/停止/重启去重通过。P1 新增测试覆盖优先级公平性、取消、检查点原子性与旧实例隔离、2 MiB HTTP 内容闭环、摘要与权限、清理保护与去重窗口、备份恢复及损坏拒绝、v3 迁移失败回滚、配置版本继承、索引计划。

目前没有运行新的 RSS 长期浸泡测试，也没有宣称零拷贝或固定吞吐提升。完整动态权限和配置发布、内容加密/对象存储、多实例协调、会话多附件回复及通用 MCP 暴露仍待后续模块交付。
