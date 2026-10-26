# 代码分层与开发约定

## 当前目录

```text
backend/
  src/
    main.rs                     启动配置、日志、监听与退出
    lib.rs                      依赖装配与 AppState
    config.rs                   类型化配置、默认值和启动校验
    domain/                     系统、连接器、身份、发布、事件、投递与审计模型
    application/system.rs       应用用例与业务编排
    application/ports/          连接器与业务依赖 trait、上下文及错误约定
    application/connectors/     能力注册表与事件接收用例
    application/services/       认证、工具调用、路由、投递、回复与统一治理
    application/modules.rs      业务模块装配入口，依赖显式注入
    interfaces/http/
      mod.rs                    路由组装
      handlers.rs               请求提取、应用调用、响应转换
      dto.rs                    HTTP 输入输出模型
      error.rs                  对外错误格式与状态码
      middleware.rs             请求上下文、tracing 与耗时
    infrastructure/
      telemetry.rs              日志 subscriber 与输出格式
      shutdown.rs               进程退出信号
      storage/                  SQLite 连接/迁移、事件、Outbox、审计，按职责拆文件
      connectors/http_sink.rs   固定目标 HTTP 投递
      pipeline/                 配置加载、静态授权/路由绑定、运行装配和 worker
  tests/http.rs                 真实 Router 的 API 合约测试
  tests/connectors.rs           连接器契约与事件接收测试
  tests/module_flows.rs         跨模块授权、路由、投递和回复集成测试
  tests/sqlite.rs               持久化事务、竞争与恢复
  tests/http_pipeline.rs        真实 HTTP 转发与重启链路
frontend/src/
  main.tsx                      React 挂载
  app/App.tsx                   应用布局与功能组装
  features/service-status/
    api.ts                      功能响应校验与映射
    useServiceStatus.ts         请求生命周期、超时与重试状态
    ServiceStatus.tsx           功能 UI
    index.ts                    对外导出
  shared/
    api/client.ts               通用 HTTP 客户端与 ApiError
    styles/global.css           全局样式
```

后端调用方向为 `interfaces → application → domain`。`main.rs` 和 `lib.rs` 负责装配；基础设施实现应用层定义的外部能力接口，领域层不导入 Axum、数据库驱动或连接器 SDK。HTTP 入口包含 system.info 和可选 webhook；静态 Pipeline 组装路由/投递服务及真实存储/HTTP 驱动，完整业务模块仍可通过 BackendModules 注入。

前端依赖方向为 `app → features → shared`。功能状态、响应校验和组件留在对应 feature；其他功能通过其 `index.ts` 使用公共导出，不直接访问内部实现。shared 不反向导入 features 或 app。

## 逻辑块的职责

- handler 处理协议层问题，调用应用服务，不直接执行命令、查询数据库或发送消息。
- application 组织一个用例的校验、权限决策、领域操作与外部调用；长流程拆成明确步骤。
- domain 保存业务类型、状态转换与不变量，不包含协议 DTO。
- infrastructure 封装数据库、队列、连接器、凭据存储等具体实现；在需要时再引入 trait 和独立 crate。
- config 只读取并校验配置，不创建连接或产生其他副作用。
- 每个功能的公开 API 尽量小；先按职责拆模块，避免把所有代码堆进 main.rs、handlers.rs 或 utils.rs，也不为未实现能力创建空包。

## 业务边界

以下模块已建立契约与应用服务。SQLite 存储、静态授权规则与 HTTP 转发已接入；OAuth、其他协议驱动与动态管理设施待实现。详见 [连接器抽象与接入契约](connectors.md) 及 [模块抽象与对接](module-integration.md)。

| 模块 | 职责 | 边界 |
|---|---|---|
| connectors | 连接定义、发现、工具执行、事件采集 | HTTP、CLI、SDK、MQTT 和上游 MCP 的具体实现位于基础设施 |
| capabilities / profiles | 能力目录、筛选、版本化发布 | MCP/HTTP 是协议入口，不能绕过发布与授权规则 |
| events | Event Envelope、接入、去重、订阅 | 不直接调用通知平台，交给投递用例 |
| delivery | 路由、Outbox、重试、死信和原路回复 | 使用持久化状态；每次外部投递有独立结果 |
| identity / policy | 身份、OAuth、权限决策、凭据边界 | 权限校验位于应用用例边界，不能只在 UI 隐藏功能 |
| audit | 安全及业务操作审计 | 审计记录和普通运行日志独立，不能依赖 RUST_LOG |

## Trace 与日志

采用 `tracing` 发布事件和 span，`tracing-subscriber` 仅在启动入口初始化。使用 `#[tracing::instrument(skip_all)]` 或显式 span，并只记录经过选择的非敏感字段，禁止默认 Debug 输出凭据、命令参数、消息正文或完整上下文。

| 配置 | 默认值 | 作用 |
|---|---|---|
| SYNQUAY_BIND | 127.0.0.1:8080 | 后端监听 IP 和端口 |
| SYNQUAY_LOG_FORMAT | text | text 或 json，均输出到 stderr |
| RUST_LOG | info | tracing 过滤器，例如 info,synquay_server=debug |
| SYNQUAY_API_TARGET | http://127.0.0.1:8080 | Vite 开发代理的后端地址 |

每次 HTTP 请求生成新的 UUID 并在响应 `x-request-id` 中返回，不信任调用方传入的同名头。请求 span 带 request_id、method、route；route 使用匹配到的路由模板，未匹配时记为 `<unmatched>`，不记录原始 URL、路径参数或查询参数。完成事件带 status、duration_ms；成功记 INFO，4xx 记 WARN，5xx 记 ERROR。耗时截至响应头就绪，不代表未来 SSE/流式响应的完整传输时间。

应用层的 span 继承当前请求上下文。未来启动后台任务时，应使用 `.instrument(...)` 显式关联任务 span，并记录独立 job_id/event_id；异步代码避免持有 `span.enter()` guard 跨越 await。

request_id 是本服务的关联编号，不是 W3C trace_id。OpenTelemetry 导出、跨进程上下文传播、指标和日志轮转尚未实现。生产日志留存、轮转和采集由运行环境配置。

## API 错误与验证

当前 404 和 405 采用以下结构，request_id 与响应头保持一致：

```json
{
  "error": {
    "code": "not_found",
    "message": "The requested resource was not found.",
    "request_id": "<server-generated-uuid>"
  }
}
```

后续新增应用错误、JSON 提取失败和权限错误时，继续在 HTTP 边界统一映射；不要将内部错误栈、SQL 或凭据放进公开 message。webhook 写接口已统一映射认证、JSON 校验、请求体限制和存储错误。前端统一保留 HTTP 状态和 request_id，功能模块负责解释响应数据和提供重试交互。

每次改动运行 `npm run check`；涉及前端构建再运行 `npm run build:frontend`。测试重点是配置失败、权限边界、错误语义、状态变化和跨模块契约，避免仅复制实现细节。
