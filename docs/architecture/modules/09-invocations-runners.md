# 工具调用与 Runner

本模块记录每次工具调用并协调执行，和 Agent 分析任务分开。已实现 HTTP ToolExecutor、持久化 InvocationLedger、同键指纹校验、结果复用，以及结果和结束审计的原子保存；未完成调用跨重启保持未知，不自动再执行。长任务、核查重放管理和受控 CLI Runner 待实现。详见[MCP 工具闭环](../mcp-tools.md)。

## 领域对象与归属

Invocation 固定 workspace、actor/client、Profile/能力/连接版本、参数摘要、幂等键和截止时间；InvocationAttempt 保存执行节点、generation、输出引用和实际结果。ExecutionPlan 保存配置允许的目标/程序、参数映射、限额与授权引用。RunnerTask 携带最小执行授权，不携带任意用户自定义可执行文件路径。

工具效果提示与服务端执行策略分开。幂等键以工作区、主体、客户端、能力版本和调用用途限定作用域，同键不同参数返回 Conflict。参数摘要需规范化和适当的 keyed digest，不能在日志中泄露低熵秘密。

## 应用服务与端口

```rust
trait InvocationStore: Send + Sync {
    fn reserve(&self, request: InvocationReservation) -> PortFuture<'_, ReservationResult>;
    fn start(&self, command: StartInvocation) -> PortFuture<'_, InvocationLease>;
    fn finish(&self, result: InvocationCompletion) -> PortFuture<'_, InvocationReceipt>;
}
trait RunnerClient: Send + Sync {
    fn submit(&self, task: AuthorizedRunnerTask) -> PortFuture<'_, RunnerReceipt>;
    fn cancel(&self, request: RunnerCancellation) -> PortFuture<'_, CancellationReceipt>;
}
```

工具服务完成发布/权限/Schema 检查后预留调用，再开始执行；预留返回既有结果、处理中或新调用，不把协议 request_id 自动当业务幂等键。ToolExecutor 适用于直接 HTTP/MCP 调用，RunnerClient 适用于受管进程，结果统一映射为执行记录和受保护内容引用。

## 状态与事务

Reserved → Queued/Running → Succeeded、Failed、OutcomeUnknown；CancelRequested 与最终 Cancelled 分开。需要审批则预留后进入 AwaitingApproval，审批消费与启动 CAS 原子提交。发生外部调用后即使输出 Schema 错误、完成审计失败，也保留“已经执行”的事实。

启动前将 attempt、Started 审计和授权引用一起落库。完成保存结果引用和状态；引用先上传为 staged，事务提交后可读，孤儿内容由保留策略清理。执行过程不占用数据库事务。超时未知先核查，不能自动启动第二个 CLI 进程或写操作。

## Runner 与对接

InvocationService → 授权/秘密取用 → 本地 Executor 或 Runner → 结果/产物 → InvocationStore。CLI 模板固定可执行文件、argv、目录、环境变量名、网络和输出限额；处理选项注入、符号链接、进程组清理与 stdout/stderr 背压。工作目录限制不等于沙箱。

远端 Runner 主动连接或领取任务，节点身份独立轮换。任务授权绑定 operation_id、模板摘要、版本、节点和过期时间，旧节点结果经 fencing 校验。传输可能重复，Runner 需持久化执行去重账本；传输失败不直接重跑程序。

## 验收

同键同参返回同一逻辑调用；同键异参拒绝；失联重连不重复执行写操作；取消能回收进程树且保留实际结果；超大输出受限；输出/文件读取仍检查原调用权限；撤权阻止后续步骤。初版不建设任意脚本执行平台。

HTTP Executor 已通过精确版本能力目录读取协议绑定。动作/状态/事件及 CLI/MQTT 绑定契约见 [统一能力层](../capability-layer.md)；CLI/MQTT 真实执行和工具到 Outbox 的原子提交仍待实现。
