# 配置控制面

P1 已持久化流水线配置快照及 active_release。工具控制面现已支持文件发布、在线暴露草稿、预览、双重 CAS、管理员发布审计和重启恢复；发布等待在途调用结束后切换。连接和路由配置仍来自可信文件。详见[OAuth 接入与在线配置发布](../oauth-publication.md)。

控制面拥有配置的编辑、验证、发布、撤销和激活。运行时消费不可变快照，不读取正在编辑的草稿。当前有 PipelineConfig、不可变身份/目标绑定与 P1 启动快照版本；以下设计扩展为多连接与多路由管理。

## 领域对象与归属

ConnectionDraft/Revision、DestinationRevision、RouteSetRevision、ConfigurationRelease、ActivationRecord 归本模块；能力与 Profile 内容由能力模块定义，发布协调器将其引用纳入 Release。每条记录包含 workspace_id、稳定对象 ID、revision、ETag、配置 schema 版本和创建主体。快照只含 CredentialRef，不含秘密。

一个 Release 保存精确的连接、目的地、规则、Profile 版本集合及依赖摘要。独立修改可以只发布相关对象，但其引用必须指向已有有效版本。活动指针与发布历史分开，避免“最新版”在任务执行中漂移。

## 应用服务与端口

```rust
trait ConfigurationRepository: Send + Sync {
    fn draft(&self, key: ConfigKey) -> PortFuture<'_, ConfigDraft>;
    fn save_draft(&self, edit: DraftEdit, expected: Revision) -> PortFuture<'_, ConfigDraft>;
    fn revision(&self, key: RevisionRef) -> PortFuture<'_, ConfigSnapshot>;
}
trait ConfigurationValidator: Send + Sync {
    fn validate(&self, draft: ConfigDraft) -> PortFuture<'_, ValidationReport>;
}
trait PublicationCommitter: Send + Sync {
    fn publish(&self, plan: PublicationPlan, expected: Revision) -> PortFuture<'_, ReleaseReceipt>;
}
```

ConfigurationService 负责访问控制、依赖解析和预览；PublicationCommitter 负责短事务写入不可变版本、活动指针、发布审计及配置变更事件。网络连通测试是独立、带时限的操作，不能放在发布事务内。校验报告分语法、Schema、引用、权限和可选连通性，不用“能连接”代替授权。

## 状态与一致性

Draft → Validated → Published；已发布快照保持不可变，运行时可处于 PendingActivation、Active、Degraded、Disabled。发布成功只表示配置已保存；ActivationRecord 单独显示节点是否加载成功。节点激活失败继续保留旧实例或停用，按明确的替换策略决定，不能悄悄混用两个版本。

乐观锁冲突返回 Conflict。发布前验证引用仍有效，提交使用期望版本 CAS。撤销是独立当前状态，不修改历史正文。停止连接优先阻止新领取；已开始的外部副作用记录实际结果。对旧配置的待办任务可选择保留、暂停或显式重放，默认不迁移目标。

## 对接与存储

管理 API → ConfigurationService → Validator/Policy → PublicationCommitter → 配置事件 → ConnectorSupervisor。接收事件时记录 route_set_revision，调用时固定 profile_revision/capability_revision，投递保存 destination_revision。

表组为 config_drafts、config_revisions、configuration_releases、active_config_refs、activation_records。编辑权限与执行权限分开。应用模块不能直接改写其他模块的表，只向发布事务端口提供经过校验的计划。

## 迁移与验收

静态 PipelineConfig 可经一次性导入建立初始 Release；已有 pipeline_binding 保留为迁移来源，不能在原库直接改目标 URL。先进行备份、校验和映射预览，再事务导入，旧任务仍绑定旧目标。

验收：并发编辑不会覆盖；发布失败不移动活动指针；回滚有新审计；新配置不改变旧任务收件人；停用立即阻止下一次授权通过后的新步骤；旧节点无法把过期版本报告为当前已激活。
