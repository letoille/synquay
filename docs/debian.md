# deb 打包与部署

参考 Kaleye 的统一构建脚本、cargo-deb 和 systemd 方案。Synquay 的入口为 `scripts/build-deb.sh`，本地 Linux 与 GitHub Actions 使用相同流程，资产清单只维护在 `backend/Cargo.toml` 中。

## 安装包内容

| 路径 | 用途 |
| --- | --- |
| `/usr/bin/synquay-server` | Rust 后端，运行时不需要 Node.js |
| `/usr/share/synquay/web/` | React 管理台产物，由后端托管 |
| `/etc/synquay/synquay.env` | systemd 环境配置，dpkg conffile |
| `/etc/synquay/tools.json` | MCP 初始化配置 |
| `/etc/synquay/security.json` | 认证与允许的管理台来源 |
| `/etc/synquay/pipeline.json` | 消息管线初始化配置 |
| `/etc/synquay/credentials.env` | 首次安装生成的独立随机凭据，root 0600 |
| `/var/lib/synquay/` | 数据库、连接密钥加密主密钥和运行数据 |
| `/var/log/synquay/` | 应用日志与轮转归档 |
| `/etc/logrotate.d/synquay` | 日志轮转规则，dpkg conffile |
| `/usr/share/synquay/config/` | 随版本更新的配置模板 |

服务名为 `synquay`，以专用 `synquay` 用户运行，应用日志写入 `/var/log/synquay/synquay.log`；服务启停和 systemd 错误仍由 journal 记录。默认监听 `127.0.0.1:8080`，管理台和 API 同源，不需要单独启动前端开发服务器。

首次安装生成配置与凭据。默认没有工具、消息连接或自动转发路由；`bootstrap_routes: false` 只影响空数据库的首次初始化，不会覆盖已有路由。管线配置中保留的基础 Webhook/HTTP 端点用于兼容现有运行模型，HTTP 地址是不可用的占位地址，没有路由会向它发送消息。请在管理台创建实际连接和路由。

## 本地 Linux 构建

在 Debian/Ubuntu 上准备 Rust stable、Node.js 22、Python 3 与打包工具：

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config dpkg-dev python3 ca-certificates
bash scripts/build-deb.sh
```

脚本执行 `npm ci`、前端测试和构建、`cargo build --release --locked`、cargo-deb、包内容检查与 SHA256 生成。cargo-deb 固定为 3.6.0。必须提交 `Cargo.lock` 和 `package-lock.json`。

产物为 `target/debian/synquay_<版本>-1_<架构>.deb` 与 `SHA256SUMS`。预发布版本如 `0.2.0-rc.1` 转换为 Debian 版本 `0.2.0~rc.1-1`，保证排序早于正式版。默认原生构建，脚本拒绝将 macOS 二进制打成 Linux 包。

macOS 可使用 Docker 构建（首次下载与编译耗时较长）：

```bash
docker build -f scripts/Dockerfile.deb --output type=local,dest=./dist/debian .
```

Docker 按宿主机架构构建，Apple Silicon 得到 arm64 包；需要 amd64 时加 `--platform linux/amd64`，模拟构建会较慢。Dockerfile 只用于打包，安装后的服务不依赖 Docker。构建上下文排除本地数据、凭据、依赖与已有产物。

## 安装和查看

```bash
sha256sum --check SHA256SUMS
sudo apt install ./synquay_*.deb
systemctl status synquay
sudo tail -F /var/log/synquay/synquay.log
```

安装会启用并启动服务。可通过 SSH 转发访问服务器上的管理台：

```bash
ssh -L 8080:127.0.0.1:8080 user@server
```

然后打开 `http://127.0.0.1:8080/`。修改 `/etc/synquay` 中的配置后运行 `sudo systemctl restart synquay`。

管理台尚无登录鉴权，不应直接公开整个监听端口。接入 QQ 时，由 HTTPS 反向代理仅转发 `/api/qq/<连接ID>/webhook` 到 `127.0.0.1:8080`，将公网地址填入 QQ 开放平台。MCP 公网路径应按各服务认证配置单独开放。

## 日志与归档

安装包通过 systemd 将应用 stdout/stderr 追加到 `/var/log/synquay/synquay.log`，默认 JSON 格式。目录权限 `0750`、当前文件 `0640`，归属 `synquay:synquay`；重复安装和服务重启不会清空日志。开发时直接运行程序仍输出到 stderr。

```bash
sudo tail -F /var/log/synquay/synquay.log
sudo ls -lh /var/log/synquay/
sudo zcat /var/log/synquay/synquay.log.2.gz
# 服务启停或程序尚未启动的错误
sudo journalctl -u synquay -n 100 --no-pager
```

默认由系统 logrotate 的 timer/cron 调度：每天轮转非空日志，保留 14 份归档，最近一份为 `.1`，更早的为 `.2.gz` 至 `.14.gz`。检查时若超过 50 MiB，也会提前轮转；这个阈值只在 logrotate 执行时检查，不是实时磁盘上限。可在 `/etc/logrotate.d/synquay` 调整策略；修改不需要重启应用，升级按 conffile 规则保留。

```bash
systemctl status logrotate.timer
sudo logrotate --debug /etc/logrotate.d/synquay
```

文件由 systemd 持有，使用 `copytruncate` 保持写入句柄有效，轮转无需重启服务。复制与截断之间有短暂窗口，极少量并发日志可能丢失，不能将这些运行日志视为无损审计归档；业务审计仍在数据库中。详见 [logrotate 文档](https://github.com/logrotate/logrotate/blob/main/logrotate.8.in)。

升级后新日志进入文件，历史 journal 不会自动迁移。卸载与 purge 保留 `/var/log/synquay`；purge 会移除包管理的轮转配置，保留日志需自行管理。此次配置修改在安装新版 deb 后生效。

## Nginx 80 端口配置

提供 [Nginx 配置示例](../backend/deploy/nginx/synquay.conf.example)，域名为 `synquary.p.kalandraeye.com`，代理到安装包默认的 `127.0.0.1:8080`。管理台、API 和 MCP 共用该入口；关闭代理缓冲以支持 MCP 流式响应。配置仅监听 80，不包含证书和 HTTPS 跳转，后续由 Certbot 生成。

在 Debian/Ubuntu 的 Nginx 上，从仓库目录执行：

```bash
sudo install -m 644 backend/deploy/nginx/synquay.conf.example /etc/nginx/sites-available/synquay
sudo ln -s /etc/nginx/sites-available/synquay /etc/nginx/sites-enabled/synquay
sudo nginx -t && sudo systemctl reload nginx
```

启用前确认 DNS 指向服务器、80 端口可访问且 Synquay 已启动。已有同名配置或符号链接时先检查，不重复覆盖。配置使用的 [反向代理指令](https://nginx.org/en/docs/http/ngx_http_proxy_module.html) 保留原始 URI、Host 与请求的认证头。

使用 `8443` 等非标准公网端口时，代理必须使用 `proxy_set_header Host $http_host;` 保留浏览器请求中的端口；`$host` 会丢掉端口，使后端收到的 Host 与浏览器 Origin 不一致，保存时返回 `origin_denied`。如果已经用 Certbot 生成 HTTPS 配置，也需要修改实际处理 HTTPS 请求的 `location`，然后执行 `sudo nginx -t && sudo systemctl reload nginx`。无需修改后端的来源校验。

此示例会代理整个管理台，当前管理台没有登录认证；对公网使用时需另外限制管理台和管理 API 的访问。HTTPS 本身不提供管理权限控制。前端 API 请求使用 `credentials: 'omit'`，因此不能直接依赖浏览器自动附带的 Nginx Basic Auth 凭据。

## 升级、备份与卸载

```bash
sudo apt install ./synquay_<新版本>-1_amd64.deb
```

升级后自动重启服务。生成的 JSON 配置与凭据不会被安装脚本替换，`synquay.env` 由 dpkg 的 conffile 机制保护，交互升级遇到提示时保留本地配置。无人值守升级可使用：

```bash
sudo apt -o Dpkg::Options::="--force-confdef" -o Dpkg::Options::="--force-confold" install ./synquay_*.deb
```

新模板始终在 `/usr/share/synquay/`，可以与 `/etc/synquay` 对比后按需合并。升级前备份 `/etc/synquay` 和 `/var/lib/synquay`，尤其是数据库旁的 `.connection-secrets.key`；丢失该主密钥将无法解密连接凭据。复制 SQLite 文件前停止服务，或使用仓库的备份工具。

`apt remove synquay` 停止服务并移除程序，保留配置和数据。`apt purge synquay` 还移除包管理的 `synquay.env`，但有意保留生成的 JSON、随机凭据、数据库、加密主密钥与专用用户，避免误删业务数据；彻底清理需管理员确认备份后单独操作。

## GitHub Actions

工作流：`.github/workflows/deb.yml`（Debian package）。

- 手动运行默认只构建、验证并上传 Artifact，不发布 Release。
- 勾选 `publish`：验证成功后发布 `v<版本>` 的 GitHub Release。
- 推送 `v*` 标签：标签必须与根 `Cargo.toml` 的 workspace 版本完全一致，成功后发布 Release。
- 发布包含 `.deb` 与 SHA256；预发布版本自动标记 prerelease，也可手动勾选。
- 已有 Release 保留其页面，覆盖同名 `.deb` 和 `SHA256SUMS`，更新标题、说明和预发布状态；其他附件保留。发布 Job 才有 `contents: write` 权限，同版本发布串行执行。
- 同一提交重跑可直接覆盖。若标签指向其他提交，默认拒绝；从分支手动运行并勾选 `publish` 和 `replace_tag`，可将版本标签移动到本次提交并覆盖发布。标签触发或从标签手动运行不允许移动标签。已开启不可变发布或标签保护时可能无法覆盖，需要新版本或调整仓库规则。
- 附件覆盖不是原子操作；上传中断可能暂时缺少附件或校验文件与包不匹配，需重新运行发布完成覆盖。同版本替换的包不会因版本号不变而被 apt 自动判定为升级，可重新下载后使用 `sudo apt install --reinstall ./synquay_*.deb` 安装。
- 修改工作流后需提交并推送，从更新后的分支新建一次运行；旧运行的 Re-run 会继续使用旧提交中的工作流。
- 当前 CI 使用 Ubuntu 22.04 原生 amd64，面向 Ubuntu 22.04+ / Debian 12+；动态库最低版本由 `$auto` 依赖分析写入包，不假定可安装到更老发行版。
- Rust 缓存只在默认分支写入；建议第一次在默认分支手动构建，后续标签构建可复用缓存。

CI 检查 Rust 格式、Clippy、测试、初始化幂等性、前端测试与构建、真实包内容，再在独立 runner 安装、重装、卸载、purge，验证服务用户、HTTP 可用性、日志写入/轮转/压缩以及配置/数据保留。`scripts/test-deb.sh` 仅供隔离测试环境，检测到既有安装时拒绝运行。

配置工作流不会自动创建 GitHub 仓库或推送代码，需要把项目提交到 GitHub 后才能触发。
