# 路由与处理计划

路由负责将事件变成确定的处理计划，不执行消息发送或模型推理。已有 RoutingRules、MessageRenderer、RoutingService 和事件级原子排队；需要增加 RoutingTask、规则快照及失败隔离。

## 领域对象与归属

RouteRuleRevision 定义来源范围、受限条件、字段投影、转换模板及目标分支。RoutingTask 保存 event_id、route_set_revision、run_id、状态、attempt、next_attempt_at 和租约。DispatchRun 保存计划及来源，分 Automatic 和 ManualReplay。PlanTarget 为 Deliver 或 Analyze，均引用已授权、已发布的目标。

规则表达式只访问允许字段，不执行任意脚本、读取秘密或访问网络。Transform 输出类型化 Message/AnalysisInput，不能给正文附加内部高权限上下文。顺序与优先级属于快照；多个规则的执行顺序确定。

## 应用服务与端口

```rust
trait RoutingTaskStore: Send + Sync {
    fn claim(&self, query: RoutingClaim) -> PortFuture<'_, Option<LeasedRoutingTask>>;
    fn fail(&self, failure: RoutingFailure) -> PortFuture<'_, RoutingState>;
}
trait RoutePlanner: Send + Sync {
    fn plan(&self, input: PlanningInput) -> PortFuture<'_, ResolvedDispatchPlan>;
}
trait RoutePlanCommitter: Send + Sync {
    fn commit(&self, token: RoutingLeaseToken, plan: ResolvedDispatchPlan) -> PortFuture<'_, DispatchReceipt>;
}
```

保留 RoutingRules/MessageRenderer 作为规划依赖。RoutingService 只协调读取事件、固定规则、授权、规划和提交；人工重放入口显式选原版本或新版本，不能隐式套用最新配置。

## 状态与事务

Ready → Leased → Completed；暂时失败转 RetryWait，永久配置错误转 Blocked，达到预算转 DeadLetter，可显式 Cancelled。任务保存安全错误码和失败时间；到期查询以 next_attempt_at 排序，阻塞任务不重复占满前 100 条扫描窗口。

RoutePlanCommitter 同事务 CAS 完成任务、保存 DispatchRun、逐个创建 Delivery/AnalysisJob 和审计事件。空计划仍完成并保存原因。计划创建阶段任何分支无效则整批不提交；提交后的目标分别执行，某次投递失败不会重发其他成功目标。

唯一约束为自动任务 `(workspace,event_id,automatic)`；计划目标为 `(workspace,run_id,rule_revision,target_key,branch_index)`。修改规则不再处理已经完成的自动任务；人工重放生成新 run_id。不同规则是否允许向同一目标生成多条消息由显式规则决定，不偷偷做内容合并。

## 循环与对接

Event → RoutingTask → Planner → DispatchPlan → Outbox/AgentInbox。route_trace 逐步扩为连接与规范目的地/会话维度的 RouteHop，并结合最大跳数、源消息映射与回流标记；无法识别回流的渠道不承诺完全防循环。AI 回复是显式新交互，但仍携带原 correlation 和 reply lineage。

表组为 route_revisions、route_set_revisions、routing_tasks、dispatch_runs、dispatch_targets。运行时权限与快照规则分别检查；停用目的地使待办阻塞或取消，不能替换为其他收件人。

## 验收

100 条永久失败任务不会饿死后续正常事件；重复领取/提交只产生一组目标；空规则不会无限扫描；改规则不重发旧事件；多目标中一条失败不影响已成功目标；人工重放记录操作者、原因、版本和父 run_id。
