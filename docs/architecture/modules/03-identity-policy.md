# 身份权限与审批

本模块验证主体并决定每一步是否允许执行。现已支持静态 Bearer 或外部 OAuth introspection，核验 issuer、audience、有效期、scope，并绑定主体、客户端与工具白名单。管理 API 使用独立 bootstrap 凭据；已通过 Keycloak 本地授权、刷新与撤销联调，动态 grant 仓储、审批及公网客户端验证尚待完成。详见[OAuth 接入与在线配置发布](../oauth-publication.md)。

## 领域对象与归属

Principal、Client、Membership、Grant、PolicyRevision、Delegation、Approval 归本模块。认证结果包含工作区、真实主体、客户端、issuer/audience 校验结果、scope 和有效期。ExecutionAuthority 保留 original_actor、服务执行身份、授权引用及委托上限，后台任务不得只留下拥有更大权限的 worker 身份。

AuthorizationRequest 指定 action、resource、Profile/能力版本及必要的已校验参数约束。Decision 为 Allow、Deny、RequireApproval；Allow 携带必须执行的义务，例如字段投影、收件人范围、运行限额及凭据模式。不支持的义务按拒绝处理。

## 应用服务与端口

```rust
trait AuthorizationService: Send + Sync {
    fn evaluate(&self, request: AuthorizationRequest) -> PortFuture<'_, Decision>;
}
trait GrantRepository: Send + Sync {
    fn current(&self, reference: GrantRef) -> PortFuture<'_, GrantState>;
}
trait ApprovalRepository: Send + Sync {
    fn create(&self, plan: ApprovalPlan) -> PortFuture<'_, ApprovalId>;
    fn consume(&self, proof: ApprovalProof, operation: OperationId) -> PortFuture<'_, ApprovalReceipt>;
}
```

Authenticator 仍负责 token/来源凭据验证，授权服务与秘密解密分离。现有 PolicyEngine 的 Result<()> 可作为仅 Allow/Deny 的兼容门面，不能在引入 RequireApproval 后把它默认为 Allow。

## 状态与一致性

Grant 为 Active、Expired、Revoked。审批为 Pending、Approved、Denied、Expired、Consumed；绑定主体、客户端、操作 ID、能力/配置版本、参数摘要及到期时间。一次消费以 CAS 与调用预留同事务完成；同一操作重查返回已有记录，不二次执行。

每次领取、读取敏感内容、取用凭据和外部副作用前重查当前授权。发布快照提供可复现配置，不冻结历史权限。撤销阻止之后的检查通过，向运行任务发送取消信号；已发出的请求不能保证撤回。缓存必须有版本失效机制及明确最长失效窗口。

## 对接与存储

管理入口、MCP、SDK 和 worker 均调用同一授权服务。MCP 用户 token 不转发给上游。入站平台用户以连接命名空间映射，只能获得显式授予的权限。Agent 任务上下文取任务委托、Agent 主体、Profile 和资源范围交集。

表组为 principals、clients、memberships、grants、policies、delegations、approvals；秘密数据在凭据模块。拒绝也应留安全审计；审计失败不能改变拒绝结论。配置发布、调用预留等关键操作在产生副作用前要求持久化审计。

## 验收

错误受众/过期令牌拒绝；跨租户、跨用户任务/附件访问拒绝；worker 不提升原调用权限；撤权后队列不能继续发出新请求；同一审批只能绑定同一操作；不同参数复用审批失败；普通已授权转发不强制进入审批。

## 空间成员授权实现

已实现 Workspace/Membership、Owner/Administrator/Operator/Viewer、SQLite 持久化、CAS 管理和最后所有者保护。WorkspacePolicy 作为 ToolRuntime 原有工具授权的额外限制；成员撤销或空间停用后，后续 MCP 调用鉴权拒绝。具体覆盖范围和 API 见 [空间模型与授权](../workspaces.md)。
