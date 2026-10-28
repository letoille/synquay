import type { ChangeConnectionInput, AddConnectionInput, VerifyInput, Verification, ControlView, Draft, DraftInput, History, OperationResult, PublishInput, Receipt, Tool, McpServer, ServerChange, ServerDeletion, ServerAuth, ContentInventory, BuiltinScope, ConversationGrant, BuiltinTool } from './types';

export class ControlError extends Error {
  constructor(readonly status: number, readonly code: string, readonly requestId: string | null = null) {
    super(code); this.name = 'ControlError';
  }
}
const bad = (): never => { throw new ControlError(0, 'invalid_response'); };
function obj(v: unknown): Record<string, unknown> { return v !== null && typeof v === 'object' && !Array.isArray(v) ? v as Record<string, unknown> : bad(); }
function str(v: unknown): string { return typeof v === 'string' ? v : bad(); }
function integer(v: unknown, min = 1): number { return typeof v === 'number' && Number.isSafeInteger(v) && v >= min ? v : bad(); }
function nullable(v: unknown): string | null { return v === null ? null : str(v); }
function names(v: unknown): string[] { return Array.isArray(v) && v.length <= 32 && v.every(n => typeof n === 'string') ? v : bad(); }
function draft(v: unknown): Draft {
  const d = obj(v); return { generation: integer(d.generation), base_revision: integer(d.base_revision), exposed_tools: names(d.exposed_tools), actor_id: str(d.actor_id) };
}
function receipt(v: unknown): Receipt {
  const r = obj(v); if (typeof r.replayed !== 'boolean') return bad();
  return { operation_id: str(r.operation_id), revision: integer(r.revision), replayed: r.replayed };
}
function contentInventory(value: unknown): ContentInventory {
  const c=obj(value);
  if (!Array.isArray(c.resources)||c.resources.length>32||!Array.isArray(c.prompts)||c.prompts.length>32) return bad();
  const resources=c.resources.map(raw=>{
    const r=obj(raw),kind=str(r.kind),parameter=nullable(r.parameter);
    if (!['resource','template'].includes(kind) || (kind==='template' && !parameter) || (kind==='resource' && parameter!==null)) return bad();
    return {id:str(r.id),description:str(r.description),mime_type:str(r.mime_type),kind:kind as 'resource'|'template',parameter};
  });
  const prompts=c.prompts.map(raw=>{
    const p=obj(raw);if(!Array.isArray(p.arguments)||p.arguments.length>8) return bad();
    return {id:str(p.id),description:str(p.description),arguments:p.arguments.map(raw=>{
      const a=obj(raw);if(typeof a.required!=='boolean'||!Array.isArray(a.choices)||a.choices.length>100||!a.choices.every(v=>typeof v==='string')) return bad();
      return {name:str(a.name),description:str(a.description),required:a.required,choices:a.choices as string[]};
    })};
  });
  if(new Set(resources.map(r=>r.id)).size!==resources.length||new Set(prompts.map(p=>p.id)).size!==prompts.length) return bad();
  return {revision:integer(c.revision,0),resources,prompts};
}
function view(v: unknown): ControlView {
  const r = obj(v), c = obj(r.connection);
  if (!Array.isArray(r.tools) || r.tools.length > 2048 || !['oauth', 'bearer'].includes(str(c.authentication))) return bad();
  const tools: Tool[] = r.tools.map(raw => {
    const t = obj(raw), d = obj(t.definition);
    if (!['ReadOnly', 'SideEffecting', 'Unknown'].includes(str(d.effect)) || typeof t.exposed !== 'boolean') return bad();
    return { public_name: str(t.public_name), connection_id: str(t.connection_id), url: t.protocol==='mqtt'?'':str(t.url), bearer_env: t.protocol==='mqtt'?null:nullable(t.bearer_env), exposed: t.exposed, allow_loopback_http: t.allow_loopback_http === undefined ? false : typeof t.allow_loopback_http === 'boolean' ? t.allow_loopback_http : bad(),
      definition: { id: str(d.id), description: str(d.description), effect: d.effect as Tool['definition']['effect'], input_schema: obj(d.input_schema), output_schema: d.output_schema === null ? null : obj(d.output_schema) } };
  });
  return { content: r.content===undefined?undefined:contentInventory(r.content), ...(r.workspace === undefined ? {} : { workspace: { id: str(obj(r.workspace).id), name: str(obj(r.workspace).name), state: str(obj(r.workspace).state) } }), revision: integer(r.revision), profile_id: str(r.profile_id), admin_id: str(r.admin_id), tools, draft: r.draft === null ? null : draft(r.draft),
    connection: { resource: nullable(c.resource), authentication: c.authentication as 'oauth' | 'bearer', issuer: nullable(c.issuer) } };
}
function history(v: unknown): History {
  const r = obj(v); if (!Array.isArray(r.items) || r.items.length > 50) return bad();
  return { items: r.items.map(raw => { const i = obj(raw); return { revision: integer(i.revision), created_ms: integer(i.created_ms, 0), actor_id: str(i.actor_id), exposed_tools: names(i.exposed_tools) }; }), next_before_revision: r.next_before_revision === null ? null : integer(r.next_before_revision) };
}
function serverAuth(value: unknown): ServerAuth {
  const a = obj(value), mode = str(a.mode);
  if (mode === 'inherited' || mode === 'none') return {mode};
  if (mode === 'basic') return {mode,username:str(a.username),password_env:str(a.password_env)};
  if (mode === 'jwt') return {mode,issuer:str(a.issuer),audience:str(a.audience),subject:str(a.subject),public_key_pem:str(a.public_key_pem)};
  if (mode === 'oauth' && typeof a.allow_loopback_http === 'boolean') return {mode,issuer:str(a.issuer),resource:str(a.resource),introspection_endpoint:str(a.introspection_endpoint),introspection_client_id:str(a.introspection_client_id),introspection_secret_env:str(a.introspection_secret_env),required_scope:str(a.required_scope),subject:str(a.subject),oauth_client_id:str(a.oauth_client_id),allow_loopback_http:a.allow_loopback_http};
  return bad();
}
function server(value: unknown): McpServer {
  const r = obj(value);
  if (![r.enabled, r.deleted, r.is_default].every(v => typeof v === 'boolean') || !['oauth','bearer','basic','jwt','none'].includes(str(r.authentication))) return bad();
  const id = str(r.id), path = str(r.path);
  if (!/^[a-f0-9-]{36}$/i.test(id) || !/^\/mcp(?:\/[A-Za-z0-9_-]{1,80})?$/.test(path)) return bad();
  const scope=r.content_scope===undefined?undefined:obj(r.content_scope);
  const content_scope=scope?{resources:names(scope.resources),prompts:names(scope.prompts)}:undefined;
  if(content_scope && (new Set(content_scope.resources).size!==content_scope.resources.length||new Set(content_scope.prompts).size!==content_scope.prompts.length)) return bad();
  let builtin_scope: BuiltinScope|undefined;
  if (r.builtin_scope !== undefined) {
    const b=obj(r.builtin_scope), tools=names(b.tools);
    if (!tools.every(x=>['connections_list','connection_status','connection_statistics','recent_messages','qq_bot_info','qq_send_text'].includes(x)) || !Array.isArray(b.conversations) || b.conversations.length>100) return bad();
    const parseGrants=(raw:unknown,send:boolean):ConversationGrant[]=>{if(!Array.isArray(raw)||raw.length>100)return bad();return raw.map(raw=>{const c=obj(raw),kind=str(c.kind);if(kind!=='group'&&kind!=='direct'&&(!(!send&&kind==='stream')))return bad();return {connection_id:str(c.connection_id),account:str(c.account),kind,conversation_id:str(c.conversation_id)};});};
    builtin_scope={tools:tools as BuiltinTool[],connections:names(b.connections),conversations:parseGrants(b.conversations,false),qq_send_conversations:parseGrants(b.qq_send_conversations??[],true)};
  }
  return {id, path, content_scope, builtin_scope, content_revision:r.content_revision===undefined?undefined:integer(r.content_revision,0), auth: r.auth === undefined ? undefined : serverAuth(r.auth), description: r.description === undefined ? '' : str(r.description), transport: r.transport === undefined || r.transport === 'streamable_http' ? r.transport as 'streamable_http' | undefined : bad(), name: str(r.name), enabled: r.enabled as boolean, deleted: r.deleted as boolean, is_default: r.is_default as boolean,
    revision: integer(r.revision), tool_revision: integer(r.tool_revision), exposed_tools: names(r.exposed_tools), address: nullable(r.address), authentication: r.authentication as McpServer['authentication']};
}
/** Console client. Login credentials are not used in the current development phase. */
export function controlApi() {
  async function request(path: string, method: string, body: unknown, signal?: AbortSignal): Promise<unknown> {
    const timeout = AbortSignal.timeout(40_000);
    let response: Response;
    try {
      response = await fetch(`/api/tools/${path}`, { method, signal: signal ? AbortSignal.any([signal, timeout]) : timeout,
        cache: 'no-store', credentials: 'omit', redirect: 'error',
        headers: { Accept: 'application/json', ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
    } catch { throw new ControlError(0, signal?.aborted ? 'cancelled' : 'network_error'); }
    if (!response.ok) {
      let code = 'request_failed';
      try { const data: unknown = await response.json(); const value = obj(data).code; if (typeof value === 'string' && /^[a-z_]{1,64}$/.test(value)) code = value; } catch { /* Proxy errors may not be JSON. */ }
      throw new ControlError(response.status, code, response.headers.get('x-request-id'));
    }
    try { return await response.json() as unknown; } catch { throw new ControlError(0, 'invalid_response', response.headers.get('x-request-id')); }
  }
  return {
    servers: async (signal?: AbortSignal): Promise<McpServer[]> => {
      const r = obj(await request('servers','GET',undefined,signal));
      if (!Array.isArray(r.items) || r.items.length > 64) return bad();
      return r.items.map(server);
    },
    server: async (id: string) => server(await request(`servers/${encodeURIComponent(id)}`, 'GET', undefined)),
    saveServer: async (id: string, input: ServerChange) => server(await request(`servers/${encodeURIComponent(id)}`, 'PUT', input)),
    deleteServer: async (id: string, input: ServerDeletion) => server(await request(`servers/${encodeURIComponent(id)}`, 'DELETE', input)),
    changeConnection: async (id: string, input: ChangeConnectionInput) => receipt(await request(`connections/${encodeURIComponent(id)}`, input.change.kind === 'delete' ? 'DELETE' : 'PUT', input)),
    addConnection: async (input: AddConnectionInput) => receipt(await request('connections', 'POST', input)),
    connectionResult: async (id: string): Promise<OperationResult> => {
      const r = obj(await request(`connections/${encodeURIComponent(id)}`, 'GET', undefined));
      if (r.status === 'committed' && r.receipt) return { status: 'committed', receipt: receipt(r.receipt) };
      if (r.status === 'unconfirmed' && r.receipt === null) return { status: 'unconfirmed', receipt: null };
      return bad();
    },
    verify: async (input: VerifyInput): Promise<Verification> => {
      const r = obj(await request('verify', 'POST', input));
      if (r.invocation_id !== input.invocation_id || !['succeeded', 'failed', 'unknown', 'unconfirmed'].includes(str(r.status))) return bad();
      return { invocation_id: str(r.invocation_id), status: r.status as Verification['status'] };
    },
    view: async (signal?: AbortSignal) => view(await request('config', 'GET', undefined, signal)),
    save: async (input: DraftInput, signal?: AbortSignal) => draft(await request('draft', 'PUT', input, signal)),
    publish: async (input: PublishInput, signal?: AbortSignal) => receipt(await request('publish', 'POST', input, signal)),
    history: async (before?: number, signal?: AbortSignal) => history(await request(`history?limit=10${before === undefined ? '' : `&before_revision=${before}`}`, 'GET', undefined, signal)),
    result: async (input: PublishInput, signal?: AbortSignal): Promise<OperationResult> => {
      const r = obj(await request(`operations/${encodeURIComponent(input.operation_id)}?expected_revision=${input.expected_revision}&draft_generation=${input.draft_generation}`, 'GET', undefined, signal));
      if (r.status === 'committed' && r.receipt) return { status: 'committed', receipt: receipt(r.receipt) };
      if (r.status === 'unconfirmed' && r.receipt === null) return { status: 'unconfirmed', receipt: null };
      return bad();
    },
  };
}
export type ControlApi = ReturnType<typeof controlApi>;
export function errorMessage(error: unknown): string {
  if (!(error instanceof ControlError)) return '操作未完成，请重新查询状态。';
  if (error.code === 'invalid_message_types') return '请选择有效且不重复的消息类型。';
  if (error.code === 'route_source_selection_required') return '请选择接收会话范围。';
  if (error.code === 'route_target_selection_required') return '请选择发送目标。';
  if (error.code === 'reply_requires_same_connection') return '回复来源会话需要使用同一个连接。';
  if (error.code === 'mqtt_filter_outside_subscription') return 'Topic 筛选必须在连接订阅范围内。';
  if (['invalid_qq_route_scope','unsupported_route_binding','invalid_route_binding','unknown_route_binding_target'].includes(error.code)) return '路由范围与连接不兼容，请重新选择。';
  if (['invalid_command_action','unknown_connection_action','command_sender_unavailable','command_reply_requires_same_connection','invalid_route_processing','invalid_direct_response','route_reply_unavailable'].includes(error.code)) return '请检查能力、调用参数、用户授权和回复范围。';
  if (error.code === 'invalid_command_mapping') return '指令映射无效，请检查匹配内容、消息字段和模板。';
  if (error.code === 'connection_grant_in_use') return '该名称被 OAuth 授权引用，请先调整服务端授权配置再重命名。';
  if (error.code === 'unknown_or_duplicate_content') return '资源或提示词不存在或重复，请刷新后重试。';
  if (error.code === 'invalid_mcp_url') return 'MCP 地址无效：请使用 HTTPS（本机可用 HTTP），路径为 /mcp 或 /mcp/名称，不含查询参数。';
  if (error.code === 'mcp_url_taken') return 'MCP 地址路径已被其他服务使用。';
  if (error.code === 'invalid_server_auth') return '认证配置无效，请检查公钥、签发者和地址等字段。';
  if (['missing_tool_secret', 'invalid_tool_secret', 'invalid_secret_reference'].includes(error.code)) return '密钥环境变量不存在或格式无效，请检查服务端配置。';
  if (error.code === 'recovery_storage_unavailable') return '浏览器无法保存发布操作编号。本次发布未发起，请允许当前站点使用会话存储。';
  
  if (error.code === 'origin_denied') return '此管理台地址尚未获准访问，请在管理员配置中添加准确的浏览器来源。';
  if (error.status === 403) return '当前身份没有执行此操作的权限。';
  if (error.status === 404) return '管理接口未启用或对象不存在，请检查管理员配置。';
  if (error.status === 409) return '配置已发生变化，请读取最新版本后重新检查。';
  if (error.status === 429) return '已有操作正在处理或服务繁忙，请稍后查询。';
  if (error.code === 'capacity_exceeded') return '存储容量不足，暂时无法保存。';
  if (error.status === 400) return '配置或操作参数无效，请重新读取服务信息。';
  if (error.code === 'invalid_response') return '服务响应格式异常，结果尚未确认。';
  return '服务暂时无法响应，结果尚未确认。';
}
