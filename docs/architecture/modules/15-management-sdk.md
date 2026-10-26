# 管理接口与 SDK

管理接口和 SDK 是应用服务的客户端边界。已有 React 首期管理台、独立管理员 Bearer、显式浏览器来源校验，以及工具暴露范围编辑、预览、发布回执、结果核查与分页历史。完整管理员 OIDC 登录、连接向导与 SDK 尚待实现。前端不复制权限引擎或直接访问数据库。详见[管理台运行说明](../../management-console.md)与[OAuth 接入与在线配置发布](../oauth-publication.md)。

## 应用视图与契约

产品交互见[管理台设计](../management-console.md)：围绕连接、MCP 服务、消息规则和运行记录组织入口，首期实现能力选择、草稿、预览、发布回执及历史。设计遵循简洁、易用、正式、克制；正式界面只开放已具备服务端支持的功能。

管理视图包括 ConnectionView、ProfilePreview、ReleaseDiff、DispatchTimeline、InvocationView、AnalysisView、RecoveryCaseView。读模型可聚合多个模块，但不成为第二份权威状态。所有列表分页、过滤和游标绑定授权范围，不能只在 UI 隐藏敏感字段。

```rust
trait ManagementQuery: Send + Sync {
    fn connection(&self, query: AuthorizedConnectionQuery) -> PortFuture<'_, ConnectionView>;
    fn timeline(&self, query: AuthorizedTimelineQuery) -> PortFuture<'_, DispatchTimeline>;
}
trait ConfigurationTransfer: Send + Sync {
    fn preview_import(&self, input: AuthorizedImport) -> PortFuture<'_, ImportPlan>;
    fn export(&self, query: AuthorizedExport) -> PortFuture<'_, RedactedConfigBundle>;
}
```

写接口直接调用拥有状态的模块用例；ManagementQuery 只负责读聚合。SDK 提供认证、事件封装、固定幂等键重试、游标、任务领取和结果提交；不绕过服务端权限，不直接插入存储。

## 管理流程

连接向导：选择类型 → 填写配置/凭据引用 → 校验 → 可选连通测试 → 保存草稿 → 发布激活。能力向导：发现差异 → 选择能力 → 预览当前用户可见内容 → 发布 MCP Profile。状态页展示接收、路由、分析、回复和投递各阶段，并区分 unknown 与 failed。

普通查询不需要审批。重放、覆盖配置或策略要求审批的操作先生成具体预览，使用计划 ID、版本、参数摘要及有效期提交，防止确认后内容变化。前端只表达用户意图，服务端仍重新校验权限与对象版本。

## 接口边界

现有 `/api/webhooks/{connection_id}` 保持兼容；新增管理命令使用版本化 API。服务端 ETag/expected_revision 防止编辑丢失，命令支持有作用域的幂等键。配置导出只含 secret_ref 和脱敏状态；导入不能覆盖现有秘密或自动启用外部发送。

OpenAPI/JSON Schema 与前端类型从同一契约构建并做兼容检查。适配器配置表单由已审核的 Schema 驱动，字段限制在服务端仍执行。跨语言 SDK/进程适配协议有独立版本协商，不默认允许任意脚本执行。

## 状态通知与验收

管理台事件流只用作刷新提示，断开后按持久化 cursor/查询补齐。浏览器收到进度不等于任务持久化完成；核心响应使用操作 Receipt。所有 API 仍带 request_id，返回安全错误码、是否可安全重试及必要的版本冲突信息。

验收：篡改前端请求不能绕过授权；并发编辑冲突可见；导出不泄露秘密；导入只形成草稿；失联刷新能恢复时间线；同一 SDK 事件重试沿用 source_event_id；同一任务结果重交不会生成重复回复。
