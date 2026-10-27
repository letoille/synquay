# QQ 官方机器人

QQ Bot 是双向消息连接，当前支持官方 Webhook、群和 C2C 单聊文本消息接收与发送，复用持久化入口、指令映射、HTTP Agent 路由及投递队列。不是模拟 QQ 用户登录的协议机器人。

## 配置

1. 管理台直接填写 QQ 开放平台的 AppSecret。保存后只显示「已配置」，不提供查看入口；编辑时留空保留原密钥，填写新值替换。更换 AppID 必须重新填写密钥。QQ 不再使用环境变量引用。
2. 管理台「连接 → 添加连接 → QQ Bot」填写 AppID、密钥；限定群可以留空。群 OpenID 是群消息事件 `d.group_openid`，不是 QQ 群号；成员 OpenID 是 `d.author.member_openid`。
3. 「允许的成员」为空时接收所有符合连接范围的用户；填写后仅接收列出的成员。给设备控制指令使用的连接应限定成员。平台签名只证明来源是 QQ，不代表群内每个人都有设备操作权限。
4. 新连接保存后默认启用，复制 `/api/qq/{connection_id}/webhook` 地址，使用部署的 HTTPS 域名。localhost 地址只能本地验证，不能填到 QQ 公网回调配置。
5. 在 QQ 开放平台配置该 HTTPS 回调地址，订阅 `GROUP_AT_MESSAGE_CREATE`，单聊订阅 `C2C_MESSAGE_CREATE`。如果需要全量群消息，还需群主授权接收范围并订阅 `GROUP_MESSAGE_CREATE`。
6. 到「路由」选择此 QQ 连接为入口，目标选 HTTP / MQTT / QQ，或者选择 Agent（HTTP 接收端）。配置指令时，消息字段使用 `/content`。

一个连接对应一个机器人。留空限定群后，在路由中选择群或单聊范围；会话从验签通过的消息自动发现，可搜索、分页及多选。已有固定群配置继续限制在该群。详细契约、存储和限制见 [连接专属路由](connection-routing.md)。

## 收到的标准载荷

群事件类型为 `qq.group.message`，单聊为 `qq.direct.message`；单聊载荷使用 `user_openid`，`group_openid` 为 null。群 @ 消息的 content 已由 QQ 去除 @机器人的前缀，Synquay 只去掉正文首尾空白；不把任意用户提及当作指令。当前不处理附件下载、语音转写或卡片中的嵌套正文。

```json
{
  "content": "/助手 查询设备状态",
  "msg_id": "ROBOT1.0_xxx",
  "group_openid": "GROUP_OPENID",
  "sender_openid": "MEMBER_OPENID",
  "timestamp": "2026-10-26T12:00:00+08:00",
  "attachments": null
}
```

附件信息仅作为不可信元数据保留，不自动访问其中 URL。不支持的事件、连接范围外的会话、被排除的用户和标记为机器人的发送者被确认但不入队。

## 指令与回复

例如 `/回声 你好`，路由来源和目标都选择此 QQ 连接，目标范围选「回复来源会话」。启用「指令映射」，字段 `/content`，触发 `/回声`，选择「指令与参数」，输出 JSON：

```json
{
  "content": {"$command":"arguments"},
  "msg_id": {"$payload":"/msg_id"},
  "group_openid": {"$payload":"/group_openid"},
  "msg_seq": 1
}
```

QQ 出口只发送文本：`content` 必须是非空文本，当前限制 4000 UTF-8 字节。目标来自路由的群/单聊选择或可信来源会话，载荷不能覆盖；旧的固定群模式仍校验 `group_openid`。其他 JSON 字段不透传为 QQ API 参数。

被动回复使用可信来源中的原消息 ID（旧固定群模式兼容载荷 `msg_id`）。群回复有时间窗口及次数限制；群序号限制 1–5，单聊 1–4，实际窗口和配额由 QQ 当前平台判定。`msg_seq` 默认 1，可显式设置为 1–5；同一消息的不同逻辑回复须使用不同序号，重试保持原序号。当前不自动分配多路由序号，不自动拆分长消息。重复回复被 QQ 拒绝后进入待核实状态，不能报告为已发送。

不带 `msg_id` 是主动消息，仅在连接勾选「允许主动发送」时允许，QQ 平台还需群主授权和配额。被动回复过期后不会自动转为主动消息。通知、定时任务可使用主动发送。

### Agent 往返

需要运行实际 Agent HTTP 服务：

此流程仅适用于旧固定群模式。机器人级动态会话的异步回复需要后续实现服务端回复句柄，不能从任意 Webhook 声明原会话。

1. QQ → Agent HTTP：将指令参数、`msg_id`、`group_openid` 发给 Agent。
2. Agent 保存这些关联字段，完成后向一个已配置、带身份校验的 Synquay Webhook 回调发送结果。
3. 回调 → QQ：路由载荷包含 `content`、原 `msg_id`、匹配的 `group_openid` 和 `msg_seq`。

Agent 的 HTTP 2xx 只代表提交成功，不代表结果已生成。结果须在 QQ 被动回复窗口内返回。现有 MCP Agent 部署队列暂不支持直接绑定此动态 QQ 连接；本版不宣称 QQ 消息能够自动唤醒 ChatGPT，也没有内置模型执行器。

## 可靠性与权限

- 实现 QQ Ed25519 签名和 op=13 回调验证。事件签名覆盖原始请求体，校验 AppID，时间戳窗口五分钟；服务端时钟需同步。
- 上限 64 KiB 请求体、五秒读取超时，复用入口背压配额。
- op=0 事件落库确认后才 ACK。连接作用域 + 原消息 ID 去重，重启后仍有效；同 ID 不同内容返回冲突。重复的全量 / @ 事件若标准化载荷一致不会重复路由。
- Bot Secret、token 不进入快照或日志；响应体有大小上限，日志只记录数值状态和错误码。
- Access token 按需获取并缓存，过期前 60 秒刷新；并发刷新合并。支持官方 `expires_in` 字符串与数字。HTTP 200 仍检查 QQ 业务错误码。
- 限流、无权限、回复过期分别处理。超时、5xx、去重响应不会盲目确认或再次发送；复用投递待核实与恢复机制，不保证第三方消息恰好一次送达。
- 复用已有连接快照、事件去重及投递索引，没有新增数据库表或索引。

## 尚未接入

WebSocket、单聊/频道、Markdown/媒体上传、指令面板同步、群发现、多群共享回调、撤回、禁言、审批、踢人、黑名单、逐群能力探测。控制台不提供尚未实现的群管理开关。平台开放接口不等于机器人已经获授权；踢人等内邀接口尤其不能仅靠本地配置开通。

## 官方依据

- [Webhook 回调验证](https://bot.q.qq.com/wiki/develop/api-v2/dev-prepare/event-emit/webhook.html)
- [Ed25519 签名](https://bot.q.qq.com/wiki/develop/api-v2/dev-prepare/interface-framework/sign.html)
- [Access token](https://bot.q.qq.com/wiki/develop/api-v2/dev-prepare/access-token.html)
- [群 @ 事件](https://bot.q.qq.com/wiki/develop/api-v2/autogen/event/group_at_message_create.html)
- [群消息发送](https://bot.q.qq.com/wiki/develop/api-v2/autogen/api/v2_groups_group_openid_messages.post.html)

## 密钥存储

连接保存请求的 `secret` 为只写字段，连接读取接口仅返回 `secret_configured`。实际值不进入连接快照、操作记录或审计输出；运行时配置中的密钥字段禁止 JSON 序列化和反序列化。浏览器不把含密钥的请求放入 sessionStorage；页面未关闭时可用原请求重试，关闭或刷新后先重新读取连接状态，再编辑。

SQLite v18 的 `connection_secrets` 表使用 AES-256-GCM 密文，认证附加数据绑定空间、管线来源和连接 ID。密钥更新、连接配置及操作回执在同一数据库事务内提交；重试比较 HMAC 指纹，不存明文请求。删除连接同时删除对应密文。组合主键覆盖读取、替换与删除，无额外索引。

加密主密钥自动生成在数据库旁的 `<数据库文件名>.connection-secrets.key`，Unix 权限 0600。`scripts/backup.py` 已同步备份、校验和恢复该文件；手工备份和迁移也必须同时保存数据库与该文件；丢失或篡改时拒绝加载，不能恢复已保存的 AppSecret。服务器仍需解密密钥调用 QQ，因此本设计保证管理台不可回读，不承诺对拥有服务器文件权限的管理员隐藏。
