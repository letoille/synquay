# 凭据管理

凭据模块提供受限的秘密取用、刷新和轮换。当前 HTTP 链路从环境变量读取独立入站/出站令牌；目标设计将秘密存储与连接配置分开，配置只保留引用。

## 领域对象与归属

CredentialBinding 绑定工作区、连接、用途、所有者、上游主体和模式（PerUser/SharedService/NodeLocal）；SecretVersion 保存加密密文及密钥版本；CredentialLease 是一次取用的短期受限结果。SecretRef 不能直接当下载地址或普通可展示的字段。

用户授权、网关入口 token、平台机器人秘密、事件签名密钥与 Runner 节点身份分别绑定。缺少 PerUser 授权时返回 NeedsAuthorization，不能回退到管理员共享账号。

## 应用服务与端口

```rust
trait CredentialResolver: Send + Sync {
    fn resolve(&self, request: CredentialUse) -> PortFuture<'_, CredentialLease>;
}
trait SecretVault: Send + Sync {
    fn store(&self, write: SecretWrite) -> PortFuture<'_, SecretVersionRef>;
    fn decrypt(&self, access: VaultAccess) -> PortFuture<'_, SecretMaterial>;
}
trait CredentialRefresher: Send + Sync {
    fn refresh(&self, binding: CredentialBindingRef) -> PortFuture<'_, CredentialState>;
}
```

CredentialResolver 是应用服务，由可信适配器构建逻辑用途和目标引用，先调用授权服务再访问 Vault；参数中的 Authorization/env 不能覆盖绑定。SecretMaterial 不实现 Debug/普通序列化，限制复制与驻留时间；不能承诺语言运行时中不存在内存副本。

## 状态与一致性

绑定状态 Active、RefreshRequired、NeedsAuthorization、Revoked；秘密版本 Active、Retiring、Retired。刷新按绑定单飞并用 expected_version CAS，网络刷新在事务外，结果提交重新确认绑定未撤销。新凭据和旧版本关系原子落库，禁止并发刷新相互覆盖。

任务固定 binding_id 及允许用途，取用时选择当前可用 secret version，并在审计中记录实际版本。地址、作用主体或用途变化需要新连接配置版本；轮换秘密不改变消息目的地。已经发出的请求保留其实际执行身份记录。

## 对接与存储

配置发布验证引用可用但不解密；连接器和 Runner 按执行上下文取用；仅向远端节点传必要秘密或一次性兑换凭证。NodeLocal 秘密由节点保管，云端仅保存引用。主密钥不与数据库备份捆绑保存，密钥提供者作为独立基础设施实现。

表组为 credential_bindings、secret_versions、refresh_leases、credential_use_records。运行日志只记绑定/版本 ID 与结果码；管理台显示脱敏状态，不提供通用“读取全部秘密”接口。

## 验收

跨用户/连接取用拒绝；无上游授权返回明确状态；轮换后旧任务使用允许的新秘密但不变目标；并发刷新只提交一个版本；刷新期间撤销不能重新激活；日志、错误和发布快照不含原始秘密；备份缺少独立密钥时不能解密。
