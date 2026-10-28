# 连接专属路由

连接保存账户、协议、凭据和收发能力；路由保存接收范围与发送目标。公共路由流程依次执行：来源匹配 → 指令映射/处理 → 目标解析 → 持久化投递。协议细节由连接适配器实现。

## 接口

`application::ports::connection_routing::ConnectionRoutingAdapter` 包含：

- `source_descriptor` / `target_descriptor`：声明管理台可用模式与字段。
- `validate_source` / `validate_target`：保存时校验协议、版本、方向、范围及回复限制；修改连接时重新校验引用它的路由。
- `matches`：使用接入适配器提供的来源信息匹配。
- `resolve_target`：在消息入投递队列前解析具体目标。
- `discovery_account`：可选发现目录的上游账户命名空间。

`RouteBinding { adapter, version, config }` 是版本化扩展配置。公共层不解释协议 JSON，具体适配器使用带 `deny_unknown_fields` 的类型反序列化；未知版本、字段和适配器不兼容均拒绝。添加协议时实现此接口，在 `connectors/routing/mod.rs` 注册；常规文本、单选/多选会话字段由通用前端渲染器展示。新控件类型需要显式加入前端白名单。

`ManagedRoute.routing.source` 保存入口选择，`routing.targets[destination_id]` 保存每个出口的配置。省略整个 `routing` 的旧客户端保存请求保留原配置；显式 `{source:null,targets:{}}` 清除配置，仍须通过协议校验。切换来源或删除出口时，界面会清理对应配置。

## 已实现模式

| 连接 | 接收匹配 | 发送目标 |
| --- | --- | --- |
| QQ Bot | 指定多个群、指定多个单聊、所有群、所有单聊、全部会话 | 指定一个群、指定一个单聊、回复当前来源会话 |
| MQTT | 全部订阅消息、订阅范围内的 Topic 筛选（`+` / `#`） | 默认发布 Topic、路由指定的固定 Topic |
| Webhook | 当前入口的全部消息，可叠加通用事件类型/指令筛选 | 无发送能力 |
| HTTP Client | 无接收能力 | 连接固定 URL，消息不覆盖 URL 或凭据 |

MQTT 的订阅连接维护实际 Broker 订阅范围。路由只进一步筛选，不自动扩大订阅。发送 Topic 不能包含通配符，不能命中本连接的订阅范围，QoS、Retain、TLS 和凭据继续由连接管理。路由指定 Topic 是管理员配置，不从消息正文动态插值。Broker ACL 仍是服务端最终权限边界。

QQ 新连接可以留空旧的 `group_openid`，使用一个 AppID 接多个群与单聊。此时路由必须明确选择接收范围和目标，不存在隐式“全部群”。已有固定群连接保持限制；留空之前必须先配置现有路由的范围，防止解除旧限制后意外扩大执行范围。同一 AppID 仍只能有一个连接。

## 来源信息与投递

`IncomingEvent.route_context` 由协议适配器设置，随 `EventEnvelope` 持久化：

- QQ：AppID、会话类型、会话 OpenID、原消息 ID、发送者 OpenID，仅在验签、发送者限制等检查通过后设置。
- MQTT：Broker PUBLISH 实际 Topic；与 `include_metadata` 选项无关。该选项只影响业务载荷形状。

通用 Webhook 不从请求正文创建可信来源信息。指令模板不能覆盖它。

`Message.destination_binding` 由路由适配器解析，并随投递记录保存。重试使用同一个解析结果；QQ/MQTT 出口在网络调用前再次校验。入站 `Message` 中同名字段会被路由重新解析的值覆盖，不能通过正文伪造发送目标。

QQ 的“回复来源会话”只适用于本条消息仍携带可信来源信息、且出口是同一个 QQ 连接的情况。原消息 ID 只可用于原 AppID、原会话的被动回复。跨群转发按主动消息处理，需要启用主动发送且符合 QQ 平台授权与配额。

异步 Agent 经独立 Webhook 返回的结果没有这份可信来源信息。本次不把任意 `group_openid/msg_id` 当作可信回复授权；后续应通过可过期、绑定任务与调用者的服务端回复句柄接回原会话。现有固定群旧模式仍兼容原有手工关联流程。

## 会话发现与 API

QQ 群消息和 C2C 消息验签并持久化成功时，同一事务更新发现目录。仅保存 AppID、会话类型、OpenID、最后发现时间；不把聊天内容复制进目录，也不假定 OpenID 是群号或昵称。当前通过消息事件发现，不主动枚举全部群、不处理入群/退群成员生命周期，不据此判断群当前是否仍可发消息。

`GET /api/message-connections/{connection_id}/conversations?kind=group&search=...&page=1&page_size=20`

- 与现有连接管理接口使用相同的控制台访问边界。
- `kind`: `group` / `direct`；`search` 为 OpenID 子串。
- 页大小 1–100，返回 `items/total/page/page_size`。
- 按空间、连接、上游账户隔离，单个连接最多保存 10,000 条发现记录；达到上限仍更新已知会话并正常接收消息。
- Schema v17 使用组合主键去重，使用 `(workspace,connection,account,kind,last_seen_ms DESC,id)` 索引支持筛选与稳定分页。连接删除或账号替换时清理对应记录。
- 发现不创建或启用路由。手动选择“所有群/全部会话”的路由会覆盖此后收到的新会话，这是显式选择的语义。

## 指令调用连接能力

路由统一编排「匹配 → 处理 → 目标 → 回复」。指令是 `command` 匹配条件；`processor` 决定转发、直接回复或调用能力；`response` 是路由级回复策略，与能力参数分离。

`processor.kind = respond` 用于直接回复：配置指令与 JSON 模板 `{"content":"回复文本"}`，设置 `response: {"kind":"source"}`，目标自动绑定来源连接的发送端。原会话由已验证入站上下文确定，不能由正文覆盖。回复复用现有 Outbox 和投递去重，已入队的直接回复遵循普通投递语义。

`processor.kind = invoke_capability` 配置一个目标连接、一项版本化能力及允许触发的用户。目标表示执行能力的连接，并不要求操作必须发送消息；例如查询机器人信息可以只记录结果。`response.kind = source` 将结果回复原会话，`none` 不回复。省略 response 的更新保留旧配置，显式 none 清除回复策略。


管理台操作：选择 QQ 来源与接收范围 → 处理选择「调用能力」→ 配置指令 → 选择目标连接与操作 → 填写调用参数和允许触发的用户 → 保存后启用。允许触发的用户使用回调中的发送者 OpenID，每行一个；不是 QQ 号，也不根据消息文本推断管理员身份。群范围与用户授权同时生效。

当前内置操作：

| 标识 | 参数 | 目标范围 | 行为 |
| --- | --- | --- | --- |
| `qq.bot_info`（revision 1） | `{}` | 不要求消息目标 | 查询当前机器人信息 |
| `qq.send_text`（revision 1） | `{"content":"文本"}` | 路由固定选择群、单聊或来源会话 | 提交文本消息，受 QQ 主动/被动发送限制 |

例如 `/发送 hello` 使用「指令与参数」匹配，参数模板填写 `{"content":{"$command":"arguments"}}`。模板仅映射数据，不执行代码；不能覆盖认证信息、目标群、用户或原消息 ID。`qq.bot_info` 对应官方 SDK 的 `GET /users/@me`；发送能力复用现有 QQ 消息适配器和 Token 缓存。API 参考：[腾讯 botpy](https://github.com/tencent-connect/botpy/blob/master/botpy/api.py)。

路由保存的关键字段示例（UUID 使用实际连接及其发送端 ID）：

```json
{
  "processor": {
    "kind": "invoke_capability",
    "tool_id": "qq.send_text",
    "revision": 1,
    "allowed_senders": ["发送者OpenID"]
  },
  "response": {"kind": "source"},
  "command": {
    "text_pointer": "/content",
    "trigger": "/发送",
    "mode": "prefix",
    "output": {
      "kind": "json",
      "event_type": "command.invoke",
      "template": {"content": {"$command": "arguments"}}
    }
  }
}
```

### 扩展接口与执行保证

- `ConnectionActions` 提供版本化能力目录与绑定目标后的 `ToolExecutor`。目录包含参数 Schema、效果提示、是否需要消息目标、是否占用被动回复序号。`GET /api/routes` 的目标连接通过 `actions` 返回这些信息。新增协议能力应扩展工厂与适配器，公共路由仍只排队，不在匹配阶段调用远程 API。
- `ConnectionRoutingAdapter` 补充 `validate_command_senders`、`authenticated_sender`、`command_reply`。只有能提供可信发送者的适配器才能用于指令授权；目前实现 QQ，普通 Webhook 不会从正文提取用户身份。
- 队列仅保存事件、路由和版本引用。执行前重读原事件，检查当前路由版本、来源范围、连接状态和用户授权，再映射参数。修改/停用/删除路由后，旧版本尚未执行的调用失败关闭；已完成的外部操作不会撤销。
- `InvocationService` 是 MCP 调用与指令调用共用的执行边界，执行 Schema 校验、授权复核、调用去重、结果记录和审计。调用审计主体按空间、来源连接、上游身份命名空间和发送者生成，不沿用后台操作员身份。
- 同一事件、同一路由使用稳定调用编号；SQLite 调用账本保存结果并与完成审计事务提交。重启后复用已知结果；正在执行或结果不确定的调用不会自动重复。继续使用已有调用账本组合主键和 Outbox 目标索引，无新增表或索引。
- 结果回复进入独立 Outbox 投递，绑定原会话与原消息。回复失败不会重新调用能力。调用结果保存后、回复入队前发生崩溃或存储错误时，任务进入待核对；使用原任务恢复时复用结果，不能承诺这一间隙已经完成回复。
- 当前每条能力路由只执行一个操作，回复限同一连接。能力执行与回复记录都计入路由投递统计。没有自动把这些能力开放到 MCP 服务；开放目录仍需显式配置。
- QQ 被动回复窗口、每消息次数及权限由平台限制，过期会失败。一个发送能力在原会话消耗第一条被动回复时，结果回执使用第二个序号；多条路由同时回复同一入站消息的跨路由配额协调尚未实现。
- 踢人、撤回、群管理等能力尚未接入，不通过这次抽象假定机器人获得平台权限。异步 Agent 回调的服务端回复句柄仍是独立待办。

## 当前边界

每条路由的每个出口连接仍只有一个目标配置；向同一个 QQ Bot 的多个群分别发送，可以配置多条路由。来源可在同一路由多选群。MQTT 目前一个连接一个订阅过滤器，通配符范围内可以建立多个路由。

会话列表当前显示 OpenID；群名/备注、QQ 退群状态同步、订阅过滤器列表、动态 Topic 模板以及异步 Agent 回复句柄尚未实现。它们应扩展适配器契约或目录服务，不应在公共路由分发流程新增协议分支。
