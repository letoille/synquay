# 事件与来源订阅

P1 已实现 SourceCheckpoints、generation 接管隔离和与事件同事务的游标推进，并接入 IngressService；真实 MQTT/CLI 驱动及其网络确认语义仍待实现。详见[P1 实现](../p1-storage.md)。

本模块拥有事实记录、接收去重、来源消费位置和内容版本。已有 IncomingEvent/EventEnvelope、EventIngress/EventStore、SQLite 存储；P1 已实现原子游标提交；统一来源订阅及真实驱动确认仍待扩展。

## 领域对象与归属

EventEnvelope 保持不可变；SourceSubscription 描述订阅来源和范围；Checkpoint 保存 source_partition、opaque_cursor、generation 和 schema_version。ContentItem/ContentRevision 保存文章等内容的规范来源、发布时间、采集时间和版本。聊天 Message 由会话模块定义，非聊天事件无需伪造发送人或回复路线。

SourceMode 分 Push、Stream、Poll、Observe。历史导入、retained 快照与实时新增带不同的服务端来源标签，路由可显式选择。原生 offset 或源事件 ID 优先去重；没有稳定 ID 的来源必须声明策略及限制，不能默认按正文永久去重。

## 应用服务与端口

```rust
trait SourceSubscriptionRepository: Send + Sync {
    fn active(&self, query: SourceQuery) -> PortFuture<'_, SourcePage>;
}
trait CheckpointStore: Send + Sync {
    fn load(&self, key: SourcePartitionKey) -> PortFuture<'_, Checkpoint>;
    fn advance(&self, commit: CheckpointAdvance) -> PortFuture<'_, Checkpoint>;
}
trait IngressCommitter: Send + Sync {
    fn accept(&self, plan: AcceptancePlan) -> PortFuture<'_, AcceptanceReceipt>;
}
```

EventSource.run 与 InboundDecoder 继续是协议能力端口。IngressService 验证来源、大小、Schema、去重策略和范围，构建 AcceptancePlan；事务端口统一写入事件、必要消息/返回上下文、路由任务和同库可提交的 Checkpoint。调用方不能直接构造已授权身份。

## 确认与事务

成功回执只在提交后返回。源 ID 在工作区/连接/分区命名空间唯一；重复记录返回原 event_id 和原接收结果，不生成新路由任务。必要的消息映射唯一约束与事件插入同事务。

能够将 Checkpoint 一起提交时，验证 runtime generation 并 CAS 推进；否则必须先提交事件，再提交 Checkpoint，最后确认上游。中间崩溃允许重读，由去重收敛。若某协议客户端提前确认或没有重放能力，明示损失窗口；不能用本地接口宣称端到端无损。消费游标是协议不透明值，核心只检查版本和所有权，是否单调由来源适配器验证。

## 对接与存储

签名/认证 → Decoder → IngressService → IngressCommitter → Receipt → 上游确认。每个新事件固定 route_set_revision；路由 worker 不读取后来变更的规则决定旧事件命运。表组为 events、source_subscriptions、source_checkpoints、content_items、content_revisions、routing_tasks。

初次订阅按配置仅初始化游标或受限导入；导入任务与实时消费有各自预算，避免旧文章或 retained 状态触发大量新通知。内容访问失效时保留来源状态并暂停需要重新授权的来源。

## 迁移与验收

保留当前事件唯一键，在新增 partition 字段时将旧事件映射到默认分区。对已有未路由事件补建唯一 RoutingTask，已有 dispatch 标记不再创建自动任务。验收：提交后崩溃不丢已接收事件；提交前不确认；旧 generation 不能推进游标；重连/历史导入不产生重复任务；Schema 不支持时明确拒绝；来源缓冲满时背压而非无限占用内存。

已有 MQTT 状态/事件订阅的描述与绑定契约，StateObservation 区分来源时间、接收时间及实时/retained/历史来源。真实驱动和 StateReader 实现尚未接入，见 [统一能力层](../capability-layer.md)。
