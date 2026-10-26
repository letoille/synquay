# 能力目录与发布

本模块管理可发现能力及客户端可见范围。已实现持久化 PublishedProfile、JSON Schema 2020-12 校验，以及可信 HTTP 工具清单内的在线暴露发布；MCP 目录取当前发布与调用者授权的交集。自动能力发现及 resources/prompts/events 尚待实现。详见[OAuth 接入与在线配置发布](../oauth-publication.md)。

## 新增的实现基础

统一动作/状态/事件描述、业务资源、协议绑定和不可变内存目录已经实现，HTTP 工具运行时已接入。结构化使用说明可经 MCP tools/list 提供给 Agent；MQTT、CLI、上游 MCP 目前只有绑定契约。详见[统一能力层](../capability-layer.md)。下文持久化发现目录及自动导入仍是目标设计。

## 领域对象与归属

CapabilityDefinition 包含稳定 capability_id、所属连接、kind、上游标识和发现来源；CapabilityRevision 保存 Schema、描述、内容摘要和行为约束。kind 为 Tool、Resource、ResourceTemplate、Prompt 或 Event。DiscoverySnapshot 是观察结果；ExposureProfile 是管理员选择；PublishedProfileRevision 是已发布集合。发现到的新增能力默认不暴露。

资源定义保存受控 URI 模板，事件定义保存载荷 Schema，Prompt 保存模板与参数。SchemaRegistry 管理支持的 dialect、引用集合和大小/深度限制；能力描述和工具注解不自动获得执行权限。

## 应用服务与端口

```rust
trait CapabilityDiscovery: Send + Sync {
    fn discover(&self, context: OperationContext) -> PortFuture<'_, DiscoverySnapshot>;
}
trait CapabilityCatalog: Send + Sync {
    fn revision(&self, key: CapabilityRevisionRef) -> PortFuture<'_, CapabilityRevision>;
    fn record_discovery(&self, snapshot: DiscoverySnapshot) -> PortFuture<'_, CatalogDiff>;
}
trait ResourceReader: Send + Sync {
    fn read(&self, context: OperationContext, request: ResourceRead) -> PortFuture<'_, ResourceContent>;
}
trait PromptProvider: Send + Sync {
    fn render(&self, context: OperationContext, request: PromptRequest) -> PortFuture<'_, PromptContent>;
}
```

复用既有 ToolExecutor、ProfileRepository、SchemaValidator；事件来源与对客户端发布的事件定义分开。CatalogService 负责差异分析，ProfileService 负责暴露选择、别名、字段约束与发布预览；配置发布事务引用这些不可变版本。

## 状态与一致性

能力可为 Discovered、Verified、Unavailable、Deprecated；Profile 经过 Draft、Validated、Published。上游 Schema 不兼容变化将已发布绑定标记不可用并提示差异，不自动扩大参数或资源范围。公开名称和 URI 在工作区/Profile 命名空间中稳定且唯一，不依赖上游显示名。

列表、读取、调用使用同一 PolicyEngine。Profile 隐藏不能代替授权；直接传内部名称、URI 或下载引用仍检查范围。目录缓存键包含 workspace、principal、client、grant/policy 版本与 Profile revision，撤权使缓存失效。分页游标绑定相同查询范围。

## 对接与存储

ConnectorDiscovery → CatalogDiff → 管理员选择 → Profile 发布 → MCP/管理预览。请求先查发布快照再解析内部能力，不允许直接透传任意上游 tool_id。输入在调用前校验，输出在执行后校验；输出无效不能被解释为调用没有发生。

表组为 capabilities、capability_revisions、discovery_snapshots、profiles、profile_revisions、exposures、schema_documents。权限受控的资源/附件在内容模块保存，目录仅保存引用。存储层版本、能力 schema 版本和协议版本互不替代。

## 迁移与验收

将现有 PublishedProfile 的 tools 保持为可兼容投影，逐步扩展有区分标签的 exposures，避免一次性破坏已有 ToolService。验收：同名上游工具稳定映射；隐藏直调被拒绝；上游新增不自动发布；未知 Schema 引用拒绝；受限结果字段不会从嵌入资源绕出；旧发布快照可定位。
