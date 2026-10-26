# 存储与运行运维

P1 已加入公平优先调度、查询索引、保留清理、PASSIVE checkpoint 及离线备份/恢复。schema v6 在工具发布和调用账本基础上增加草稿、草稿审计与发布操作去重，CAS 与管理员审计原子提交；操作核查由复合主键覆盖；v7 新增发布审计的 workspace/profile/revision 唯一索引，支持管理台历史分页关联，备份脚本兼容 v7。详见[OAuth 接入与在线配置发布](../oauth-publication.md)。

本模块实现各业务拥有的仓储和跨模块事务端口，提供迁移、备份、恢复、容量与清理。当前采用 rusqlite、SQLite WAL/FULL、版本 5 和 blocking worker；继续沿用，数据库驱动不是领域边界。

已实现 v1→v2 事务迁移、未完成工作容量回填、普通 64/关键 8 个接纳名额、逻辑积压与磁盘水位保护，详见[积压容量与异常恢复](../backpressure.md)。下述通用迁移计划、备份与保留服务仍为设计接口。

P0 已补 v2→v3 迁移、目标索引、投递尝试与原子恢复记录，见[P0 实现](../p0-operations.md)。

## 归属与端口

业务模块拥有实体含义、状态转换和唯一键；存储层拥有 SQL、索引、编码与原子提交。禁止将 Connection/Transaction 传给 handler 或让服务任意执行其他模块 SQL。跨模块事务使用 AcceptancePlan、PublicationPlan、DispatchPlan、AnalysisCompletionPlan 等有类型端口。

```rust
trait MigrationRunner: Send + Sync {
    fn plan(&self, target: SchemaVersion) -> PortFuture<'_, MigrationPlan>;
    fn apply(&self, plan: ValidatedMigrationPlan) -> PortFuture<'_, MigrationReceipt>;
}
trait BackupService: Send + Sync {
    fn create(&self, request: BackupRequest) -> PortFuture<'_, BackupReceipt>;
    fn verify(&self, request: BackupVerification) -> PortFuture<'_, VerificationReport>;
}
trait RetentionService: Send + Sync {
    fn plan(&self, policy: RetentionPolicy) -> PortFuture<'_, RetentionPlan>;
    fn execute(&self, plan: AuthorizedRetentionPlan) -> PortFuture<'_, RetentionReceipt>;
}
```

Clock、ID 生成和抖动源可注入到应用测试，事务端租约有效性由数据库/存储权威时钟判定；Instant 只用于进程内截止时间，不能持久化。

## 迁移与数据编码

数据库版本、持久化记录编码版本、配置 Schema 和协议版本分别管理。新增 SchemaMigration 记录版本、校验摘要、开始/结束及结果；未知未来版本拒绝启动。演进使用 additive schema 和显式回填，不直接把当前 Rust enum 的重命名当作兼容格式。

逐步用存储 DTO 替代领域类型的隐式 serde 编码，先支持读取现有 v1，再写入明确的新 encoding_version；验收旧库恢复和状态映射后才移除旧读取路径。原有事件去重与 dispatch 标记必须保留，避免升级触发重复发送。

## 并发与事务

给阻塞数据库操作增加有界 admission queue，容量满时返回可识别背压；不无限 spawn_blocking 等待同一 Mutex。事务短小、竞争使用 CAS/唯一约束；外部网络、模型推理、文件下载与 Runner 执行在事务外。

同库跨模块端口的原子性由具体 SQLite 实现保证；未来拆服务须用持久消息与幂等协调替代，不能直接把远程接口包装成“原子事务”。PostgreSQL 后续实现相同契约，使用共享合同测试而非仅验证 SQL 可执行。

## 备份保留与容量

一致性备份使用 SQLite 支持的备份方式，不能只复制运行中的主文件。恢复先在独立位置校验 schema、引用、去重标记和活跃租约；Sending 恢复为待核查，不能自动重发。加密主密钥与数据备份分开管理。

正文、附件、审计和幂等墓碑分别有保留期；幂等记录至少覆盖声明的上游重放窗口，清理前检查活跃任务引用。磁盘水位、最大正文、附件配额和积压容量触发拒收/暂停，不能返回已持久化成功后再无痕丢弃。

## 验收

迁移中断可恢复；新程序可读旧库；未知版本失败关闭；恢复演练不会重复投递；正文到期不误删活跃产物；保留清理后仍满足声明的去重窗口；数据库请求过载内存有界；状态完成与附属消息/审计写入一起提交或一起回滚。

v8 新增 workspaces、workspace_members、workspace_changes，复合主键覆盖空间/成员读取和游标分页，部分索引用于有效所有者检查。成员变更、revision 和审计同事务提交；备份工具已支持 v8，见 [空间模型](../workspaces.md)。
