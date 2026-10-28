# MCP 工具发布与调用闭环

Synquay 已实现单工作区的“配置 HTTP 工具 → 选择暴露范围 → 发布版本 → MCP 认证调用 → 保存结果与审计”。本文记录最初文件配置闭环；后续已加入[OAuth 与在线暴露发布](oauth-publication.md)。连接和凭据引用仍来自可信文件，协议层复用 ToolService 与 Governance。

## 模块与调用链

| 层 | 实现 | 职责 |
|---|---|---|
| 协议入口 | interfaces/http/mcp.rs | 官方 rmcp 3.7.0 Streamable HTTP，认证、限流、DTO 转换 |
| 配置和组合 | infrastructure/tool_runtime | 文件校验、固定主体策略、连接器注册、环境凭据、Schema 预编译 |
| 应用服务 | application/services/tools.rs | Profile 校验、权限、参数校验、调用及输出校验 |
| 持久化端口 | application/ports/invocations.rs | 调用接纳、重复请求判定、保存完成结果和审计 |
| SQLite 适配器 | infrastructure/storage/tools.rs | 发布事务、调用去重、容量预留和完成事务 |
| HTTP 执行器 | infrastructure/tool_runtime/http.rs | 固定 URL 的 JSON POST、凭据注入、有界响应和超时 |

MCP 入口为 `/mcp`。最初只发布 tools，现已扩展[资源、模板、提示词和参数补全](../webhook-mcp.md)。真实 SDK 客户端已验证 2026-07-28 的 discover 生命周期，以及 2025-11-25 的 initialize 生命周期。服务无会话状态，不依赖 MCP Session ID。新版本握手、字段和传输由[官方 Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)处理。

## 配置发布

示例为 `examples/tools.json`；通过 `SYNQUAY_TOOLS_CONFIG` 启用。单进程最多 32 个工具，每个工具绑定独立 connection_id，当前一个数据库服务一个运行中的工具配置。文件租约阻止同一数据库被两个 ToolRuntime 同时激活；这不是分布式部署支持。

首次发布 revision=1，以后每次配置变更必须递增 1。完全相同的当前快照可重复启动；同版本不同内容、跳版本和回退版本均拒绝。回滚旧内容应发布新 revision。工具定义、暴露范围、地址和凭据引用属于快照；数据库路径与凭据值不属于快照。校验全部成功后，版本、当前指针和发布记录同事务提交。

`exposed=false` 的工具既不会出现在目录，也不能直接调用。修改后递增 revision 并重启。客户端看到名称 `echo__r1`，版本后缀固定其观察到的配置和 Schema；发布新版本后旧名称调用失败，需要重新发现目录。

文件发布记录保存配置中的服务主体，并不证明操作文件的真实管理员身份。后续在线暴露发布已使用独立管理员凭据，保存草稿、发布审计与操作去重；连接和凭据引用仍受文件权限控制，草稿审批尚未实现。

## 认证与权限

入口凭据从 token_env 指定的环境变量读取，长度 32–4096 个可见 ASCII 字符。每个请求校验 Bearer，再按固定 workspace、principal、Profile 和已暴露连接器授权；工具调用前再次检查。凭据值不进入配置快照、审计或请求日志。上游凭据通过各工具独立的 bearer_env 引用；不会将调用者 Bearer 转发给上游。

静态 Bearer 模式仍可选用；后续已加入外部 OAuth introspection 与资源发现。带 Origin 的浏览器请求全部拒绝，对外需由反向代理终止 TLS。用户登录由外部授权服务承担，Keycloak 本地真实授权已通过，ChatGPT 公网端到端授权仍待联调。

## 执行语义和异常

客户端可在 `_meta["synquay/invocation-id"]` 提供稳定 UUID，同一逻辑调用重试时保留它。未提供时服务端生成新 UUID，并在结果 `_meta` 返回；JSON-RPC request ID 不作为业务去重标识。新 UUID 表示新操作，不能承诺跨新 UUID 的恰好一次执行。

账本键为 workspace、principal、invocation。指纹覆盖 Profile、revision、工具、连接范围和规范化 JSON 参数；参数只存 SHA-256 指纹，完成结果会保存在私有数据库中。同键异参返回冲突；已完成且仍获授权的同键请求返回结果，不再次请求上游。并发中的重复请求、崩溃后未完成的记录返回 OutcomeUnknown。即使进程重启也不会自动重试未知结果；未知结果的人工核查与重放管理 API 尚未实现。

开始审计必须成功才执行。最终结果与结束审计同事务保存，失败时账本保持未完成；实际执行结果与 `synquay/audit-recorded=false` 一起返回，避免把审计故障描述成“尚未执行”。输入验证、隐藏工具和旧版本检查均在执行前完成。结果复用仍检查当前授权和当前发布版本。

HTTP 只对配置的固定 URL 发 JSON POST，不接受模型提供 URL、方法或凭据。仅允许 HTTPS，或显式开启的 IP 回环 HTTP；拒绝 URL 用户信息、查询参数和片段，关闭重定向和环境代理。运维者应将凭据放在 bearer_env，不能放入路径。内网 HTTPS 是显式配置的信任范围，不提供多租户动态 URL 的 SSRF 隔离。

请求和响应上限均为 64 KiB；输出必须为 JSON object。2xx 之后读失败、响应过大、错误格式、非成功 HTTP 状态或发送后取消，都可能已经产生副作用，因此不自动再发。输出 Schema 不符也会保留失败结果。Schema 使用 JSON Schema 2020-12，启动时预编译；仅允许本地 fragment 引用，禁止 `$id` 和外部引用。

## 有界资源和数据库

入口最多 32 个进行中的请求，读请求体最多 5 秒；外部 HTTP 最多 8 个并发，连接超时 5 秒、总超时 20 秒。应用操作有 25 秒期限，入口 30 秒兜底。复用 reqwest 连接池，不为每次请求建立客户端。此阶段未新增 MCP 吞吐基准，P0 的历史测量不能替代 MCP 性能结论。

SQLite v5 在同一个迁移事务中加入 tool_releases、tool_heads、tool_publications、tool_invocations 与容量计数表；v1–v4 自动升级，未知版本拒绝。Profile 查询和调用查重使用复合主键，不再增加重复索引。账本容量以计数器和事务触发器维护，调用路径不扫描全表。每个新调用先预留 257 KiB，完成后缩减到实际结果大小加元数据预算；上限为 100000 项、1 GiB 记录预算，磁盘空闲低于 256 MiB 拒绝新调用。额度用尽时既有完成/查重仍可运行。

这些预算不等于数据库文件大小，也不覆盖配置历史和审计的全部空间。去重账本不会自动删除，避免清理后误重放；需在后续定义工具账本归档和保留窗口。`scripts/inspect_tools.py` 可只读查看版本、占用和未完成/未知数量；不输出参数、凭据或结果。现有离线备份/恢复脚本已支持 v10，会包含工具表及后续新增的在线发布记录。

## 本地运行

从项目根目录启动测试上游：

```sh
python3 scripts/demo_tool.py
```

另一终端设置至少 32 字符的随机 token，再启动后端：

```sh
export SYNQUAY_MCP_TOKEN="$(openssl rand -hex 32)"
export SYNQUAY_TOOLS_CONFIG=examples/tools.json
cargo run -p synquay-server
```

客户端终端使用相同 token。先列出工具，再按返回名称调用；下面的调用 ID 示例用于同一逻辑请求，真实新请求应生成新 UUID：

```sh
export SYNQUAY_MCP_URL=http://127.0.0.1:8080/mcp
cargo run -p synquay-server --example mcp_client
cargo run -p synquay-server --example mcp_client -- echo__r1 '{"value":7}' 00000000-0000-0000-0000-000000000007
python3 scripts/inspect_tools.py ./synquay-tools.db
```

重复执行相同调用命令只返回已存结果；相同 ID 改成 value=8 会冲突。将 exposed 改为 false、revision 改为 2 并重启后，目录为空且旧版本不能调用。

## 验收与下一阶段

83 项 Rust 测试通过，另有 1 项默认忽略的手工编码基准。本阶段新增 7 项集成测试，覆盖真实 SDK 客户端、新旧协议握手、认证/Origin/大小限制、工具隐藏、旧版本拒绝、Schema、并发重复调用、执行中断后重启、发布 CAS、输出异常、账本额度、结束审计原子性和 v5 迁移回滚。

验证命令：`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`；实际进程验证使用 `scripts/smoke_mcp.py` 和 `scripts/smoke_pipeline.py`，先执行 `cargo build -p synquay-server --examples --bin synquay-server`。

后续已补 OAuth 资源服务器和在线暴露发布，当前进展见[OAuth 接入与在线配置发布](oauth-publication.md)。Keycloak 本地联调已通过；接下来完成管理台和公网客户端验证，再做上游 MCP 桥接。CLI/MQTT 执行器、长任务、完整在线配置 UI、消息经 AI 分析后的原路回复仍按模块路线推进；静态资源/提示词文件发布已完成。
