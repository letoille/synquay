# 投递与恢复

投递模块拥有逻辑投递、物理尝试、外部回执和结果核查。当前 Outbox、租约、Sending 前置落盘与 HTTP sink 已实现；P0 已补独立目标通道、尝试记录及人工证据核查/放弃/重放，见[P0 实现](../p0-operations.md)。自动渠道核查、会话分区和配置版本化仍待完善。

## 领域对象与归属

Delivery 固定内容引用、destination_revision、服务端 ExecutionAuthority、run_id、幂等键和原事件。DeliveryAttempt 记录 attempt_id、lease generation、开始/结束、错误分类、实际秘密版本引用和外部回执。RecipientReceipt 表示平台提供的 accepted/delivered/read；ReconciliationCase 保存核查状态和证据。

重试仍是同一 Delivery 的新 attempt；改变内容或目标必须创建新 Delivery 和显式父关联。通知目标的地址从固定版本解析，当前配置可撤销它但不能静默替换地址。

## 应用服务与端口

```rust
trait DeliveryInspector: Send + Sync {
    fn inspect(&self, request: DeliveryInspection) -> PortFuture<'_, InspectionResult>;
}
trait DeliveryRecovery: Send + Sync {
    fn resolve(&self, command: ReconcileCommand) -> PortFuture<'_, RecoveryReceipt>;
    fn replay(&self, command: ReplayCommand) -> PortFuture<'_, DeliveryId>;
}
trait DeliveryReceiptIngress: Send + Sync {
    fn accept(&self, receipt: VerifiedProviderReceipt) -> PortFuture<'_, ReceiptUpdate>;
}
```

扩展既有 DeliveryOutbox，增加 renew/release/attempt 查询及明确的取消状态。MessageSink 仍负责发送；能证明结果的渠道才实现 DeliveryInspector。恢复服务先做权限、版本和证据校验，再调用 CAS 仓储，不能直接编辑 SQL 状态。

## 状态与事务

Pending → Leased → Sending → Accepted；可进入 RetryWait、Blocked、Failed、Cancelled、OutcomeUnknown。Delivered/Read 为独立平台回执维度。当前 submitted 映射为 Accepted，reconcile 映射为 OutcomeUnknown 加核查记录；旧状态迁移保留实际语义。

发送前同事务持久化 attempt 与 Sending，结束后原子保存 attempt 结果、Delivery 状态、MessageLink 和相关审计意图。HTTP/平台发送在事务外。失去租约不能提交 authoritative 结果；可追加只读晚到证据交给核查，不能覆盖当前所有者。

已知未发送的前置暂时错误可以安全退避，与“可能已发送”分开分类。自动重试要求适配器明确说明可安全重放范围和幂等窗口；不因 HTTP 方法或 Idempotency-Key 头存在就假定幂等。次数、总时长、带抖动退避与 Retry-After 都受预算约束。

## 调度与核查

独立投递池按工作区/连接公平调度，设置全局与目标并发、限速和熔断。单目标慢请求不阻塞接收和路由。人工可确认已成功、确认未发送后安排重试、放弃或显式重放；证据不足时保留未知。每个动作记录操作者与原因，默认不批准可能重复的发送。

收到平台回调先验证签名和目标，再按外部消息 ID/attempt 关联，去重处理乱序回执，不把 read 回退到 accepted。中断后没有外部 ID 的任务只能用适配器可验证的幂等查询或人工核查。

## 验收

慢目标不拖延其他连接；进程崩溃后 Sending 不盲重发；前置取消/暂时失败不被误报为已经发送；过期 worker 不能覆盖状态；相同回执重复/乱序不回退；重放不修改原记录；权限撤销阻止新发送但不伪造外部撤回。
