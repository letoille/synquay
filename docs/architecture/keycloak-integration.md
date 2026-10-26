# Keycloak 本地授权联调

2026-10-24。Synquay 已通过真实 Keycloak 26.8.1 的本地 OAuth 联调：用户登录与同意、授权码 + S256 PKCE、令牌刷新、MCP 工具发现/调用，以及撤销后的拒绝访问。此结果覆盖资源服务器与授权服务的协议链路；公网部署、ChatGPT 实际连接和管理台登录按用户决定留到下一步。

## 选择与边界

首个参考授权服务选用自托管 Keycloak，固定版本为 26.8.1；Synquay 保留通用 introspection 接口，不依赖 Keycloak 专有令牌格式。用户、OAuth 客户端和同意记录由 Keycloak 管理，Synquay 保存内部身份映射、工具权限和业务审计。

本地运行使用隔离的 H2 数据库和 start-dev，只绑定随机回环端口。每次创建开发 realm、测试用户、精确回调地址和独立随机密钥；退出时停止服务并删除临时数据。发行包缓存保留供后续回归，不修改系统 Java。生产环境需独立确定 TLS、数据库、持久化和密钥管理。

## 实测发现与修正

Keycloak 开启 resource-indicators 后，会将令牌 audience 绑定到请求的 MCP URL；其 introspection 同时检查认证客户端 ID 是否在 audience 内。原本使用短名称作为资源客户端 ID，导致真实令牌被拒绝校验。

解决方式是让资源客户端 ID、resource_url 与 Synquay resource 同为完整 MCP URL。Agent 客户端保持独立的 synquay-agent，Audience mapper 指向资源客户端。没有关闭受众检查，也没有把错误令牌转换为内部可信身份。

Synquay 原先额外禁止 introspection_client_id 包含冒号，阻止了 URL 类型的客户端标识。本次移除该限制，保留长度和可见字符检查；认证时仍先按表单规则编码客户端 ID 和密钥，再组合 Basic 认证。Rust 回归测试验证 URL 客户端标识及包含加号、冒号的密钥。

错误 resource 可能在授权阶段获得授权码，但在兑换令牌时返回 invalid_target，因此测试以最终令牌拒绝为判据。Keycloak 对本地安全上下文使用 Secure Cookie；测试脚本仅为限定的回环请求适配 Python CookieJar，不改变生产 HTTPS 要求。

Keycloak 的 resource-indicators 仍为实验特性，版本升级必须重新验证。配置依据见[Keycloak MCP 指南](https://www.keycloak.org/securing-apps/mcp-authz-server)及[Keycloak 版本发行说明](https://github.com/keycloak/keycloak/releases/tag/26.8.1)。

## 可复现工具

| 文件 | 用途 |
|---|---|
| examples/keycloak/runtime-lock.json | Keycloak 与 macOS arm64 Java 的官方地址、版本、SHA-256 |
| scripts/fetch_oauth_runtime.py | 验证摘要后解包，拒绝覆盖已有目录；其他系统可跳过 Java 下载 |
| scripts/keycloak_realm.py | 为单次联调生成私有 realm、凭据和 Synquay 配置 |
| scripts/smoke_keycloak.py | 启动真实授权服务与 Synquay，执行正反向协议测试并清理 |
| examples/keycloak/README.md | 安装、运行、身份绑定和公网迁移说明 |

在项目目录执行，macOS Apple Silicon 示例：

```sh
python3 scripts/fetch_oauth_runtime.py --directory target/oauth-runtime
cargo build --locked --examples --bin synquay-server
python3 scripts/smoke_keycloak.py \
  --keycloak-home target/oauth-runtime/keycloak-26.8.1 \
  --java-home target/oauth-runtime/jdk-21.0.13+9/Contents/Home
```

其他系统提供 Java 21+ 并使用下载器的 --skip-java。需要 Python 3.12+。生成的测试身份不使用现有账号，令牌不输出到控制台或写入仓库。

## 已验证结果

| 路径 | 结果 |
|---|---|
| 发现与回调 | issuer 一致、支持 S256、回调 state/iss 核验通过 |
| 登录和用户同意 | 真实登录/同意表单完成授权码流程，未使用 password grant |
| 错误 PKCE | 缺少 challenge、错误 verifier 被拒绝 |
| 授权码与资源绑定 | 同一授权码不能再次兑换；未知 resource 兑换失败 |
| 身份与权限 | introspection 的 issuer、audience、sub、client_id 和 scope 满足 Synquay 契约 |
| MCP 调用 | 官方 Rust SDK 列出 echo__r1 并调用成功，上游不收到 Agent 令牌 |
| 刷新与撤销 | 刷新后可调用；撤销刷新令牌后，继续刷新失败，MCP 拒绝旧访问令牌并返回 401 |
| 回归 | 90 项 Rust 测试通过、1 项手工微基准默认忽略；clippy 无警告 |

脚本按 HTTP 表单协议模拟本地登录，没有检验浏览器渲染、跨站 Cookie 或真实 ChatGPT UI。授权服务运行日志和 Synquay 日志检查未包含本次生成的密码、密钥或令牌。

## 下一阶段

下一步可先开发最小管理台，或另行安排公网联调。公网联调需确定 Synquay 与授权服务的 HTTPS 地址，并给 ChatGPT 注册独立 OAuth 客户端，按管理页提供的精确回调地址配置，再绑定对应主体和工具权限。官方支持预定义客户端，授权码、PKCE、resource 和 issuer 验证需一起检查，见[OpenAI 授权文档](https://developers.openai.com/plugins/build/auth)。

当前未部署公网服务，也未宣称 ChatGPT 实际连接通过。管理 API 仍使用独立 bootstrap Bearer，管理台登录和动态 grant 仓储尚未完成。
