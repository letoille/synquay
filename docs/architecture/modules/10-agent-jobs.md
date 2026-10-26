# Agent 收件箱与任务

本模块管理交给 Agent 的工作，不执行模型推理。现已实现首个 JSON/HTTP 来源的持久化收件箱：MCP 领取、续租、结果原子提交、固定来源回调和有界失败恢复。实际范围见[运行与接口说明](../../agent-inbox.md)。下文仍包含多平台、审批、通知及结果多版本等后续设计，不能视为全部已实现。

## 领域对象与归属

AgentBinding 保存 Agent 身份、可读 Profile、触发方式、并发/时限和允许的结果目标。InboxEntry 是授权范围内的任务视图；AnalysisJob 固定输入引用、有限会话上下文、AgentBindingRevision、ReplyRouteRef、授权引用及 deadline。ClaimLease 绑定 worker/Agent 主体与 generation；AnalysisResult 保存结果版本、内容引用和提交键。

触发方式分客户端主动领取、外部 Worker 和经过验证的平台事件通道。推送/通知只提示有任务，持久化队列是事实来源。触发失败可单独重试，不重复创建 AnalysisJob，不把通知已受理等同于 Agent 已分析。

## 应用服务与端口

```rust
trait AgentInbox: Send + Sync {
    fn list(&self, query: AuthorizedInboxQuery) -> PortFuture<'_, InboxPage>;
    fn read(&self, request: AuthorizedInboxRead) -> PortFuture<'_, AnalysisInput>;
}
trait AnalysisJobStore: Send + Sync {
    fn claim(&self, request: AnalysisClaim) -> PortFuture<'_, Option<ClaimedAnalysis>>;
    fn renew(&self, request: AnalysisRenewal) -> PortFuture<'_, ClaimLease>;
    fn fail(&self, request: AnalysisFailure) -> PortFuture<'_, AnalysisState>;
}
trait AnalysisCommitter: Send + Sync {
    fn complete(&self, plan: AnalysisCompletionPlan) -> PortFuture<'_, AnalysisReceipt>;
}
trait AgentNotifier: Send + Sync {
    fn notify(&self, notice: AgentWorkNotice) -> PortFuture<'_, NotificationReceipt>;
}
```

AgentJobService 负责逐次授权、结果 Schema 校验、租约所有者验证和调用 ReplyPlanner；AnalysisCommitter 负责事务。对外工具名称可为 inbox.list/read、jobs.claim/renew/complete/fail/cancel，属于项目 API，不能标为 MCP 标准方法。

## 状态与原子完成

Queued → Claimed → Running → Completed；可进入 RetryWait、Failed、Cancelled、Expired、Blocked。领取和续租受任务 deadline、最大处理次数及 Agent 配额约束。租约失效允许重新分析，旧 Agent 不再有权提交结果。

完成幂等键为 `(workspace,job_id,result_version)`，另保存规范化结果摘要。相同键和相同内容返回原 Receipt；不同内容 Conflict。新提交必须持有效租约与当前完成权限，并满足允许的下一 result_version。已经提交的结果重查仍验证读取权限，但不要求旧租约仍有效。

AnalysisCommitter 同事务校验 CAS、保存结果、完成任务、生成经过授权的回复投递或待审意图及审计。Completed 表示结果与后续意图已持久化，不能表示回复已送达。结果允许无回复；若策略要求审核，则创建明确的 PendingApproval 意图，不提前进入可发送 Outbox。

## 权限与对接

RoutePlanCommitter 创建任务；Agent 通过 MCP/SDK 领取；读取任务、会话和附件分别限制范围。任务委托不能赋予 Agent 原发布者/服务账号的所有权限，模型生成的目标 URL、工具名称和权限字段不能成为授权来源。

Agent 提交 → 校验 Job/Claim/Result → ReplyPlanner → AnalysisCommitter → Outbox。权限/路线过期后未提交的完成请求拒绝并记录安全状态，不能为“完成任务”绕过回复权限；已经保存的合法结果可按读取权限查询，待发回复仍受最新撤销检查。

## 验收

两个 Agent 不能同时持有有效领取权；旧 generation 被拒绝；通知重复不重复建任务；结果回包丢失后重交不重复回复；同键异内容拒绝；任务完成而渠道失败分别显示；客户端不支持事件时显示等待领取，不能宣称自动唤醒。
