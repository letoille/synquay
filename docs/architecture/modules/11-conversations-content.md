# 会话回复与附件

P1 已实现受保护 ContentRef、FileContentStore、HTTP 流式上传/下载及单内容二进制投递；多附件会话回复、对象存储及按任务授权仍待实现。详见[P1 实现](../p1-storage.md)。

本模块拥有消息的渠道身份、会话关系、回复地址与内容引用。已有 Message/ContentPart、ReplyRoute 和 ReplyService；本地持久化内容已在 P1 实现；首个 Agent 回复闭环通过固定 HTTP 来源适配器回传原消息 ID，详见[收件箱实现](../../agent-inbox.md)；通用会话映射、多渠道路线创建及多附件回复仍待补。

## 领域对象与归属

Conversation 包含连接命名空间、外部会话 ID 和可选线程；MessageRecord 保存 source_message_id、sender_ref、会话、内容及修订；MessageLink 关联源消息、Delivery 和外部目标消息 ID。平台昵称不用于高权限身份绑定。

ReplyRouteRevision 包含原连接版本、原会话/线程/消息、允许回复模式、目标主体、授权引用和有效期。AttachmentRef/ArtifactRef 指向受保护 Blob，保存大小、类型、摘要、来源与访问范围。内容 URL 是受权限控制的交付方式，不是访问权本身。

## 应用服务与端口

```rust
trait ConversationRepository: Send + Sync {
    fn context(&self, query: AuthorizedContextQuery) -> PortFuture<'_, ConversationContext>;
}
trait ReplyPlanner: Send + Sync {
    fn plan(&self, request: AuthorizedReplyRequest) -> PortFuture<'_, ReplyIntent>;
}
trait ContentStore: Send + Sync {
    fn stage(&self, upload: AuthorizedContentUpload) -> PortFuture<'_, StagedContent>;
    fn read(&self, query: AuthorizedContentRead) -> PortFuture<'_, ContentStream>;
}
```

扩展现有 ReplyRouteStore：路线创建只能从验证后的入站渠道上下文产生，由 IngressCommitter 原子绑定消息。交互式 ReplyService 调用 Planner 后提交 Outbox；Agent 完成流程使用同一 Planner，但通过 AnalysisCommitter 提交，避免两次独立写入产生部分成功。

## 状态与事务

返回路线 Active、Expired、Revoked；消息修订追加保存；附件为 Staged、Available、Quarantined、Expired。Blob 先上传，消息事务只发布已验证引用；未被事务引用的 staged Blob 到期清理。跨对象存储不伪装成单数据库事务。

外部平台回复成功后，将 MessageLink、Delivery 状态和实际回执同事务记录。平台已发送但回包丢失时走核查，不生成假的目标消息 ID。来源编辑和删除是否同步由显式路由策略定义，初版可只支持新增消息，能力声明必须反映实际情况。

## 渠道能力与对接

ChannelFeatures 区分 receive、send、reply、threads、attachments、edit、receipts。Planner 将类型化回复映射到渠道支持范围；缺少线程可按已配置规则降级为引用，缺少附件只能显式拒绝或提供授权链接，不能静默遗漏。

MCP 资源、工具输出与邮件附件共用内容访问授权。下载按租户、主体、任务和有效期限制，路径不会直接映射宿主文件。远程内容抓取使用单独的固定来源/出口策略，不能由任意模型 URL 驱动内部网络访问。

## 存储与验收

表组为 conversations、messages、message_revisions、reply_routes、message_links、content_objects、content_grants。正文保留期与最小去重/映射记录分开管理，删除内容不立即允许旧消息再次被误当新消息发送。

验收：同一平台消息重收不重复建路线；回复只能到原授权会话；过期/撤销路线拒绝；跨任务附件读取拒绝；不支持附件时明确反馈；结果完成原子关联回复；保留期清理不删除正在使用的产物。
