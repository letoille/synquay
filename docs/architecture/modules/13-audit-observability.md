# 审计与可观测性

审计回答谁以什么授权做了什么；运行日志、指标和 trace 帮助定位故障。已有 AuditLog、SQLite 记录、request_id 和 tracing；P0 新增独立管理授权、运行指标/耗时桶和原子恢复记录，见[P0 实现](../p0-operations.md)。通用查询权限、完整审计阶段和跨进程关联待完善。

## 领域对象与归属

AuditRecord 扩展 original_actor、executing_principal、client、action、resource、配置/授权版本、operation_id、attempt_id、phase、safe_error_code。业务正文、token 与完整命令参数不写入常规审计。TraceContext 保留 correlation/causation 与可选分布式 trace 引用，不把 request_id 误作标准 trace ID。

审计事件追加写；完成审计缺失与业务执行失败是独立状态。运营指标聚合积压、最老任务年龄、重试/核查数量、来源延迟、租约冲突和资源拒绝，不使用用户 ID 或消息正文作为高基数标签。

## 应用与基础设施端口

```rust
trait AuditQuery: Send + Sync {
    fn search(&self, query: AuthorizedAuditQuery) -> PortFuture<'_, AuditPage>;
}
trait AuditExporter: Send + Sync {
    fn export(&self, batch: AuthorizedAuditBatch) -> PortFuture<'_, ExportReceipt>;
}
trait HealthProbe: Send + Sync {
    fn check(&self, scope: HealthScope) -> PortFuture<'_, HealthReport>;
}
```

沿用 AuditLog。事务内的状态操作把审计意图一起写入，外部审计系统通过独立 Outbox 导出；远端日志平台不可用不应要求重做已经成功的外部发送。导出重试以 audit record_id 去重。

## 阶段与失败

阶段包括 Requested、Authorized/Denied、AwaitingApproval、Started、Succeeded/Failed、OutcomeUnknown、CancelRequested/Cancelled、Reconciled、Replayed。实际领域状态仍由相应模块拥有，审计阶段是事实记录，不能通过“改审计”改变任务。

关键外部副作用前审计持久化失败则不开始。副作用后的持久化失败报告降级状态并进入恢复查询，不伪装为未执行。存储不可用时至少输出固定安全诊断码；不能通过内存日志声称已经形成持久审计。

## 运维对接与验收

健康分 liveness、readiness 和每连接健康。readiness 检查数据库、迁移版本、必要配置与 worker 心跳；单个外部目标失败不必让全部健康接口失败。tracing span 使用 event/job/delivery/invocation ID 显式关联，后台任务不能丢失 causation。

AuditQuery 按租户与角色过滤，返回游标分页；正文诊断是单独授权的功能。数据库管理员能改本地 SQLite，只有启用可验证外部归档后才可声明相应防篡改能力。保留清理单独授权并留痕。

验收：跨租户审计不可见；日志无凭据与正文；结果未知和业务失败可区分；审计导出重复不造新记录；worker 停止可在 readiness/指标观察；执行成功而结束审计失败不触发再次执行。
