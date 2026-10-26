# Synquay · 汇流港

连接应用、事件与 AI Agent。后端和前端统一放在本仓库，当前包含项目骨架和可运行的持久化 HTTP 消息转发链。

已增加带认证的 MCP HTTP 工具闭环，配置和本地示例见 [MCP 工具发布与调用](docs/architecture/mcp-tools.md)。

P0 调度、恢复接口与负载基线见[运行说明](docs/architecture/p0-operations.md)。

## 目录

```text
synquay/
├── Cargo.toml          # Rust workspace
├── backend/            # synquay-server：领域、应用服务、接口、基础设施
├── frontend/           # @synquay/web：应用入口、功能模块、公共代码
├── docs/               # 架构入口与开发约定
└── package.json        # npm workspace 与开发脚本
```

## 本地运行

需要 Rust stable（支持 edition 2024，建议 1.98.1 或更新版本）及 Node.js 20.19+ 或 22.12+。在项目根目录运行：

```sh
npm ci
```

终端一启动后端：

```sh
cargo run -p synquay-server
```

终端二启动前端：

```sh
npm run dev:frontend
```

打开 http://127.0.0.1:5173。管理台暂不需要登录，自动读取配置；MCP 调用仍需授权。前端通过 Vite 的 `/api` 代理访问 http://127.0.0.1:8080，无需开发用的宽松 CORS 设置。

后端 `GET /api/health` 返回 `{"status":"ok"}`；`GET /api/info` 返回服务名、版本和 `scaffold` 阶段。健康检查仅代表当前进程可响应，不代表未来连接器或数据库可用。

默认只监听本机。通过进程环境变量修改地址：

```sh
SYNQUAY_BIND=127.0.0.1:8081 cargo run -p synquay-server
SYNQUAY_API_TARGET=http://127.0.0.1:8081 npm run dev:frontend
```

`RUST_LOG=debug` 可调整日志级别，`SYNQUAY_LOG_FORMAT=json` 输出 JSON 日志。日志写到 stderr；每个请求都有服务端生成的 `x-request-id`，用于关联响应与日志。当前没有 `.env` 自动加载机制，`.env.example` 仅作为配置参考。

```sh
SYNQUAY_LOG_FORMAT=json RUST_LOG=info,synquay_server=debug cargo run -p synquay-server
```

配置错误会在监听端口前终止启动。请求日志包含路由模板、方法、状态码和响应头就绪耗时，不记录请求体、查询参数、Cookie 或 Authorization。当前 tracing 是进程内 span 与结构化日志，尚未接入 OpenTelemetry 或跨服务 trace context。

## 验证与构建

```sh
npm run check
npm run build:frontend
cargo build --release -p synquay-server
```

前端产物位于 `frontend/dist/`，后端位于 `target/release/synquay-server`。设置 `SYNQUAY_WEB_DIR` 为前端产物目录后，后端在同一端口提供管理台与 API。Debian/Ubuntu 安装包包含前后端、独立服务用户和 systemd 服务，构建、安装与 GitHub Actions 用法见 [deb 打包与部署](docs/debian.md)。管理台当前没有登录鉴权，默认仅监听回环地址；QQ 公网回调可以通过反向代理单独开放。

提交 `Cargo.lock` 和 `package-lock.json`；缓存、构建产物、依赖目录和凭据不提交。

## 实现范围

已实现：分层代码框架、类型化配置及校验、文本/JSON tracing 日志、请求 ID 与耗时记录、404/405 统一错误响应、健康与版本接口、优雅退出、前端 API 客户端和连接状态、配置与 API 测试。

已建立连接器抽象：InboundDecoder、EventIngress、EventStore、EventSource、ToolExecutor、MessageSink、ConnectorLifecycle，以及统一上下文、事件/消息/工具类型、能力注册表和连接绑定的事件接收用例。详见 [连接器契约](docs/connectors.md)。

已补齐模块契约与应用编排：身份验证、权限决策、版本化工具发布、Schema 校验、审计、事件路由、Outbox 投递与原路回复，统一通过 BackendModules 注入依赖。详见 [模块抽象与对接](docs/module-integration.md)。

已接通真实消息转发：可选配置的认证 webhook → SQLite EventStore → 路由 → Outbox → HTTP 目标，并持久化审计。启动后每秒扫描待办，支持去重与租约恢复。使用方式、示例配置及状态说明见 [持久化 HTTP 转发](docs/http-relay.md)。

已增加 MCP 工具入口、外部 OAuth 令牌校验及在线暴露发布，见[配置与运行说明](docs/architecture/oauth-publication.md)。Keycloak 本地联调已通过，复现方式见[联调说明](examples/keycloak/README.md)。尚待实现：公网客户端验证、动态权限管理、多渠道驱动与上游 MCP 桥接。P0 已支持受保护的人工核查和重放。P1 已补流式内容、数据库调度/索引、维护备份与来源检查点，见[实现说明](docs/architecture/p1-storage.md)。当前 HTTP 目标使用 Synquay 的消息 DTO，需要对应接收服务；管理台支持多 MCP 服务的创建、编辑、启停、删除及独立工具范围，默认服务保留原能力发布和分页历史接口，见[管理台运行与验收](docs/management-console.md)。

项目名称为 **Synquay**，中文名为 **汇流港**，代码标识统一为 `synquay`。目标域名为 `synquay.com`，尚未注册。开源许可证尚待选择。

逐模块设计见 [核心架构与模块契约](docs/architecture/README.md)，架构来源见 [docs/architecture.md](docs/architecture.md)，代码分层和扩展约定见 [docs/development.md](docs/development.md)。

统一能力、资源与协议绑定说明见 [能力层](docs/architecture/capability-layer.md)。HTTP 工具可使用 `examples/tools-documented.json` 提供 Agent 使用指南。

空间、成员与 MCP 授权管理见 [空间模型与 API](docs/architecture/workspaces.md)。

QQ 指令可调用连接能力并按需回复原会话，配置与执行边界见 [连接路由与指令能力](docs/connection-routing.md#指令调用连接能力)。
