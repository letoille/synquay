# Keycloak 本地 OAuth 联调

使用真实 Keycloak 26.8.1、临时测试用户和 Synquay，执行授权码 + PKCE、刷新、撤销及 MCP 调用。所有服务只监听回环地址，测试结束停止进程并清理临时数据库和凭据。此目录没有生产部署配置。

## 准备与运行

需要 Python 3.12+、Java 21+ 和编译后的 Rust 服务及示例客户端。官方发行包版本及 SHA-256 固定在 [runtime-lock.json](runtime-lock.json)。

macOS Apple Silicon 可在项目目录执行：

```sh
python3 scripts/fetch_oauth_runtime.py --directory target/oauth-runtime
cargo build --locked --examples --bin synquay-server
python3 scripts/smoke_keycloak.py \
  --keycloak-home target/oauth-runtime/keycloak-26.8.1 \
  --java-home target/oauth-runtime/jdk-21.0.13+9/Contents/Home
```

其他系统使用 `fetch_oauth_runtime.py --skip-java` 下载 Keycloak，并通过 `--java-home` 指定当地已有的 Java 21+。下载器验证摘要后解包，拒绝覆盖已有运行目录；它不安装系统 Java，也不修改 shell 配置。下载文件总计约 374 MB，解压需要额外空间。首次启动包含 Keycloak 构建与数据库初始化，通常比后续 HTTP 测试耗时。

测试需要能绑定本机端口。运行期间会启动一个 Keycloak JVM、一个 Synquay 进程和一个 HTTP echo 服务，没有公网监听；不需要用户输入现有账号密码，也不调用模型 API。

## 配置关系

| 对象 | 本地配置 | 用途 |
|---|---|---|
| Realm | synquay-lab | 每次测试在独立数据库中导入 |
| OAuth 客户端 | synquay-agent | 机密客户端，授权码 + S256 PKCE，精确回调地址和用户同意 |
| 资源客户端 | 本次 MCP 完整 URL | client_id 与 audience 一致；使用独立 introspection 密钥及 resource_url |
| Scope | synquay:tools | 添加资源客户端 audience 与 subject claim |
| Synquay grant | 生成的 sub + synquay-agent | 映射内部主体/客户端 UUID，工具白名单为 echo |
| 管理员 | 独立生成的主体与 Bearer | 不与授权服务或 Agent 密钥复用 |

`keycloak_realm.py` 生成私有配置，顶层临时目录权限为 0700，文件为 0600。访问令牌与刷新令牌仅驻留在测试进程内存及子进程环境中，不输出或保存到仓库。测试身份只属于新生成的开发 realm。

资源客户端 ID 使用 MCP 完整 URL，使资源指示符生成的 URL audience 同时满足 Keycloak introspection 的客户端受众检查。Synquay 将含冒号/斜杠的客户端 ID 按 OAuth 表单规则编码后用于 Basic 认证，不关闭 Keycloak 的受众校验。

启用 Keycloak `resource-indicators`，使用 `resource_url` 和 Audience mapper 将授权与令牌请求绑定到目标 MCP 地址。该功能仍为 Keycloak 实验特性，需固定版本并在升级时重新运行联调。普通登录与 OAuth 客户端配置本身不能替代受众绑定验证。[Keycloak MCP 指南](https://www.keycloak.org/securing-apps/mcp-authz-server)

## 测试路径

脚本读取真实 OIDC 发现文档，按登录和同意页面的表单提交临时测试身份，验证 state 和回调 issuer，然后兑换授权码。它不会用 password grant 代替授权码流程；realm 禁用了 password grant、implicit 和 service account grant。

正向检查覆盖资源发现、登录与用户同意、PKCE、令牌 introspection 的身份和受众、官方 Rust MCP SDK 工具目录与调用、刷新后的调用，以及撤销后拒绝访问。反向检查覆盖缺少 PKCE、错误 verifier、授权码复用和错误 resource。

这是 HTTP 协议层的登录流程测试，没有验证浏览器布局、跨站 Cookie 或 ChatGPT UI。Keycloak 将回环地址视为安全上下文并设置 Secure Cookie；脚本为受限回环请求显式适配 Python CookieJar，生产服务仍要求 HTTPS。

## 后续公网联调

需先确定公开 HTTPS 的 Synquay resource URL 和 Keycloak issuer，配置 TLS、生产数据库及独立持久化。授权服务元数据、令牌 issuer 与 Synquay 配置必须完全一致。生产环境使用 `start`，不沿用本测试的 `start-dev`、H2 和自动生成的测试用户。

ChatGPT 可使用预定义 OAuth 客户端。后续注册独立客户端，把管理页面显示的精确回调 URI 加入允许列表，再将对应 sub/client_id 映射到 Synquay grant。官方要求授权码和 PKCE，并让 resource 参与受众绑定；回调地址按实际管理页面为准。[OpenAI 授权文档](https://developers.openai.com/plugins/build/auth)

本地脚本通过不等于 ChatGPT 实际连接已通过；公网部署与 ChatGPT 账号联调在下一阶段完成。
