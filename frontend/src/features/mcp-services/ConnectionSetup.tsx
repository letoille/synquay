import { AlertDialog, Dialog } from 'radix-ui';
import { Icon } from '../../shared/ui/Icon';
import { IconButton } from '../../shared/ui/IconButton';
import { CopyButton } from '../../shared/ui/CopyButton';
import { Button, Input, Checkbox } from '../../shared/ui/Controls';
import { t } from '../../shared/i18n';
import { useEffect, useRef, useState } from 'react';
import { ControlError, errorMessage } from './api';
import type { ControlApi } from './api';
import type { ChangeConnectionInput, AddConnectionInput, ControlView, Effect, Receipt, Tool } from './types';
import { jsonObject, operationKey, readOperation, saveOperation } from './connectionModel';

type Operation = {kind:'add';input:AddConnectionInput} | {kind:'change';connectionId:string;input:ChangeConnectionInput};

export function ConnectionSetup({ api, view, locked, completed, onBlock }: { api: ControlApi; view: ControlView; locked: boolean; completed: () => void; onBlock: (value: boolean) => void }) {
  const key = operationKey(view, 'connection');
  const [pending, setPending] = useState(() => readOperation(key));
  const [editing, setEditing] = useState(false), [busy, setBusy] = useState(false);
  const [name, setName] = useState(''), [url, setUrl] = useState(''), [loopback, setLoopback] = useState(false);
  const [description, setDescription] = useState(''), [effect, setEffect] = useState<Effect>('Unknown');
  const [schema, setSchema] = useState('{\n  "type": "object",\n  "properties": {},\n  "additionalProperties": false\n}');
  const [existing, setExisting] = useState<Tool | null>(null);
  const [removing, setRemoving] = useState<Tool | null>(null);
  const opener = useRef<HTMLElement | null>(null);
  const createButton = useRef<HTMLButtonElement>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  const [resolved, setResolved] = useState(false);
  const [error, setError] = useState(''), [message, setMessage] = useState('');
  const gate = useRef(false), last = useRef<Operation | null>(null);
  const blocked = editing || busy || !!pending || !!removing;
  useEffect(() => { onBlock(blocked); return () => onBlock(false); }, [blocked, onBlock]);
  useEffect(() => {
    if (!blocked) return;
    const fn = (e: BeforeUnloadEvent) => e.preventDefault(); window.addEventListener('beforeunload', fn);
    return () => window.removeEventListener('beforeunload', fn);
  }, [blocked]);
  function validate() {
    if (!/^[a-zA-Z0-9_-]{1,100}$/.test(name)) throw Error("能力名称请使用 1–100 个英文字母、数字、下划线或连字符。");
    const u = new URL(url);
    if (u.username || u.password || u.search || u.hash) throw Error("地址不能包含账号、密码、查询参数或片段。");
    if (u.protocol !== 'https:' && !(loopback && u.protocol === 'http:' && ['127.0.0.1', 'localhost', '[::1]'].includes(u.hostname))) throw Error("请输入 HTTPS 地址；本机 HTTP 地址需要单独允许。");
    if (!description.trim()) throw Error("请说明这个能力能做什么及何时使用。");
    let input: Record<string, unknown>;
    try { input = jsonObject(schema); } catch { throw Error('请输入有效的 JSON Schema。'); } if (input.type !== 'object') throw Error("输入格式必须是 type 为 object 的 JSON Schema。");
    return input;
  }
  async function done(receipt: Receipt, id: string) {
    if (receipt.operation_id !== id) throw new ControlError(0, 'invalid_response');
    const deleted = last.current?.kind === 'change' && last.current.input.change.kind === 'delete';
    saveOperation(key, null); setPending(null); setEditing(false); setRemoving(null); setExisting(null); last.current = null;
    setMessage(deleted ? '连接已删除，引用已清理。' : '操作已完成。'); setName(''); setUrl(''); setDescription(''); setLoopback(false); setEffect('Unknown'); setSchema('{"type":"object","properties":{},"additionalProperties":false}'); completed();
  }
  async function run(fn: () => Promise<void>) {
    if (gate.current) return; gate.current = true; setBusy(true); setError('');
    try { await fn(); } catch (e) { setError(e instanceof ControlError ? errorMessage(e) : e instanceof Error ? e.message : "操作未完成。"); }
    finally { gate.current = false; setBusy(false); }
  }
  async function send(operation: Operation) {
    const id = operation.input.operation_id;
    if (!saveOperation(key, id)) throw Error('无法保存恢复编号，尚未发起操作。请允许此站点使用会话存储。');
    last.current = operation; setPending(id);
    try { await done(await (operation.kind === 'add' ? api.addConnection(operation.input) : api.changeConnection(operation.connectionId, operation.input)), id); }
    catch (e) { if (e instanceof ControlError && [400,401,403,404,409,413,415,429].includes(e.status)) {saveOperation(key,null);setPending(null);last.current=null;} throw e; }
  }
  function save() { void run(async () => {
    const inputSchema=validate(), id=crypto.randomUUID();
    if (existing) await send({kind:'change',connectionId:existing.connection_id,input:{operation_id:id,expected_revision:view.revision,change:{kind:'update',public_name:name,description:description.trim(),url,effect,input_schema:inputSchema,allow_loopback_http:loopback}}});
    else await send({kind:'add',input:{operation_id:id,expected_revision:view.revision,tool:{public_name:name,connection_id:id,url,bearer_env:null,exposed:false,allow_loopback_http:loopback,definition:{id:name,description:description.trim(),effect,input_schema:inputSchema,output_schema:null}}}});
  }); }
  function edit(tool: Tool | null) {
    opener.current=document.activeElement as HTMLElement;setExisting(tool);setEditing(true);setError('');setMessage('');
    setName(tool?.public_name??'');setDescription(tool?.definition.description??'');setUrl(tool?.url??'');setEffect(tool?.definition.effect??'Unknown');setLoopback(tool?.allow_loopback_http??false);
    setSchema(JSON.stringify(tool?.definition.input_schema??{type:'object',properties:{},additionalProperties:false},null,2));
  }
  function restoreFocus(event: Event) {
    event.preventDefault();
    (opener.current?.isConnected ? opener.current : createButton.current)?.focus();
  }
  function closeEditor() { if (!busy && !pending) { setEditing(false); setRemoving(null); setError(''); } }
  const effectLabel = (value: Effect) => t({ReadOnly:'只读取数据', SideEffecting:'会修改数据或操作设备', Unknown:'未声明'}[value]);
  const feedback = <>{message && <p className="feedback" role="status">{t(message)}</p>}{error && <p className="error" role="alert">{t(error)}</p>}</>;
  return <section aria-label={t('连接管理')}>
    <div className="section-bar server-list-actions"><span className="muted small">{t('HTTP 工具')} · {view.tools.filter(t=>t.url).length}</span><div className="actions">
      <Button disabled={locked || blocked} onClick={completed}>{t('刷新')}</Button>
      <Button ref={createButton} disabled={locked || blocked || !view.workspace || view.tools.filter(t=>t.url).length >= 32} onClick={() => edit(null)}><Icon name="plus" size={16} />{t('添加 HTTP 工具')}</Button>
    </div></div>
    {!view.workspace && <p className="notice">{t('当前服务尚未提供在线配置接口，请升级并重启后端。')}</p>}
    {!editing && !pending && !removing && feedback}
    <div className="server-list">{view.tools.filter(t=>t.url).map(tool => <article className="server-row connection-row" key={tool.connection_id} aria-label={tool.public_name}>
      <div className="server-summary"><div className="server-title"><h2>{tool.public_name}</h2><span className="connection-protocol">HTTP · {t('工具')}</span><div className="card-address"><code title={tool.url}>{tool.url}</code><CopyButton value={tool.url} label={`${t('复制地址')} · ${tool.public_name}`} /></div></div><div className="card-description-row"><p className="card-description" title={tool.definition.description}>{tool.definition.description}</p></div></div>
      <div className="actions server-card-actions"><IconButton label={`${t('编辑')} · ${tool.public_name}`} disabled={locked || blocked} onClick={() => edit(tool)}><Icon name="edit" size={17} /></IconButton><IconButton variant="ghost" label={`${t('删除')} · ${tool.public_name}`} disabled={locked || blocked} onClick={() => {opener.current=document.activeElement as HTMLElement;setRemoving(tool);setError('');setMessage('');}}><Icon name="trash" size={17} /></IconButton></div>
    </article>)}{!view.tools.filter(t=>t.url).length && <p className="empty">{t('尚未添加 HTTP 工具')}</p>}</div>
    <Dialog.Root open={editing || !!pending} onOpenChange={open => {if (!open) closeEditor();}}><Dialog.Portal><Dialog.Overlay className="ui-overlay" /><Dialog.Content className="ui-dialog" aria-describedby={undefined} onCloseAutoFocus={restoreFocus} onOpenAutoFocus={event => {if (!pending) {event.preventDefault(); nameInput.current?.focus();}}} onEscapeKeyDown={event => {if (busy || pending) event.preventDefault();}} onInteractOutside={event => event.preventDefault()}>
      <div className="ui-dialog-header"><Dialog.Title>{t(pending ? '操作结果待确认' : existing ? '编辑连接' : '添加连接')}</Dialog.Title><Button variant="ghost" className="ui-close" aria-label={t('关闭')} disabled={busy || !!pending} onClick={closeEditor}><Icon name="close" size={18} /></Button></div>
      {pending ? <div className="ui-dialog-body connection-recovery">{feedback}<p className="small muted">{t('先查询原操作，确认结果后再继续。')}</p><p className="mono">{pending}</p><div className="actions"><Button disabled={busy} onClick={() => void run(async () => {const result = await api.connectionResult(pending); if (result.receipt) await done(result.receipt, pending); else setMessage('尚未发现提交回执。可稍后查询；若已重新加载页面，请保留编号并联系管理员核查。');})}>{t('查询保存结果')}</Button>{last.current && <Button disabled={busy} onClick={() => void run(async () => {await send(last.current!);})}>{t('使用原编号重试')}</Button>}</div><Checkbox checked={resolved} onCheckedChange={value => setResolved(value === true)}>{t('我已核查服务端，结束本地跟踪')}</Checkbox><Button disabled={!resolved || busy} onClick={() => {saveOperation(key, null); setPending(null); setEditing(false); setRemoving(null); last.current = null; setResolved(false); setError(''); setMessage(''); completed();}}>{t('结束核查')}</Button><p className="small muted">{t('这不会取消已经接纳的保存操作。')}</p></div> :
      <form className="server-editor connection-editor" onSubmit={event => {event.preventDefault(); save();}}>
        <div className="ui-dialog-body">{feedback}<fieldset disabled={busy || locked}>
          <label className="ui-field"><span>{t('名称')}</span><Input ref={nameInput} required maxLength={100} pattern="[a-zA-Z0-9_-]+" value={name} onChange={event => setName(event.target.value)} placeholder="query_device_state" /></label>
          <label className="ui-field"><span>{t('说明')}</span><Input required maxLength={4000} value={description} onChange={event => setDescription(event.target.value)} /></label>
          <label className="ui-field"><span>{t('请求地址')}</span><Input type="url" required maxLength={2048} value={url} onChange={event => setUrl(event.target.value)} placeholder="https://api.example.com/actions/query" /></label>
          <div className="ui-field"><span>{t('接入方式')}</span><p className="server-transport-value">HTTP · JSON POST</p></div>
          <div className="ui-field"><span>{t('认证方式')}</span><p className="server-transport-value">{existing?.bearer_env ? 'Bearer' : t('无鉴权')}</p></div>
          <div className="connection-loopback"><Checkbox checked={loopback} onCheckedChange={value => setLoopback(value === true)}>{t('允许服务端本机的 HTTP 地址')}</Checkbox></div>
          <label className="ui-field"><span>{t('操作性质')}</span><select className="ui-input" value={effect} onChange={event => setEffect(event.target.value as Effect)}>{(['Unknown','ReadOnly','SideEffecting'] as const).map(value => <option key={value} value={value}>{effectLabel(value)}</option>)}</select></label>
          <label className="ui-field connection-schema"><span>{t('输入参数')}<small>JSON Schema</small></span><textarea required className="ui-input mono" rows={6} maxLength={16000} value={schema} onChange={event => setSchema(event.target.value)} spellCheck={false} /></label>
        </fieldset></div><div className="ui-dialog-footer"><Button disabled={busy} onClick={closeEditor}>{t('取消')}</Button><Button type="submit" variant="primary" disabled={busy || locked}>{t(busy ? '正在保存…' : '保存连接')}</Button></div>
      </form>}
    </Dialog.Content></Dialog.Portal></Dialog.Root>
    <AlertDialog.Root open={!!removing && !pending} onOpenChange={open => {if (!open) closeEditor();}}><AlertDialog.Portal><AlertDialog.Overlay className="ui-overlay" /><AlertDialog.Content className="ui-dialog ui-dialog--confirm" onCloseAutoFocus={event => {if (pending) event.preventDefault();else restoreFocus(event);}}>
      <div className="ui-dialog-header"><AlertDialog.Title>{t('删除连接')}</AlertDialog.Title></div>
      <div className="ui-dialog-body"><p className="delete-server-name">{removing?.public_name}</p><AlertDialog.Description>{t('同时移除所有 MCP 服务及未发布配置中的引用。MCP 服务和历史记录保留。')}</AlertDialog.Description>{feedback}</div>
      <div className="ui-dialog-footer"><AlertDialog.Cancel asChild><Button disabled={busy}>{t('取消')}</Button></AlertDialog.Cancel><Button variant="danger" disabled={busy || locked} onClick={() => {if (removing) void run(() => send({kind:'change',connectionId:removing.connection_id,input:{operation_id:crypto.randomUUID(),expected_revision:view.revision,change:{kind:'delete'}}}));}}>{t('确认删除')}</Button></div>
    </AlertDialog.Content></AlertDialog.Portal></AlertDialog.Root>
  </section>;
}
