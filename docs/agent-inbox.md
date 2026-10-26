# Webhook → Agent 收件箱 → 来源适配器回复

这条链路已实现并通过真实 HTTP 和官方 rmcp 客户端测试。Synquay 接收并保存消息，创建持久化分析任务；Agent 通过 MCP 领取输入、提交文本结果，Synquay 将回复投递到配置中的来源 HTTP 适配器。服务端不执行模型推理，也不会自动唤醒 ChatGPT。

“原路回复”在当前实现中指：固定回调收到原 `source_event_id` 与回复内容，由来源适配器使用自己保存的消息→会话映射调用聊天平台。尚未内置微信、QQ、邮箱等渠道适配器；测试回调不能代替真实平台发送权限。

## 配置与运行

两个配置共同启用此模式：

- `examples/pipeline-agent.json`：Webhook、来源适配器回调及 Agent 队列策略。
- `examples/tools-agent.json`：`agent_jobs=true`，通过 `/mcp` 暴露业务工具。

两者须使用同一个 SQLite 文件、同一个 workspace、匹配的 profile，并使用同一个 executor/principal。启动顺序是 Pipeline 初始化绑定，再初始化 MCP；主程序会拒绝数据库不一致或启用了 Agent 管道却未启用 MCP Agent 工具的组合。示例从项目根目录运行，两个相对路径均指向 `data/agent-demo.db`。

Agent 模式与原始事件直接转发互斥：消息进入分析任务后，只有提交成功才排队发送回复。当前仅支持 JSON 消息、一个来源及一个固定回复目标，不支持同时转发到 additional_targets 或使用内容附件。已有非 Agent 管道行为不变。

```sh
mkdir -p data
export SYNQUAY_WEBHOOK_TOKEN="$(openssl rand -hex 32)"
export SYNQUAY_MCP_TOKEN="$(openssl rand -hex 32)"
export SYNQUAY_SOURCE_CALLBACK_TOKEN="$(openssl rand -hex 32)"
export SYNQUAY_PIPELINE_CONFIG=examples/pipeline-agent.json
export SYNQUAY_TOOLS_CONFIG=examples/tools-agent.json
cargo run -p synquay-server
```

示例使用 HMAC-SHA256 + 普通 JSON，签名方式见 [Webhook 协议](webhook-mcp.md)。`X-Delivery-ID` 使用来源适配器产生的稳定消息 ID；该 ID 与正文一起签名。来源适配器应先保存其对应的渠道、会话与原消息，再调用 webhook。正文中的 URL、目标或权限字段都只是输入数据，不控制回复路线。

回调服务在示例中为 `http://127.0.0.1:9091/messages`，生产适配器应校验配置的 callback Bearer。仅观察 HTTP 接收可运行 `python3 examples/http_receiver.py`，它只在回环地址打印 delivery_id，不验证 callback Bearer，也不发送真实聊天消息。MCP 客户端使用独立的 SYNQUAY_MCP_TOKEN；来源适配器无需持有这个令牌。

此模式将来源、目标、profile 与队列策略绑定到数据库，重启必须保持一致。使用新的示例数据库运行，不要用它覆盖已有转发库；目前没有在线 Agent 绑定迁移接口。

## MCP 业务工具

这些是通过 MCP tools/list/call 提供的 Synquay 工具，不是 MCP 标准方法或 MCP Tasks 协议。

| 工具 | 参数 | 行为 |
|---|---|---|
| `synquay_inbox_list__v1` | 可选 `after` UUID | 按 job_id 返回最多 50 项元数据；下页传最后一个 job_id |
| `synquay_inbox_read__v1` | `job_id` | 返回输入及任务/回复投递状态 |
| `synquay_jobs_claim__v1` | `claim_id` UUID | 领取一项，返回 token、lease_until_ms 和输入；无任务时 claim=null |
| `synquay_jobs_renew__v1` | `token` | 延长有效租约，最长不超过任务截止时间 |
| `synquay_jobs_complete__v1` | `token`、`text` | 保存结果摘要、完成任务、创建回复 Outbox、写审计，全部同事务 |
| `synquay_jobs_fail__v1` | `token`、`retry` | retry=true 进入 5 秒退避后待领取；false 终止，不发送回复 |

领取成功后保留返回的 token：`{job_id, claim_id, generation}`。每次新领取生成 claim_id；遇到响应丢失，使用同一 claim_id 重试可找回仍然有效的同一次领取。已经过期或完成的旧领取 ID 返回冲突，读取任务状态后再决定下一步。无任务时不保存空领取记录；可以用新 ID 继续轮询。

完成只支持 1–8192 字节 UTF-8 非空文本，不能传 URL、收件人或额外目标字段。同一次领取提交相同文本，在响应丢失、租约到期或进程重启后仍返回原 delivery_id；不同文本返回冲突。每项任务只接受一个最终结果，尚不支持结果多版本、附件、草稿审批或“完成但不回复”。

测试工具调用可复用现有示例客户端。下面通过环境变量引用输入，避免将真实消息写入命令样例：

```sh
export SYNQUAY_MCP_URL=http://127.0.0.1:8080/mcp
cargo run -p synquay-server --example mcp_client
cargo run -p synquay-server --example mcp_client -- \
  synquay_inbox_list__v1 '{}' "$(uuidgen)"
```

随后让已连接 MCP 的 Agent 领取、分析并提交；需要人工协议检查时，也可使用 mcp_client 调用其余工具。工具参数中的 claim_id/token 是任务幂等与租约凭据；通用 `_meta["synquay/invocation-id"]` 不替代它们。不要把手工构造的测试回复描述为模型分析结果。

收到 webhook 不触发 Agent 运行。持续自动分析需要独立 Worker 主动轮询 MCP、调用模型、定期续租并提交；本轮没有新增模型提供商、轮询 Worker 进程或 ChatGPT 推送通道。

## 状态与可靠性

任务状态为 queued、claimed、completed、failed、expired。暂时失败通过 due_ms 退避，失效租约由每秒管道维护或下次领取处理。租约代次单调增加，旧 token 无法提交；租约同时绑定主体与 OAuth 客户端。并发领取由 SQLite Immediate 事务串行判定，多个 Worker 不会同时获得同一个有效任务。

配置范围：lease_seconds 为 5–300 秒；deadline_seconds 为 lease_seconds–86400 秒，从事件接收时间计算；max_attempts 为 1–10；max_in_flight 为 1–32，按队列限制。续租不能越过截止时间，最大尝试次数耗尽后终止。MCP 轮询调用各自仍受已有请求并发、超时和数据库有界接纳限制。

`completed_reply_queued` 只证明结果及回复意图已提交。inbox.read/list 分别返回任务 state 和 delivery_state；外部发送成功记录 submitted，表示供应方已接收。HTTP 500、发送后响应丢失等不确定结果进入 reconcile，复用现有核查流程，不自动重新发送。fail 请求若响应丢失，应读取状态，不能把 Conflict 当作“失败操作未发生”。

接收时为每个事件额外预留 64 KiB 回复预算；因此队列达到接收上限后，已经接收的任务仍可完成并入队回复。实际磁盘故障和审计失败仍可能阻止提交，此时整个事务回滚。输入通过事件引用读取，不复制到每次领取记录；JSON 指纹及回复哈希用于去重，不用请求 ID 替代业务键。

本阶段保留最多 10000 项任务历史及最多 max_attempts 倍数的成功领取记录，达到历史上限停止建新任务并通过原路由失败/背压机制显示阻塞，不自动删除去重凭据。尚无任务历史归档 API，应在生产长时间运行前补上归档策略。输入/投递正文沿用既有保留策略：未完成任务持有容量记录，不进入正文清理；终态后输入可能被清理，read 的 input 变为 null，完成回执仍可去重。

## 回调契约

Synquay 沿用 HTTP sink 的 schema_version=1 消息格式：

```json
{
  "schema_version": 1,
  "delivery_id": "<稳定投递UUID>",
  "parts": [{
    "type": "json",
    "value": {
      "type": "agent.reply",
      "event_id": "<任务/内部事件UUID>",
      "source_event_id": "<来源原消息编号>",
      "text": "Agent提交的回复"
    }
  }]
}
```

来源适配器验证回调身份，再按 source_event_id 查找原会话/线程，发送 text。应拒绝未知消息编号，并按 delivery_id 去重。正文里的渠道身份不用于选择收件人；原消息编号属于已认证来源连接的命名空间。该映射由来源适配器持久化，Synquay 当前没有通用多平台 ConversationRepository 实现。

## 权限、抽象与存储

MCP 根配置的 agent_jobs 默认 false。OAuth 使用者还需在对应 grant 中显式设置 `agent_jobs:true`；工具/资源授权不会自动给出队列权限。工作区与成员必须 active，Viewer 只能读取其被授予的队列，不能领取、续租或提交。当前粒度为单个 Profile 的整条队列，尚未提供逐会话子范围授权。

每次服务调用先执行 Governance；SQLite 事务内再次检查身份有效期、当前成员、角色和空间状态。队列读取也要求审计提交成功才返回内容。Agent 模式的外部投递增加 WorkspacePolicy，发送前再次检查空间与服务主体；Webhook 接收本身仍使用固定来源凭据，因此暂停空间后可以保留入站消息，但不能由 Agent 领取或对外投递。

| 层 | 实现 |
|---|---|
| 领域 | `domain/agent.rs`：绑定、策略、任务视图、ClaimToken、CompletionReceipt |
| 端口 | `application/ports/agent.rs`：AgentJobStore；负责并发领取与原子完成契约 |
| 服务 | `application/services/agent.rs`：队列授权、参数约束、任务操作 |
| 存储 | `infrastructure/storage/agent/queue.rs`：任务创建、绑定、到期维护；jobs.rs：领取和结果事务；mod.rs：授权、审计和读取 |
| 协议 | `interfaces/http/agent_tools.rs`：严格参数 DTO、工具目录与 MCP 映射 |
| 组合 | Pipeline 将待路由事件转换为任务；ToolRuntime 接入 MCP；原 DeliveryService 投递回复 |

SQLite 升级到 v11，增加 agent_binding、agent_jobs、agent_claims。任务 ID 与来源 event_id 相同，重复路由不会产生第二任务。迁移在现有事务中执行，失败回滚；备份/检查脚本识别 v11。索引覆盖按 Profile 的就绪任务、有效租约计数、截止时间扫描和 UUID 游标分页；领取历史使用主体、客户端和 claim_id 的复合主键，不扫描正文。

测试覆盖真实 webhook→官方 MCP 客户端→固定 HTTP 回调、重复请求和重启、并发领取/旧代次/到期/退避、审计故障回滚、成员撤销、OAuth 显式授权及客户端绑定、队列满时完成、HTTP 失败核查、迁移回滚与查询计划。完整回归与 Clippy 命令见 [开发说明](development.md)。
