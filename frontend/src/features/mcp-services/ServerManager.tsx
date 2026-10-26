import { generatedOrigin } from '../../shared/urls';
import { useEffect, useRef, useState } from 'react';
import { Dialog, AlertDialog } from 'radix-ui';
import { Button, Input } from '../../shared/ui/Controls';
import { Icon } from '../../shared/ui/Icon';
import { IconButton } from '../../shared/ui/IconButton';
import { CopyButton } from '../../shared/ui/CopyButton';
import { t } from '../../shared/i18n';
import { ControlError, errorMessage, type ControlApi } from './api';
import type { ControlView, McpServer, ServerChange, ServerDeletion, ServerAuth, ContentScope, CapabilityKind } from './types';

import { CapabilityEditor } from './CapabilityEditor';
import { ServerDetails } from './ServerDetails';
import { ServerAuthFields } from './ServerAuthFields';

type Operation = { id: string; kind: 'save'; input: ServerChange } | { id: string; kind: 'delete'; input: ServerDeletion };
export function ServerManager({ api, view, locked, changed, onBlock }: {
  api: ControlApi; view: ControlView; locked: boolean; changed: () => void; onBlock: (value: boolean) => void;
}) {
  const [items, setItems] = useState<McpServer[]>([]), [loading, setLoading] = useState(true);
  const [editor, setEditor] = useState<{ id: string; existing: McpServer | null } | null>(null);
  const [scope, setScope] = useState<ContentScope>({resources:[],prompts:[]});
  const [description, setDescription] = useState('');
  const [editorAddress, setEditorAddress] = useState('');
  const [auth, setAuth] = useState<ServerAuth>({mode:'inherited'});
  const [name, setName] = useState(''), [enabled, setEnabled] = useState(true), [selected, setSelected] = useState<string[]>([]);
  const [removing, setRemoving] = useState<McpServer | null>(null), [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<unknown>(null), [message, setMessage] = useState('');
  const [pending, setPending] = useState<Operation | null>(null), [attempt, setAttempt] = useState(0);
  const [details, setDetails] = useState<{ server: McpServer; mode: CapabilityKind } | null>(null);
  const [stopping, setStopping] = useState<McpServer | null>(null);
  const gate = useRef(false);
  const opener = useRef<HTMLElement | null>(null);
  const createButton = useRef<HTMLButtonElement>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  const blocked = !!editor || !!removing || !!stopping || busy || !!pending;
  useEffect(() => { onBlock(blocked); return () => onBlock(false); }, [blocked, onBlock]);
  useEffect(() => {
    if (!blocked) return;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener('beforeunload',warn); return () => window.removeEventListener('beforeunload',warn);
  },[blocked]);
  useEffect(() => {
    const controller = new AbortController(); setLoading(true);
    void api.servers(controller.signal).then(result => { if (!controller.signal.aborted) {setItems(result);setProblem(null);} })
      .catch(error => { if (!controller.signal.aborted) setProblem(error); })
      .finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  },[api,attempt]);
  const address = (server: McpServer) => server.address ?? new URL(server.path, generatedOrigin()).href;
  function edit(server: McpServer | null) {
    opener.current = document.activeElement as HTMLElement;
    setProblem(null); setMessage(''); setName(server?.name ?? ''); setEnabled(server?.enabled ?? true);
    setScope(server?.content_scope ?? {resources:[],prompts:[]});
    setDescription(server?.description ?? ''); setAuth(server?.auth ?? {mode:'inherited'});
    const id = server?.id ?? crypto.randomUUID();
    setEditorAddress(server ? address(server) : new URL(`/mcp/${crypto.randomUUID().replaceAll('-', '').slice(0, 8)}`,generatedOrigin()).href);
    setSelected(server?.exposed_tools ?? []); setEditor({ id, existing: server });
  }
  async function run(operation: Operation) {
    if (gate.current) return;
    gate.current=true; setBusy(true);setPending(operation);setProblem(null);setMessage('');
    try {
      const result = operation.kind === 'save' ? await api.saveServer(operation.id, operation.input) : await api.deleteServer(operation.id, operation.input);
      if (result.id !== operation.id || result.deleted !== (operation.kind === 'delete')) throw new ControlError(0,'invalid_response');
      setItems(current => result.deleted ? current.filter(item => item.id !== result.id) : current.some(item => item.id === result.id) ? current.map(item => item.id === result.id ? result : item) : [...current, result]);
      setPending(null);setEditor(null);setRemoving(null);setStopping(null);setAttempt(value => value+1);changed();
      setMessage(operation.kind === 'save' ? '服务已保存。' : '服务已删除。');
    } catch (error) {
      setProblem(error);
      if (error instanceof ControlError && [400,403,404,409,413,415,429].includes(error.status)) setPending(null);
    } finally { gate.current=false;setBusy(false); }
  }
  function save() {
    if (!editor) return;
    void run({id:editor.id,kind:'save',input:{operation_id:crypto.randomUUID(),expected_revision:editor.existing?.revision ?? 0,
      ...(view.content && (!editor.existing || editor.existing.content_scope) ? {content_scope:scope,expected_content_revision:view.content.revision}:{}),
      expected_tool_revision:editor.existing?.tool_revision ?? 0,name:name.trim(),description:description.trim(),public_url:editorAddress.trim(),auth,transport:'streamable_http',enabled,exposed_tools:selected}});
  }
  function toggle(server: McpServer, next: boolean) {
    void run({ id: server.id, kind: 'save', input: { operation_id: crypto.randomUUID(), expected_revision: server.revision,
      expected_tool_revision: server.tool_revision, name: server.name, enabled: next, exposed_tools: server.exposed_tools } });
  }
  function openDetails(server: McpServer, mode: CapabilityKind) {
    opener.current = document.activeElement as HTMLElement;
    setDetails({ server, mode });
  }
  const disabled = busy || !!pending || locked;
  function closeEditor() {
    if (busy || pending) return;
    setEditor(null); setRemoving(null); setStopping(null); setProblem(null); setMessage('');
  }
  function restoreFocus(event: Event) {
    event.preventDefault();
    (opener.current?.isConnected ? opener.current : createButton.current)?.focus();
  }
  const feedback = <>
    {problem !== null && <div className="error" role="alert"><p>{t(errorMessage(problem))}</p>{problem instanceof ControlError && problem.status === 409 && <p>{t('请取消编辑、刷新列表后重试。')}</p>}</div>}
    {message && <p className="feedback" role="status">{t(message)}</p>}
    {pending && !busy && <section className="notice"><p>{t('操作结果待确认，请重试原操作；不要重复创建服务。')}</p><p className="mono small">{pending.input.operation_id}</p><Button onClick={() => void run(pending)}>{t('使用原编号重试')}</Button></section>}
  </>;
  return <section aria-label={t('MCP 服务管理')}>
    <div className="section-bar server-list-actions"><span className="muted small">{loading ? t('正在加载…') : t('{0} 个服务', [items.length])}</span><div className="actions"><Button disabled={blocked || loading} onClick={() => setAttempt(value => value + 1)}>{t('刷新')}</Button><Button ref={createButton} variant="primary" disabled={locked || blocked || loading || !!problem || items.length >= 64} onClick={() => edit(null)}><Icon name="plus" size={16} />{t('创建 MCP 服务')}</Button></div></div>
    {!editor && !removing && !stopping && feedback}
    <div className="server-list" aria-busy={loading}>{items.map(server => <article key={server.id} className="server-row" aria-label={server.name}>
      <div className="server-summary">
        <div className="server-title"><h2>{server.name}</h2><div className="card-address"><code title={address(server)}>{address(server)}</code><CopyButton value={address(server)} label={`${t('复制地址')} · ${server.name}`} /></div><span className="client-count" title={t('当前为无状态 HTTP，不保留客户端会话。')}>{t('客户端 {0}',[0])}</span></div>
        <div className="card-description-row">{server.description && <p className="card-description" title={server.description}>{server.description}</p>}<div className="capability-counts">{server.exposed_tools.slice(0,3).map(name=><button key={name} className="capability-link capability-tool-name" title={name} disabled={locked || blocked || loading} onClick={()=>openDetails(server,'tools')}>{name}</button>)}{server.exposed_tools.length>3&&<button className="capability-link" aria-label={t('更多工具 {0}',[server.exposed_tools.length-3])} disabled={locked || blocked || loading} onClick={()=>openDetails(server,'tools')}>+{server.exposed_tools.length-3}</button>}{(['resources','prompts'] as const).map(kind=>{const count=server.content_scope?.[kind].length??0;return count>0?<button key={kind} className="capability-link" disabled={locked || blocked || loading} onClick={()=>openDetails(server,kind)}>{t(kind==='resources'?'资源 {0}':'提示词 {0}',[count])}</button>:null;})}</div></div>
      </div>
      <div className="actions server-card-actions"><IconButton label={t(server.enabled ? '停止' : '启用')} disabled={locked || blocked || loading} onClick={() => {opener.current = document.activeElement as HTMLElement; if (!server.enabled) toggle(server, true); else {setStopping(server);setProblem(null);setMessage('');}}}><Icon name={server.enabled ? 'stop' : 'play'} size={17} /></IconButton><IconButton label={t('编辑')} disabled={locked || blocked || loading} onClick={() => edit(server)}><Icon name="edit" size={17} /></IconButton><IconButton variant="ghost" label={t('删除')} disabled={locked || blocked || loading} onClick={() => {opener.current = document.activeElement as HTMLElement;setRemoving(server);setProblem(null);setMessage('');}}><Icon name="trash" size={17} /></IconButton></div>
    </article>)}{!loading && !items.length && !problem && <div className="empty"><Icon name="layers" size={28} /><h2>{t('尚未创建 MCP 服务')}</h2><Button variant="primary" disabled={locked || blocked} onClick={() => edit(null)}>{t('创建 MCP 服务')}</Button></div>}</div>
    {details && <ServerDetails server={details.server} view={view} kind={details.mode} close={() => setDetails(null)} restoreFocus={event => { if (editor) event.preventDefault(); else restoreFocus(event); }} />}
    <AlertDialog.Root open={!!stopping} onOpenChange={open => {if (!open) closeEditor();}}><AlertDialog.Portal><AlertDialog.Overlay className="ui-overlay" /><AlertDialog.Content className="ui-dialog ui-dialog--confirm" onCloseAutoFocus={restoreFocus} onEscapeKeyDown={event => {if (busy || pending) event.preventDefault();}}>
      <div className="ui-dialog-header"><AlertDialog.Title>{t('停用服务')}</AlertDialog.Title></div><div className="ui-dialog-body"><p className="delete-server-name">{stopping?.name}</p><AlertDialog.Description>{t('停用后将停止接收新请求，已受理的操作可能完成。可随时重新启用。')}</AlertDialog.Description>{feedback}</div>
      <div className="ui-dialog-footer"><AlertDialog.Cancel asChild><Button disabled={busy || !!pending}>{t('取消')}</Button></AlertDialog.Cancel><Button variant="primary" disabled={disabled} onClick={() => {if (stopping) toggle(stopping, false);}}>{t('确认停用')}</Button></div>
    </AlertDialog.Content></AlertDialog.Portal></AlertDialog.Root>
    <Dialog.Root open={!!editor} onOpenChange={open => { if (!open) closeEditor(); }}>
      <Dialog.Portal><Dialog.Overlay className="ui-overlay" /><Dialog.Content className="ui-dialog" aria-describedby={undefined} onOpenAutoFocus={event => {event.preventDefault(); nameInput.current?.focus();}} onCloseAutoFocus={restoreFocus} onInteractOutside={event => event.preventDefault()} onEscapeKeyDown={event => { if (busy || pending) event.preventDefault(); }}>
        <div className="ui-dialog-header"><Dialog.Title>{t(editor?.existing ? '编辑 MCP 服务' : '创建 MCP 服务')}</Dialog.Title><Dialog.Close asChild><Button variant="ghost" className="ui-close" aria-label={t('关闭')} disabled={busy || !!pending}><Icon name="close" size={18} /></Button></Dialog.Close></div>
        {editor && <form className="server-editor" onSubmit={event => {event.preventDefault(); if (!disabled) save();}}>
          <div className="ui-dialog-body">
            {feedback}
            <fieldset disabled={disabled}>
              <div className="server-basics"><label className="ui-field" htmlFor="server-name"><span>{t('名称')}</span><Input ref={nameInput} id="server-name" required maxLength={120} value={name} onChange={event => setName(event.target.value)} /></label></div>
              <label className="ui-field server-description-field"><span>{t('说明')}</span><Input maxLength={200} value={description} onChange={event=>setDescription(event.target.value)} placeholder={t('说明这个服务的用途，例如：查询和控制家里的智能设备')} /></label>
              <label className="ui-field server-endpoint"><span>{t('MCP 地址')}</span><Input type="url" required maxLength={2048} value={editorAddress} onChange={event => { const value=event.target.value; setEditorAddress(value); if (auth.mode==='oauth') setAuth({...auth,resource:value}); }} /></label>
              <div className="server-protocol-fields"><div className="ui-field"><span>{t('传输方式')}</span><p className="server-transport-value">Streamable HTTP</p></div><ServerAuthFields existingMode={editor.existing?.authentication} value={auth} change={next=>setAuth(next.mode==='oauth' && !next.resource ? {...next,resource:editorAddress} : next)} /></div>
              <div className="section-bar capability-heading"><h3>{t('开放能力')}</h3></div>
              <CapabilityEditor view={view} tools={selected} scope={scope} disabled={disabled} changeTools={setSelected} changeScope={setScope} />
            </fieldset>
          </div>
          <div className="ui-dialog-footer"><Button disabled={busy || !!pending} onClick={closeEditor}>{t('取消')}</Button><Button type="submit" variant="primary" disabled={disabled || !name.trim()}>{t(busy ? '正在保存…' : '保存并生效')}</Button></div>
        </form>}
      </Dialog.Content></Dialog.Portal>
    </Dialog.Root>
    <AlertDialog.Root open={!!removing} onOpenChange={open => { if (!open) closeEditor(); }}>
      <AlertDialog.Portal><AlertDialog.Overlay className="ui-overlay" /><AlertDialog.Content className="ui-dialog ui-dialog--confirm" onCloseAutoFocus={restoreFocus} onEscapeKeyDown={event => {if (busy || pending) event.preventDefault();}}>
        <div className="ui-dialog-header"><AlertDialog.Title>{t('删除 MCP 服务')}</AlertDialog.Title></div>
        <div className="ui-dialog-body"><p className="delete-server-name">{removing?.name}</p><AlertDialog.Description>{t('删除后，该地址将停止接收新请求；已受理的操作可能完成。底层连接和历史记录保留。')}</AlertDialog.Description>{removing && <code className="delete-server-address">{address(removing)}</code>}{feedback}</div>
        <div className="ui-dialog-footer"><AlertDialog.Cancel asChild><Button disabled={busy || !!pending}>{t('取消')}</Button></AlertDialog.Cancel><Button variant="danger" disabled={disabled} onClick={() => {if (removing) void run({id:removing.id,kind:'delete',input:{operation_id:crypto.randomUUID(),expected_revision:removing.revision}});}}>{t('确认删除')}</Button></div>
      </AlertDialog.Content></AlertDialog.Portal>
    </AlertDialog.Root>
  </section>;
}
