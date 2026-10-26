# P0 调度、恢复与容量基线

本页为 P0 阶段记录；大内容、数据库公平优先级和维护等后续成果见[P1 实现](p1-storage.md)。

2026-10-24。已实现单进程、单工作区、静态多目标 HTTP 链路的 P0：受保护的失败核查/恢复接口、目标隔离调度、指标和有限负载场景。跨工作区公平调度、分布式路由租约、OAuth 管理台、大附件流式传输与历史清理仍待后续实现。

## 模块与对接

领域层新增 RecoveryCommand、RecoveryTarget、RecoveryAction、RecoveryReceipt；应用端口新增 RecoveryStore 和目标过滤的 DeliveryOutbox.claim_target。SQLite 实现恢复事务、投递尝试账本和版本迁移；Pipeline 负责固定操作者授权、目标调度及生命周期；HTTP 管理接口与 ops.py 只调用这些入口，不允许客户端执行任意 SQL 或改写消息目标。

每个目标必须有唯一 connection_id、destination_id、rule_id。原有单目标配置和绑定保持兼容；additional_targets 最多 31 个，合计最多 32 个。所有事件按固定规则转发到这些目标。增加或改变目标会改变数据库绑定，不能直接用修改后的配置打开旧库；需独立测试库或后续显式配置迁移。

## 核查与恢复闭环

| 对象/状态 | 操作 | 行为 |
|---|---|---|
| failed 投递 | replay | 保留原投递，生成新 delivery_id 和 parent_delivery_id；继续占用原逻辑工作额度 |
| reconcile 投递 | replay | 必须提供 evidence 和 confirmed_not_submitted=true；表示操作者完成渠道核查，服务端不会替操作者验证渠道证据 |
| reconcile 投递 | confirm_submitted | 必须提供渠道确认依据；记录 confirmed_submitted resolution，不伪造历史网络回执 |
| failed/reconcile 投递 | abandon | 明确停止追求这次投递；原状态和正文保留，不将其认定为发送成功 |
| blocked 路由 | replay | 重开同一事件的规划，递增恢复版本，重置退避计数；不重跑已完成投递 |
| blocked 路由 | abandon | 记录放弃并释放事件额度；后续不能通过普通 enqueue 为该事件创建新投递 |

所有命令包含 command_id、expected_state、expected_revision、reason。命令 ID 是幂等键：同操作者、同参数重复提交返回原回执；同键异参返回 409。状态和恢复版本同时校验，防止一次恢复后再次失败时旧操作误命中新状态。提交结果不明时复用原命令，不生成新 ID 猜测重试。

恢复状态、容量变更、新投递和 recovery_actions 操作记录在同一事务。全部目标 submitted 或得到明确终结 resolution 后释放额度；superseded 对应的新投递仍未完成时继续保留额度。人工操作不删除历史内容，也不让失败自动变成成功。证据与理由仅保存于受保护的恢复记录，不放入常规指标或日志。

独立 delivery_attempts 记录领取、发送开始、结束和结果。租约过期记录 lease_expired；原 Sending 仍进入 reconcile。v3 之前的尝试没有足够时间事实，不回填虚构历史。普通结束审计仍可能独立失败，投递尝试结果与状态已在同一事务保存，完整审计统一提交留待后续。

## 管理接口与使用

默认关闭管理权限。配置 admin_token_env 和 operator_principal_id，设置一个至少 32 个可见 ASCII 字符的独立管理令牌；操作者 ID 与 webhook 服务主体必须不同，令牌不能与入口令牌相同。该操作者仅管理当前配置工作区。管理请求有独立 8 个接纳名额，POST 正文最多 8 KiB、读取超时 5 秒。部署时通过 HTTPS 访问；当前不是 OAuth/RBAC 管理系统。

| 接口 | 内容 |
|---|---|
| GET /api/ops | 容量、最老积压、状态分布、目标熔断、进程计数和耗时分桶；每类最多 100 个待核查条目 |
| GET /api/ops/recovery/delivery/{id} | 单投递状态、父投递、最近最多 100 次尝试和恢复记录 |
| GET /api/ops/recovery/route/{id} | 当前来源的事件路由状态和最近最多 100 条恢复记录 |
| POST /api/ops/recover | 提交已经核查的类型化命令；工作区和操作者由服务端绑定 |

本地配置示例：examples/pipeline.multi-target.example.json。先运行读取，再从快照复制 ID、state、revision 到 recovery.json；reason/evidence 不应包含凭据。

```sh
python3 scripts/ops.py --url http://127.0.0.1:8080
python3 scripts/ops.py --url http://127.0.0.1:8080 --case delivery <投递UUID>
python3 scripts/ops.py --url http://127.0.0.1:8080 --command recovery.json
```

ops.py 从 SYNQUAY_ADMIN_TOKEN 环境变量读取令牌；不从命令行参数接收令牌，不跟随重定向，也不使用代理环境变量。恢复命令结构示例：

```json
{
  "command_id": "<本次操作的新UUID，重试时保持不变>",
  "target": {"kind": "delivery", "delivery_id": "<从快照复制>"},
  "expected_state": "reconcile",
  "expected_revision": 0,
  "action": "replay",
  "reason": "渠道故障已排除，重新提交",
  "evidence": "渠道查询记录编号：确认原投递未提交",
  "confirmed_not_submitted": true
}
```

## 目标调度、隔离与停止

每个目标一个固定串行执行通道，全部目标共享 max_in_flight 名额（默认 8，上限 32）。单工作区部署中它同时约束服务和工作区的发送总并发。取得全局名额后才领取任务/加载正文；等待限速和熔断时不占该名额。排队采用有界目标通道和 semaphore 等待，不为每条消息创建无限任务。

每目标 min_interval_ms 默认 100，约束相邻处理启动间隔；失败连续达到 failure_threshold=3 后打开熔断，默认 cooldown_ms=30000。冷却结束每目标仅一个尝试；成功关闭，失败再次冷却。熔断只暂缓后续领取，不把结果未知的投递改成可重试。当前熔断和速率状态保存在进程内，重启会重置；数据库中的 failed/reconcile 不会因此重发。所有目标目前共用同一组调度参数，各自独立计数；按平台配额配置不同参数待后续扩展。

strict_order 默认 false，兼容原有失败隔离行为；一个目标同时最多一次发送，但重试/人工重放不保证原始顺序。设 true 时，未完成的较早投递会阻塞整个目标，包括未核查错误；人工重放是追加的新运行。当前没有会话分区字段，因此不能宣称不同会话间并行而同会话严格有序。

路由与各目标通道独立运行，某个慢目标不阻塞其他通道；全局名额全部被占满时仍会等待。接收/恢复提交后发出唤醒提示，1 秒补扫处理丢失提示和到期任务。停止后不再领取新任务，已开始的外部发送等待有界结果；通道异常退出会停止并排空其余通道。数据库仍是任务事实来源。

外部调用前的暂时性错误使用 Deferred，允许非幂等目标在尚未发送时退避，最多尝试 5 次；权限拒绝等仍终止。外部发送后的结果未知保持 reconcile，不自动重复调用。

## 指标、测试与容量基线

指标使用固定大小的原子计数和耗时桶，不按消息 ID 建无限标签。包含接收成功/重复/拒收、数据库接纳拒绝、投递结果、worker 错误，接收入库、数据库排队/执行和投递处理耗时。耗时桶为微秒单位、非累计桶；进程重启计数重置。submitted 运行计数表示目标返回接受结果；持久状态以 database 快照为准，记录失败另外进入错误日志。

数据库快照包含逻辑积压、最老积压年龄、待核查状态、DB/WAL 文件大小和文件系统可用空间。没有 Prometheus 导出端点，采集器可用受保护 JSON 接口。快照的队列统计来自同一读事务；文件大小和进程计数是近似同时采样。大历史库的聚合成本尚未做长期优化。

64 项 Rust 测试通过，另有 1 项默认忽略的编码微基准；Clippy 全目标检查通过。新增覆盖未知结果重放证据、幂等回执、跨工作区拒绝、恢复版本冲突、恢复审计失败整笔回滚、v2→v3 迁移和失败回滚、目标隔离/总并发/限速/熔断、管理令牌隔离及元数据无正文。真实进程冒烟覆盖转发、正常停止、重启和去重。

可重复的有限负载脚本使用临时库、本机随机端口、固定数量客户端线程。记录所有请求状态和拒收原因，成功目标的端到端延迟与未完成工作分开统计；吞吐报告同时记录接收阶段和包含排空阶段的耗时。采样不能证明整个运行期间从未出现瞬时峰值。

```sh
cargo build --locked --release -p synquay-server
python3 scripts/load_pipeline.py --scenario healthy --capacity 1000 --output docs/benchmarks/p0-healthy.json
python3 scripts/load_pipeline.py --scenario slow --payload-bytes 60000 --output docs/benchmarks/p0-slow-large.json
python3 scripts/load_pipeline.py --scenario failure --drain-seconds 3 --output docs/benchmarks/p0-failure.json
```

本次 macOS arm64、release 构建，每场景 200 条输入、8 个客户端线程：正常场景 200 条全部接收，两个目标各收到 200 条；大消息/50 ms 慢目标场景接收 50 条、容量拒收 150 条，两个目标各收到 50 条并排空；故障场景接收 50 条、拒收 150 条，健康目标收到 50 条，故障目标调用 3 次后熔断，50 条逻辑工作仍待完成或核查。具体延迟、文件大小和二进制摘要保存在 docs/benchmarks 下的 JSON。

当前执行沙箱禁止 ps，RSS 结果为 null 并记录 PermissionError；不能据此给出内存峰值或长期稳定性结论。长期 soak、真实磁盘满/掉电故障、分配次数与完整生产 SLO 仍需在可观测的部署环境验证。

## 数据库与交付边界

数据库升级至 v3；v1→v2→v3 在同一升级事务内执行。新增目标索引、恢复版本/决议/父投递、尝试和操作记录，不更改原消息编码。已有单目标绑定保持不变。仅使用临时测试库，没有升级用户部署库；正式升级前一致性备份，降级需要恢复备份并核查外部发送。

P0 当前适用于一个进程拥有一个静态工作区数据库，不支持两个调度进程共享同库并维持目标级并发/限速保证。后续优先补正文共享/大附件流式传输、历史清理、数据库执行优先级和长期负载验证，再根据实际需求引入多工作区与多节点调度。
