# MCP 协议与桥接

MCP 模块负责协议适配，不拥有业务权限、任务或消息状态。已接入官方 rmcp 3.7.0 的 Streamable HTTP tools 服务，验证 2026-07-28 discover 与 2025-11-25 initialize，并增加 OAuth 资源发现、认证挑战和 introspection。现已加入静态资源、有限资源模板、提示词和参数补全，统一经过权限与审计；见[Webhook 与 MCP 内容能力](../../webhook-mcp.md)。上游桥接与事件通道待实现。详见[OAuth 接入与在线配置发布](../oauth-publication.md)。

## 三个独立边界

DownstreamMcpServer 将已授权的 Profile 映射给客户端；UpstreamMcpConnector 作为客户端连接已有服务器；McpEventTransport 适配经验证的业务事件通道。三者独立声明能力，普通工具调用可用不代表事件、Sampling 或后台触发可用。

McpSession 包含协商版本、客户端声明能力、身份与会话生命周期。CapabilityBinding 将公开名/URI 映射到内部版本。CompatibilityRecord 保存客户端/服务端版本、支持项和最近验收结果。

## 应用与协议端口

```rust
trait McpSessionStore: Send + Sync {
    fn resolve(&self, reference: AuthenticatedSessionRef) -> PortFuture<'_, SessionCapabilities>;
}
trait UpstreamMcpSession: Send + Sync {
    fn capabilities(&self, context: OperationContext) -> PortFuture<'_, UpstreamCapabilities>;
}
trait EventSubscriptionStore: Send + Sync {
    fn activate(&self, plan: AuthorizedEventSubscription) -> PortFuture<'_, SubscriptionReceipt>;
    fn revoke(&self, command: SubscriptionRevocation) -> PortFuture<'_, ()>;
}
```

下游协议 handler 调用 ToolService、资源/Prompt 服务、AgentJobService；上游会话实现 ToolExecutor、ResourceReader、PromptProvider 等能力端口。协议 SDK 的请求/响应类型只出现在 interfaces/infrastructure，领域层不导入 SDK。

## 目录与结果映射

公开工具别名固定到连接/能力版本；不使用上游自报显示名作为唯一身份。resources/list、resources/templates/list、resources/read、Prompt 参数、工具 embedded resource 和 resource_link 都经过命名空间重写和授权。上游返回 URL 不直接变成无授权下载通道。

上游目录改变只产生待审核差异，不自动扩大已发布内容。业务错误、传输错误、取消、进度和长任务分开映射；未知/未支持的双向交互返回明确兼容错误。对外调用幂等使用业务调用键，不依赖不同会话中可能重复的协议 request_id。

## 身份和事件

入口遵循所选协议版本的资源服务器认证要求，使用 Authenticator/AuthorizationService；客户端 token 不透传给上游。上游认证按连接 CredentialBinding 获取。会话断开不删除已经持久化的调用/分析任务，取消意图交给拥有状态的服务处理。

事件订阅绑定主体、事件定义、过滤、回调引用、签名凭据、有效期和游标；订阅范围只能缩小已有权限。回调验证及签名在具体传输适配器实现，投递仍使用 Outbox 和当前授权。撤销阻止新通知，已在外部执行的请求保留实际结果。

## 验收与实施边界

先验证一个客户端的认证、tools/list/call 和隐藏工具直调拒绝，再验证两个上游同名工具桥接、资源读取授权和断连恢复。事件通道另做兼容验收；没有事件能力时以持久化 Inbox 加主动领取提供可用路径。

SDK 选型原型以现有 Rust SDK 候选为起点，实际依赖与协议版本在实施时核实并记录。客户端能力矩阵由测试结果生成，不从设计草案推断宿主一定支持。
