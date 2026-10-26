# 连接器运行管理

运行管理负责把已发布连接配置变成受监督的实例。已有 ConnectorRegistry、可选能力组合及 ConnectorLifecycle；缺少工厂、动态替换、长期任务监督与节点归属。

## 领域对象与归属

ConnectorDefinition 描述类型、实现版本、配置 Schema、能力集合和运行方式。ConnectorInstance 绑定工作区、ConnectionRevisionRef、节点、generation 和当前健康。InstanceHandle 持有可选 decoder/source/tools/resource/prompt/sink/lifecycle 能力，不强迫来源连接实现工具执行。

RuntimeLease 保证一个来源分区的有效所有者；NodeIdentity 区分本机、Collector 和 Runner。来源采集节点与命令执行节点可共用传输设施，但使用不同任务权限和运行约束。

## 应用服务与端口

```rust
trait ConnectorFactory: Send + Sync {
    fn validate(&self, config: ConnectorConfig) -> PortFuture<'_, ValidationReport>;
    fn build(&self, request: InstanceBuild) -> PortFuture<'_, InstanceHandle>;
}
trait RuntimeRegistry: Send + Sync {
    fn acquire(&self, key: InstanceRevisionRef) -> PortFuture<'_, InstanceHandle>;
}
trait RuntimeLeaseStore: Send + Sync {
    fn claim(&self, request: RuntimeClaim) -> PortFuture<'_, RuntimeLease>;
    fn renew(&self, token: RuntimeLeaseToken) -> PortFuture<'_, RuntimeLease>;
}
```

ConnectorSupervisor 是应用服务，负责 start/pause/resume/drain/stop、崩溃退避及健康上报，不承担协议解析。每次 build 的取消清理边界明确；运行句柄由引用计数保活，更新注册表不直接销毁正在调用的旧版本。

## 状态与一致性

Stopped → Starting → Running；可进入 Backoff、Paused、Draining、Failed。desired_state 与 observed_state 分开，暂停不是仅修改 UI 标签。每种来源声明 PauseBehavior：停止拉取、取消订阅或有界缓冲，并注明上游可能丢弃的消息。

新版本先校验并准备，再切换新请求的活动引用；旧版本排空。来源订阅通过租约 generation 交接，旧实例停止确认及推进 Checkpoint。配置撤销不能因自动重连被重新激活。失联节点恢复时重新领取身份/租约，旧结果受 fencing 限制。

## 对接与存储

配置变更事件 → Supervisor → Factory → InstanceHandle；工具调用与投递按精确配置版本取句柄。来源运行使用 EventIngress 和 CheckpointStore；协议连接重试不重新创建业务事件 ID。

表组为 runtime_instances、runtime_leases、runtime_observations、nodes。每连接独立并发限制、速率预算和有界缓冲，慢连接不占满全局池。状态上报不是权威租约，数据库 CAS 才决定所有者。

## 验收

旧节点不能推进游标；重连不复活停用连接；构建失败不替换健康旧实例；排空期间旧句柄仍有效；来源和投递池互不阻塞；暂停语义可观测。首版内置 Rust 工厂，跨语言 SDK 采用版本化进程消息协议，不建设动态 Rust ABI 插件系统。
