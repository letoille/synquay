# 空间：领域模型、成员与授权边界

2026-10-24。产品称“空间”，后端沿用 `Workspace` / `workspace_id`，不另建 Space ID。第一阶段完成领域模型、持久化端口、SQLite 实现、MCP 工具运行时鉴权和管理员 API；现有部署继续绑定一个默认空间。

## 已实现的范围

- Workspace 保存 ID、显示名称、Active/Suspended 状态及单调 revision。
- Membership 保存空间、主体、Owner/Administrator/Operator/Viewer 角色及 Active/Revoked 状态。
- WorkspaceRepository 定义首次初始化、同快照读取访问状态、带授权与 CAS 的更新、成员游标分页。
- WorkspacePolicy 包装原 PolicyEngine，权限取空间成员规则与既有资源/Profile/OAuth 授权的交集。Owner 也不能绕过内层授权。
- MCP 工具列表、调用及调用派发前均检查当前成员与空间状态；没有跨请求成员缓存。管理台工具草稿、发布也受空间状态约束；停用后保留读取和恢复管理入口。
- SQLite v8 增加空间、成员、变更记录，成功变更与通用审计同事务提交。审计失败会回滚角色、状态、revision 和变更记录。

当前仅接入 ToolRuntime 的鉴权。独立 webhook 转发 Pipeline 的接收、路由、投递、附件和运维入口尚未接入这个成员目录，其 workspace_id 仍沿用原有隔离规则；不能据此宣称停用空间已暂停整个系统的所有连接。两种运行时目前可使用不同数据库，后续接入必须先确定共享的权威空间仓储，不能各自缓存一份成员状态。

## 空间、Profile 与资源

空间定义归属与成员边界；资源权限限制具体对象和动作；Profile 决定对某个 MCP 接口发布哪些能力。三者独立且相交。

服务器从可信运行配置和已验证主体确定空间。新增管理 API 不接收客户端指定的 workspace_id/actor_id；额外字段被拒绝。OAuth 令牌不能通过工具参数、MQTT 消息正文或资源名称切换空间。业务资源继续使用上一阶段 ResourceRef 的 workspace/connection 范围。

用户、Agent 和服务账号均以已认证 principal 表示；客户端身份和 OAuth scope 仍由既有认证层维护。空间成员资格不是一个新的凭据，也不代替账号绑定。资源分组、跨空间共享和层级继承尚未实现。

## 角色语义

| 角色 | 数据能力上限 | 管理能力 |
|---|---|---|
| Owner | 仍需原有工具与资源授权 | 修改名称、空间状态和成员；最后一名有效所有者不能撤销或降级 |
| Administrator | 仍需原有工具与资源授权 | 读取空间/成员、修改名称；不能授予成员角色或停用空间 |
| Operator | 执行已授权动作 | 无空间管理权限 |
| Viewer | 使用已授权 Profile，并仅调用可信配置标记 ReadOnly 的已授权工具 | 无空间管理权限 |

Unknown 与 SideEffecting 工具不对 Viewer 开放。ReadOnly 标记来自可信配置，仍必须通过内层工具授权；文档或上游消息不能自行获得权限。

当前 HTTP 管理认证仍是一个独立 bootstrap 管理员，尚无多管理员 OIDC 登录。所以 API 只允许管理可信配置中已有主体，不允许改变当前 bootstrap 管理员的角色/状态，也不允许创建新的 Owner，避免失去唯一可用的恢复入口。存储契约支持多个 Owner，并有最后所有者保护，供后续完整身份管理使用。

## 初始化与兼容

首次 ToolRuntime 启动，为已配置 workspace_id 创建“默认空间”。配置了管理员时，管理员为 Owner；静态工具主体与 OAuth grants 中的主体为 Operator。没有管理员的旧配置，以原静态主体作为初始 Owner，既有工具权限不变。

同一空间已经存在时，bootstrap 不覆盖名称、状态或任何成员；重启不会恢复撤销成员，也不会恢复被停用的空间。后来在 OAuth 配置中新增的主体，还需要管理员通过成员接口明确加入空间。修改 bootstrap 管理员身份也不会自动授予已有空间的所有者权限。

## 管理 API

沿用 `/api/tools/*` 的独立管理员 Bearer、精确 Origin 检查、请求体限额和 `Cache-Control: no-store`：

| API | 用途 |
|---|---|
| GET /api/workspace | 当前绑定空间的 id/name/state/revision |
| GET /api/workspace/members?limit=50&after=UUID | 成员列表；limit 1–100，按 principal ID 游标分页 |
| PATCH /api/workspace | 修改空间或成员；必须提供 expected_revision |

修改空间名称：

```json
{"expected_revision":1,"change":{"kind":"rename","name":"我的家庭"}}
```

撤销已配置的工具主体：

```json
{"expected_revision":2,"change":{"kind":"set_member","principal_id":"00000000-0000-0000-0000-000000000003","role":"operator","state":"revoked"}}
```

停用或恢复：

```json
{"expected_revision":3,"change":{"kind":"set_state","state":"suspended"}}
```

恢复时把 state 改为 active，并使用重新读取的 revision。每次成员或空间变更都会增加空间 revision，旧版本返回 409。响应丢失时先 GET 核查，不用新版本盲目重复修改。成员分页每页独立读取，跨页不是固定快照。

## 并发与撤权语义

所有变更使用 SQLite Immediate 事务；检查操作者成员状态、CAS、最后所有者检查、更新及审计在同一事务中完成。正常调用在授权时读取最新已提交状态，并在调用账本/审计等待后、实际派发前再检查一次。

撤权阻止之后的授权检查通过，不保证撤回已经发出的请求，也不能把外部执行与数据库变更组成原子事务。当前没有强制中断在途 HTTP 请求；客户端得到的未知结果仍按已有调用账本核查，不能自动当作“未执行”。未来队列 worker 和定时任务需要保留原主体并使用同一仓储重新授权，不能只使用 worker 服务账号。

## 存储与索引

- `workspaces(id)` 主键用于空间精确查询。
- `workspace_members(workspace,principal)` 复合主键覆盖成员鉴权和 principal 游标分页。
- `workspace_active_owners(workspace,principal)` 部分索引仅包含有效 Owner，用于最后所有者存在性检查。
- `workspace_changes(workspace,revision)` 复合主键保存初始成员快照和后续变更，供按空间版本核查；request_id 对应服务端请求 ID。
- 单空间成员总数上限 256，包含撤销记录和所有者。保留撤销记录以避免自动恢复；成员分页最多 100。空间级消息/存储/并发配额仍待统一接入现有容量模块。

v8 与既有迁移一起事务执行。升级失败保留旧版本，不留下部分表。备份和检查脚本已接受 v8；现有数据迁移测试已更新。

## 后续边界

- 管理台空间切换、创建空间、成员编辑界面和多管理员认证。
- 将 webhook Pipeline、附件、后台投递和未来定时任务接入同一个空间授权来源。
- 空间级运行配额、资源分组、MQTT 逻辑连接与 topic 范围授权。
- 空间删除、完整所有权转移、跨空间显式共享。

本阶段没有新增空间切换 UI，也没有通过接口暂停正在运行的用户空间。测试使用独立临时数据库。

## 验证与本地运行状态

114 项 Rust 测试通过（其中新增 7 项空间仓储/策略测试、1 项管理 API 集成测试），另 1 项手动性能基准默认忽略；fmt、clippy 和开发构建通过。覆盖撤权重启、暂停恢复、CAS 并发、最后所有者、审计回滚、查询索引、v8 迁移失败回滚及 HTTP 权限/来源隔离。

当前本地预览进程尚未重启：沙箱拒绝后台进程身份检查，因此没有继续停止服务或迁移该预览数据库。新版 API 已通过临时服务集成测试。重启前应备份数据库；新二进制首次打开时会事务升级到 v8。
