# Webhook 与 MCP 内容能力

本轮扩展保留现有 HTTP 工具链路和管理台。新增普通 JSON webhook、HMAC 签名、事件内容去重，以及 MCP resources、resource templates、prompts 和 completion。配置由服务端文件管理；尚未提供这些新增配置的管理台编辑器。

## 已实现的边界

| 能力 | 当前状态 |
|---|---|
| Webhook 接收 | `POST /api/webhooks/{source_connection_id}`；标准信封或普通 JSON |
| Webhook 认证 | 单一 Bearer 或 HMAC-SHA256；模式由配置决定，无自动回退 |
| 事件可靠性 | 落库后返回 202；持久化去重、路由任务、投递队列、背压沿用现有链路 |
| MCP tools | 已有工具目录、Schema、执行权限、调用幂等账本和审计 |
| MCP resources | 列表、读取；经过审核的静态文本、Markdown、JSON |
| MCP resource templates | 单参数、有限枚举模板；只读取已配置条目 |
| MCP prompts | 列表、获取；字符串参数、必填/选项校验、单次模板替换 |
| MCP completion | 提示词选项和资源模板参数前缀补全，最多返回 50 项，包含总数 |
| 传输与认证 | rmcp 3.7.0 Streamable HTTP；已有静态 Bearer / OAuth introspection |
| 上游 MCP 桥接、stdio | 尚未实现 |
| 资源订阅、目录变化推送、事件推送 | 尚未实现，也不声明 subscribe/listChanged 能力 |
| Sampling、Elicitation、Tasks、MRTR、logging/setLevel | 尚未实现 |
| 图片/音频/二进制资源 | 尚未实现；当前资源提供器为静态内容 |
| Agent 收件箱 | 已通过业务 tools 实现，见 [收件箱说明](agent-inbox.md) |

协议端到端测试使用官方 Rust SDK 客户端，覆盖 2025-11-25 initialize 和 2026-07-28 discover。尚未完成 ChatGPT 公网客户端联调。收到 webhook 不会因此自动唤醒 ChatGPT；现已补上 [Inbox 与来源适配器回复闭环](agent-inbox.md)；持续自动分析仍需要独立 Agent Worker。

## Webhook 配置与签名

原有 `examples/pipeline.example.json` 保持兼容：省略 `webhook` 等同 Bearer + envelope。启用签名和普通 JSON 的完整示例为 `examples/pipeline-signed-json.json`：

```json
{
  "webhook_token_env": "SYNQUAY_WEBHOOK_SECRET",
  "webhook": {
    "authentication": "hmac_sha256",
    "format": {
      "kind": "json",
      "event_type": "chat.message",
      "id_header": "x-delivery-id"
    }
  }
}
```

`webhook_token_env` 指向环境变量，HMAC 模式下其值为共享密钥。沿用凭据约束：32–4096 个可见 ASCII 字符；配置和日志不保存密钥值。建议用 `openssl rand -hex 32` 生成。示例的 HTTP 目标是回环地址，需先运行 `python3 examples/http_receiver.py`（默认 9091）。随后在项目根目录启动：

```sh
export SYNQUAY_WEBHOOK_SECRET="$(openssl rand -hex 32)"
export SYNQUAY_PIPELINE_CONFIG=examples/pipeline-signed-json.json
cargo run -p synquay-server
```

发送方使用相同秘密，每次发送生成当前 Unix 秒时间戳，按 UTF-8 编码拼接以下前缀，再追加**实际发送的原始请求体字节**：

```text
v1\n{source_connection_uuid}\n{timestamp}\n{delivery_id}\n{raw_body}
```

上面的 `\n` 表示单个换行字节；正文之后不追加换行。`source_connection_uuid` 必须与路由中规范的小写 UUID 相同。普通 JSON 的 delivery_id 来自配置的请求头；envelope 模式该项为空字符串，其 source_event_id 位于签名正文内。计算 HMAC-SHA256，输出小写十六进制；传入 `X-Synquay-Signature: v1=<64位hex>` 和 `X-Synquay-Timestamp: <Unix秒>`。

最小发送示例（在有相同环境变量的终端运行）：

```python
import hashlib, hmac, json, os, time, urllib.request
source = "00000000-0000-0000-0000-000000000002"
delivery = "example-message-1"  # 同一消息重试保留此 ID；新消息换 ID
stamp = str(int(time.time()))
body = json.dumps({"text": "你好，Synquay"}, ensure_ascii=False).encode()
prefix = f"v1\n{source}\n{stamp}\n{delivery}\n".encode()
signature = hmac.new(os.environ["SYNQUAY_WEBHOOK_SECRET"].encode(), prefix + body, hashlib.sha256).hexdigest()
request = urllib.request.Request(
    f"http://127.0.0.1:8080/api/webhooks/{source}", data=body,
    headers={"Content-Type": "application/json", "X-Delivery-ID": delivery,
             "X-Synquay-Timestamp": stamp, "X-Synquay-Signature": "v1=" + signature},
)
with urllib.request.urlopen(request, timeout=10) as response:
    print(response.status, response.read().decode())
```

这是 Synquay 自定义签名协议，不直接兼容 GitHub、微信等平台各自的签名格式；外部平台需要适配器转换或单独实现对应认证插件。

请求限制：正文最多 64 KiB、读取最多 5 秒、时间戳与服务端相差不超过 300 秒。只接受一个 application/json Content-Type；不接受 Content-Encoding，重复认证/签名/事件编号头被拒绝。浏览器 Origin 请求被拒绝。验签使用原始字节和常量时间比较，验签成功后才解析正文。大内容继续走已有内容上传与 ContentRef 路径；该上传 API 的 Bearer 认证独立于 webhook 的 HMAC 模式。

envelope 格式：

```json
{"source_event_id":"message-1","event_type":"chat.message","payload":{"text":"hello"}}
```

普通 JSON 可直接发送业务正文；事件类型由可信配置指定，工作区、来源连接和执行主体由管道配置绑定，正文中的同名字段不会改变权限范围。当前 Pipeline 仍使用固定来源/服务主体策略，尚未接入 MCP 的动态 WorkspacePolicy 成员撤权链路。

同一工作区和来源内：同 ID、同业务内容重试返回原 event_id / duplicate；同 ID、不同类型或内容返回 409。内容指纹不包含服务端生成的接收时间、trace ID 等字段；JSON 对象键顺序和空白不影响去重，数组顺序及值变化会影响。202 表示已持久化接收，不等于外部目标已经收到。503 可按 Retry-After 退避，重试保留事件 ID；HMAC 重试超过时间窗口时要重新签名。队列满不无限积压在内存。

## MCP 内容配置与权限

使用 `examples/tools-content.json` 代替 tools.json 作为 `SYNQUAY_TOOLS_CONFIG`，沿用已有 `SYNQUAY_MCP_TOKEN` 和 `/mcp` 入口。示例含 HTTP echo 工具、使用说明资源、heater 示例说明模板和 plan-action 提示词；示例不会控制真实设备。echo 依赖 `python3 scripts/demo_tool.py`。

配置新增 `mcp_content`，包含独立的 `revision`、`resources`、`templates`、`prompts`。每项默认 `exposed=false`，显式设为 true 才能公开。静态 Bearer 只访问该主体在当前空间被允许的已暴露内容。OAuth grant 还须明确列出资源/模板 ID 与提示词 ID：

```json
{
  "tools": ["echo"],
  "resources": ["usage-guide", "device-guide"],
  "prompts": ["plan-action"]
}
```

此片段添加到已有 oauth.grants 条目；subject、client、principal 等原有必填字段仍需保留。resources/prompts 省略时视为空授权，不因为 tools 授权而自动开放。有效权限取公开范围、OAuth grant、当前工作区状态和成员权限的交集；Viewer 可读取被授予的内容，但不会因此获得其他工具权限。隐藏项无法绕过目录直接读取，补全同样经过授权。

公开引用形式：

- 资源：`synquay://{workspace}/{profile}/c1/resources/usage-guide`
- 模板：`synquay://{workspace}/{profile}/c1/templates/device-guide/{device}`，读取时将 `{device}` 替换为 `heater`
- 提示词：`plan-action__c1`

内容版本与工具 revision 独立。首次非空目录至少为 1，内容修改后增加 mcp_content.revision 并重启；同版本同内容允许重复启动，同版本不同内容及回退版本拒绝。只改内容时无需改工具 revision。移除全部内容也需使用更高内容版本，不能直接省略字段回到 0。旧 URI 和提示词名拒绝，客户端重新发现即可。内容当前为启动时文件发布，不与在线工具发布组成一个原子发布事务。

实现边界按层划分：

| 层 | 位置 / 职责 |
|---|---|
| 领域模型 | `domain/mcp_content.rs`，内容、模板、提示词及发布约束，无 rmcp 类型 |
| 端口 | `application/ports/mcp_content.rs`，McpContentProvider 的 catalog/read/render |
| 应用服务 | `application/services/mcp_content.rs`，命名空间、当前权限、暴露范围和审计 |
| 提供器 | `infrastructure/mcp_content.rs`，静态 Arc 内容复用、精确条目读取、有限参数替换 |
| 持久化 | `infrastructure/storage/mcp_catalog.rs`，不可变目录版本与启动主体记录 |
| 协议 | `interfaces/http/mcp.rs`，rmcp DTO 与业务服务映射 |

资源读取和提示词渲染在返回内容前再次检查权限，成功审计必须落库；失败不返回内容。读取结果在支持该元数据的协议版本声明 private、TTL=0。模板 URI 不访问任意文件/URL；提示词参数作为字符串只替换一次，不执行表达式。提示词内容只返回客户端，不直接触发模型或工具调用。

目录编码总量最多 64 KiB；资源与模板合计最多 32 个，提示词最多 32 个。单篇内容最多 32 KiB，JSON MIME 内容必须为有效 JSON。模板最多 100 个枚举项；提示词最多 8 个参数、8 条消息，每个实参最多 4096 字节，渲染输出合计最多 64 KiB。列表一次返回全部有权查看的条目，无分页 cursor；补全有 50 项上限。尚未增加本轮 MCP 吞吐基准，不能把有界设计当成性能实测结论。

## 数据库与验收

SQLite 迁移到 v10，在原迁移事务中增加 events.fingerprint 与 mcp_catalog_releases。事件查重复用 `(workspace, source, source_event_id)` 唯一索引；目录最新版本复用 `(workspace, profile, revision)` 主键反向扫描，未增加重复索引。测试包含查询计划核验与迁移失败回滚。

旧事件的指纹按首次重复读取懒回填；已在升级前清除正文且没有指纹的旧事件无法确认内容一致，返回冲突，不当作成功重复请求。新指纹在正文清理后仍保留。备份和检查脚本已识别 v10；目录历史与审计仍需运维规划容量和保留策略。

新增测试 `webhook_protocol.rs` 与 `mcp_content.rs` 覆盖正文/ID 篡改、签名时效、重复头、重启去重、同 ID 冲突、迁移回滚、官方客户端读资源/提示词/补全、隐藏与跨版本拒绝、OAuth 最小授权、成员撤销、审计失败关闭。完整回归 123 项通过，1 项手工基准默认忽略；Clippy（warnings as errors）和格式检查通过。验证命令：

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all --check
```
