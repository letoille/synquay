# OAuth 接入与在线配置发布

2026-10-24。Synquay 已支持外部授权服务的令牌校验，以及工具暴露范围的在线草稿、预览和发布。配置仍限定单工作区、单 Profile、固定 HTTP 工具清单。管理员发布后，后续 MCP 目录与调用按新版本执行；服务重启会恢复数据库中的发布结果。

## 模块边界

| 层 | 实现 | 职责 |
|---|---|---|
| HTTP 协议 | interfaces/http/mcp.rs、tool_control.rs | MCP 认证挑战、资源发现、独立管理认证、DTO 和容量限制 |
| 身份适配 | infrastructure/tool_runtime/oauth.rs、security.rs | 实现 Authenticator，通过外部授权服务 introspection 验证身份 |
| 发布协调 | infrastructure/tool_runtime/control.rs | 校验暴露集合、协调在途调用与发布、恢复可信快照 |
| 应用契约 | application/ports/publications.rs、services/tools.rs | 草稿/发布仓储端口，工具调用持有发布读锁 |
| 持久化 | infrastructure/storage/publications.rs、migration_v6.sql | CAS、发布幂等、不可变历史和审计原子提交 |

本阶段协调器放在 ToolRuntime 装配边界，因为它依赖可信启动清单与进程内锁；SQLite 细节保持在基础设施仓储中。后续多 Profile 控制面可把协调服务上移，复用现有端口。

## OAuth 资源服务器

提供 `GET /.well-known/oauth-protected-resource/mcp`，同时兼容无路径后缀的发现端点。元数据的 resource 和 authorization_servers 来自可信配置，不从请求 Host 推导。未认证的 MCP 请求返回 401 与 WWW-Authenticate 资源元数据地址；有效令牌缺少要求的 scope 返回 403。

每次 HTTP 请求向固定 introspection_endpoint 发起校验，不缓存令牌。验证 active、issuer、resource audience、有效期、可选 nbf/iat 与令牌类型，再将精确的 sub + client_id 映射为内部主体和客户端。最终工具权限取 grant 白名单与当前发布范围的交集。授权服务撤销令牌后，下一次校验会拒绝；已经开始的外部动作不能据此撤回。

授权服务需要返回 `active`、`iss`、`aud`、`exp`、`sub`、`client_id`、`scope`。RFC 7662 中部分字段是可选项，但本部署契约要求这些身份和受众信息；scope 缺失按权限不足处理。aud 支持字符串或字符串数组，值需包含完全匹配的 resource。授权服务不可用、超时、重定向或返回异常 JSON 时拒绝本次请求，不退回静态令牌。客户端收到 503 可稍后重新认证；不应据此自动重复可能产生副作用的工具调用。

生产端点要求 HTTPS；仅显式开启 allow_loopback_http 时允许 127.0.0.1 / ::1 的开发 HTTP。introspection 使用 client_secret_basic，密钥从环境变量读取，并按表单规则编码客户端标识和密钥。响应最多 16 KiB、并发最多 8、总时限 3 秒、连接时限 2 秒；满载直接返回限流，不无限排队。Agent 令牌不会转发给 HTTP 工具上游。

协议依据：[MCP HTTP Authorization](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization)、[RFC 7662](https://www.rfc-editor.org/rfc/rfc7662.html)、[RFC 9728](https://www.rfc-editor.org/rfc/rfc9728.html)。这里实现资源服务器，用户登录、同意页面、PKCE、客户端注册与令牌签发由外部授权服务承担。已完成 Keycloak 26.8.1 本地真实授权联调；ChatGPT 客户端的公网端到端验证留到下一阶段。

## 配置和凭据

`SYNQUAY_TOOLS_CONFIG` 指向工具文件；新增 `SYNQUAY_TOOLS_SECURITY_CONFIG` 指向安全文件。参考 `examples/tools-security.example.json`：替换 issuer、公开 resource、introspection 地址、服务凭据引用、真实主体和 OAuth 客户端绑定。resource 当前必须以 `/mcp` 为路径，不支持反向代理添加路径前缀。域名必须与客户端请求和授权服务 audience 一致。

如只需启用管理 API，可先使用 `examples/tools-admin.example.json`，保留原来的静态 MCP Bearer。管理员与 MCP 静态令牌必须使用不同的环境变量、值和主体；管理员令牌至少 32 个可见 ASCII 字符。OAuth 模式不需要设置 MCP 静态令牌。配置文件只保存凭据引用，GET 管理视图可见引用名称但不返回秘密值。

管理认证目前是独立 bootstrap Bearer，不是 OAuth 管理员角色系统。安全配置、grant、上游连接地址、Schema 与凭据引用变更仍需修改可信文件并重启。已增加精确 allowed_origins 白名单和仅内存令牌的首期管理台；未配置的 Origin、重复 Origin 和 Sec-Fetch-Site: cross-site 请求被拒绝。完整管理员 OIDC 登录仍待实现。见[管理台运行说明](../management-console.md)。

## 草稿、预览和发布 API

所有 `/api/tools/*` 请求需要 `Authorization: Bearer <管理员令牌>`，响应禁止缓存。最多接纳 4 个管理请求、1 个发布操作；JSON 最大 16 KiB、读取时限 5 秒、请求时限 35 秒。

| 方法和路径 | 输入或结果 |
|---|---|
| GET /api/tools/config | 当前 revision、profile_id、admin_id、可信 tools 清单、connection 接入元数据与最新 draft，供对比发布前后暴露集合 |
| GET /api/tools/history | limit 和 before_revision 游标分页；版本、操作者、时间及开放能力 |
| GET /api/tools/operations/{operation_id} | 带 expected_revision 与 draft_generation 核查 committed 或 unconfirmed |
| PUT /api/tools/draft | expected_generation、base_revision、exposed_tools |
| POST /api/tools/publish | operation_id、expected_revision、draft_generation |

例如当前 revision=1、尚无草稿时，隐藏全部工具：

```json
{"expected_generation":0,"base_revision":1,"exposed_tools":[]}
```

保存返回 generation=1。发布时使用新的稳定 operation_id，并在同一发布意图的网络重试中复用它：

```json
{"operation_id":"00000000-0000-0000-0000-000000000099","expected_revision":1,"draft_generation":1}
```

成功返回新 revision=2 和 replayed=false。重复相同请求返回原结果及 replayed=true；同 operation_id 换输入、旧草稿代数或旧基准版本返回冲突。修改草稿时读取最新 generation；再次发布时以当前 revision 为 base 保存新草稿。恢复已隐藏工具可提交 `exposed_tools:["echo"]`。空集合有效，未知或重复名称拒绝；API 不能增加任意 URL、Schema 或环境变量引用。

## 并发和恢复语义

工具 list/invoke 全程持有发布读锁，发布持有写锁。已经接纳的本地调用结束后才提交新版本，提交后的调用必须使用新目录名称，例如 `echo__r2`。旧名称不能借助重放绕过当前发布校验。工具调用原有超时和未知结果规则继续适用，外部副作用不随配置回滚。

发布被接纳后运行在有界独立任务中，HTTP 断开不会提前释放写锁。响应超时或提交结果不明时，先用只读 operation 查询核查结果；未确认时可用相同 operation_id 和参数重试，不创建新的发布意图。进程崩溃由 SQLite 事务保证发布指针和审计一起保存或回滚。

启用 admin 后，启动文件 revision 不高于数据库当前版本时，会恢复数据库暴露集合；恢复要求可信工具清单、身份、连接和凭据引用一致。文件改动发生冲突时停止启动；受信任的新清单通过当前数据库 revision+1 的文件发布路径显式升级。关闭 admin 则恢复原文件版本校验规则，旧启动文件可能无法启动，需先对齐当前版本。

客户端需重新查询工具目录。本阶段不推送 tools/list_changed，也不提供跨进程配置广播。

## 存储与索引

SQLite 自动迁移至 v7；迁移失败保留原版本。新增三张表：

| 表 | 查询及约束 | 索引 |
|---|---|---|
| tool_drafts | 按 workspace/profile 查询或 CAS 更新最新草稿 | 复合主键 workspace, profile |
| tool_draft_audit | 不可变草稿代数、编辑主体和内容 | 复合主键 workspace, profile, generation |
| tool_publish_operations | 操作去重、绑定输入及返回原发布版本 | 复合主键 workspace, profile, operation |

草稿与操作查询由原主键覆盖；v7 为分页历史关联新增 tool_publications(workspace,profile,revision) 唯一索引。发布事务同时写入 release、head、管理员发布审计和 operation 结果。新草稿/发布写入在磁盘空闲不足 256 MiB 时拒绝；已完成发布重放仍可查询。历史不会自动删除，现有调用账本额度不包含完整发布历史，后续需补归档策略。备份、恢复与只读工具检查脚本支持 v7。

## 验证与后续边界

93 项 Rust 测试通过，另有 1 项默认忽略的手工微基准；fmt、clippy 无警告。新增 7 项测试覆盖真实 SDK 调用、OAuth 声明与权限拒绝、撤销、授权服务并发/超时、草稿竞争、发布等待、调用方断开、审计事务回滚、重启恢复与 v6 迁移回滚。

`python3 scripts/smoke_oauth_control.py` 用编译后的服务器、官方 SDK 示例客户端与本地模拟授权服务，验证配置装配、发现、调用、管理凭据隔离、在线隐藏、重复发布、旧文件重启恢复、撤销及日志不含测试秘密。此前的 MCP 进程冒烟也通过。运行前执行 `cargo build --locked --examples --bin synquay-server`。

Keycloak 本地授权、刷新、撤销与 MCP 调用已通过，见[真实授权联调](keycloak-integration.md)。首期管理台已实现；下一步完善运行视图与动态 grant 管理；公网联调另行安排。上游 MCP 桥接、Agent 收件箱和原路回复仍需各自实现；当前没有令牌签发服务、完整 OIDC 登录、分布式发布或自动调用 AI 的后台任务。
