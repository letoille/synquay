import { useEffect,useRef,useState } from 'react';
import { Dialog,AlertDialog } from 'radix-ui';
import { Button,Input } from '../../shared/ui/Controls';
import { Icon } from '../../shared/ui/Icon';
import { IconButton } from '../../shared/ui/IconButton';
import { CopyButton } from '../../shared/ui/CopyButton';
import { t } from '../../shared/i18n';
import { Field,ConnectionFields } from './ConnectionFields';
import {ConnectionFormContext} from './FormField';
import {validateConnection,fieldMessages,formMessages,type FieldErrors} from './validation';
import { request,defaults,address,readPending,rememberPending,ConnectionError } from './api';
import type { Config,Connection,Snapshot,Input as ChangeInput } from './api';
export function MessageConnections({storageKey,locked,onBlock}:{storageKey:string;locked:boolean;onBlock:(v:boolean)=>void}) {
 const formRef=useRef<HTMLFormElement>(null);
 const [touched,setTouched]=useState<Set<string>>(()=>new Set()),[submitted,setSubmitted]=useState(false),[serverFields,setServerFields]=useState<FieldErrors>({});
 const [secretValue,setSecretValue]=useState('');
 const [view,setView]=useState<Snapshot|null>(null),[available,setAvailable]=useState(true),[error,setError]=useState('');
 const [editing,setEditing]=useState<Connection|null>(null),[removing,setRemoving]=useState<Connection|null>(null),[busy,setBusy]=useState(false);
 const [pending,setPending]=useState<ChangeInput|null>(()=>readPending(storageKey));
 const gate=useRef(false),opener=useRef<HTMLElement|null>(null);
 const blocked=busy||!!editing||!!removing||!!pending;
 const original=view?.connections.find(c=>c.id===editing?.id);
 const clientFields=editing?validateConnection(editing,secretValue,original):{};
 const visibleFields:FieldErrors={...Object.fromEntries(Object.entries(clientFields).filter(([key])=>submitted||touched.has(key))),...serverFields};
 function focusInvalid(){requestAnimationFrame(()=>{const field=formRef.current?.querySelector<HTMLElement>('[aria-invalid="true"], [data-field-group]');field?.focus();field?.scrollIntoView({block:'nearest',behavior:'smooth'});});}
 function resetValidation(){setSubmitted(false);setTouched(new Set());setServerFields({});}
 function submit(){if(!editing)return;setSubmitted(true);setError('');setServerFields({});if(Object.keys(clientFields).length){focusInvalid();return;}save(editing,editing.id,editing.config.kind==='qq_bot'?secretValue:undefined);}
 function changeConfig(config:Config){if(!editing)return;setServerFields(fields=>Object.fromEntries(Object.entries(fields).filter(([path])=>{if(!path.startsWith('config.'))return true;const read=(value:unknown)=>path.slice(7).split('.').reduce<unknown>((v,key)=>v&&typeof v==='object'?(v as Record<string,unknown>)[key]:undefined,value);return JSON.stringify(read(editing.config))===JSON.stringify(read(config));})));setEditing({...editing,config});}
 function clearField(path:string){setServerFields(fields=>{const next={...fields};delete next[path];return next;});}

 useEffect(()=>{onBlock(blocked);return()=>onBlock(false);},[blocked,onBlock]);
 async function refresh(){try{const data=await request();setAvailable(data.available===true);setView(data.snapshot??null);}catch(e){if(e instanceof ConnectionError&&e.status===404){setAvailable(false);setView(null);}else setError('读取连接失败，请刷新重试。');}}
 useEffect(()=>{void refresh();},[]);
 useEffect(()=>{if(!blocked)return;const fn=(e:BeforeUnloadEvent)=>e.preventDefault();window.addEventListener('beforeunload',fn);return()=>window.removeEventListener('beforeunload',fn);},[blocked]);
 async function send(input:ChangeInput){
   if(gate.current)return;gate.current=true;setBusy(true);setError('');
   try{
     rememberPending(storageKey,input);setPending(input);
     await request(input);sessionStorage.removeItem(storageKey);setPending(null);setSecretValue('');setEditing(null);setRemoving(null);await refresh();
   }catch(e){
     if(e instanceof ConnectionError&&[400,401,403,404,409,413,415,429].includes(e.status)){sessionStorage.removeItem(storageKey);setPending(null);const fields=Object.fromEntries(e.fields.filter(f=>fieldMessages[f.code]).map(f=>[f.path,fieldMessages[f.code]]));setServerFields(fields);if(Object.keys(fields).length&&editing){setError('');setSubmitted(true);focusInvalid();}else setError(formMessages[e.code]??fieldMessages[e.code]??'保存失败，请稍后重试。');if(e.status===409)await refresh();}
     else setError('尚未确认提交，可使用原编号重试。');
   }finally{gate.current=false;setBusy(false);}
 }
 function edit(c:Connection|null){opener.current=document.activeElement as HTMLElement;setError('');resetValidation();setSecretValue('');setEditing(c??{id:crypto.randomUUID(),name:'',description:'',enabled:false,config:defaults('mqtt')});}
 function save(c:Connection|null,id:string,secret?:string){if(c?.config.kind==='qq_bot')c={...c,config:{...c.config,allowed_senders:c.config.allowed_senders.map(s=>s.trim()).filter(Boolean)}};if(view)void send({operation_id:crypto.randomUUID(),expected_revision:view.revision,connection_id:id,connection:c,...(secret?{secret}:{})});}
 function close(){if(!busy&&!pending){setEditing(null);setSecretValue('');setRemoving(null);setError('');}}
 function focus(e:Event){e.preventDefault();opener.current?.isConnected&&opener.current.focus();}
 const protocol=(c:Connection)=>c.config.kind==='qq_bot'?'QQ Bot':c.config.kind==='mqtt'?'MQTT Client':c.config.kind==='webhook'?'Webhook':'HTTP Client';
 const flow=(c:Connection)=>c.config.kind==='qq_bot'?`${t('接收')} / ${t('发送')}`:c.config.kind==='webhook'?t('接收'):c.config.kind==='http_client'?t('发送'):[c.config.subscription?t('接收'):'',c.config.publish?t('发送'):''].filter(Boolean).join(' / ');
 return <section className="message-connections" aria-label={t('消息连接')}>
  <div className="section-bar server-list-actions"><span className="muted small">{t('消息连接')}{view?` · ${view.connections.length}`:''}</span><div className="actions"><Button disabled={locked||blocked} onClick={()=>{setError('');void refresh();}}>{t('刷新')}</Button><Button variant="primary" disabled={locked||blocked||!available||!view||view.connections.length>=32} onClick={()=>edit(null)}><Icon name="plus" size={16}/>{t('添加连接')}</Button></div></div>
  {!available&&<p className="notice">{t('消息连接接口尚不可用，请重启新版后端。')}</p>}
  {!editing&&!pending&&!removing&&error&&<p className="error" role="alert">{t(error)}</p>}
  <div className="server-list">{view?.connections.map(c=><article className="server-row connection-row" key={c.id}>
   <div className="server-summary"><div className="server-title"><h2>{c.name}</h2><span className="connection-protocol">{protocol(c)} · {flow(c)}</span><div className="card-address"><code title={address(c)}>{address(c)}</code><CopyButton value={address(c)} label={`${t('复制地址')} · ${c.name}`}/></div><span className={`server-status ${c.enabled?'is-enabled':''}`}><i/>{t(c.enabled?'已启用':'已停止')}</span></div><div className="card-description-row"><p className="card-description">{c.description|| (c.config.kind==='mqtt'?[c.config.subscription?`SUB ${c.config.subscription.topic}`:'',c.config.publish?`PUB ${c.config.publish.topic}`:''].filter(Boolean).join(' · '):'')}</p></div></div>
   <div className="actions server-card-actions"><IconButton label={`${t(c.enabled?'停止':'启用')} · ${c.name}`} disabled={locked||blocked} onClick={()=>save({...c,enabled:!c.enabled},c.id)}><Icon name={c.enabled?'stop':'play'} size={17}/></IconButton><IconButton label={`${t('编辑')} · ${c.name}`} disabled={locked||blocked} onClick={()=>edit(c)}><Icon name="edit" size={17}/></IconButton><IconButton label={`${t('删除')} · ${c.name}`} disabled={locked||blocked} onClick={()=>{opener.current=document.activeElement as HTMLElement;setError('');setRemoving(c);}}><Icon name="trash" size={17}/></IconButton></div>
  </article>)}{available&&view&&!view.connections.length&&<p className="empty">{t('添加消息连接，再到路由中选择来源和目标。')}</p>}</div>
  <Dialog.Root open={!!editing||!!pending} onOpenChange={open=>{if(!open)close();}}><Dialog.Portal><Dialog.Overlay className="ui-overlay"/><Dialog.Content className="ui-dialog" aria-describedby={undefined} onCloseAutoFocus={focus} onInteractOutside={e=>e.preventDefault()} onEscapeKeyDown={e=>{if(busy||pending)e.preventDefault();}}>
   <div className="ui-dialog-header"><Dialog.Title>{t(pending?'操作结果待确认':view?.connections.some(c=>c.id===editing?.id)?'编辑连接':'添加连接')}</Dialog.Title><Button variant="ghost" className="ui-close" aria-label={t('关闭')} disabled={busy||!!pending} onClick={close}><Icon name="close" size={18}/></Button></div>
   {pending?<div className="ui-dialog-body">{error&&<p className="error" role="alert">{t(error)}</p>}<p className="mono small">{pending.operation_id}</p><Button disabled={busy} onClick={()=>void send(pending)}>{t('使用原编号重试')}</Button></div>:editing&&<form ref={formRef} noValidate className="server-editor connection-editor" onSubmit={e=>{e.preventDefault();submit();}}><div className="ui-dialog-body">{error&&<p className="error" role="alert">{t(error)}</p>}{Object.keys(visibleFields).length>0&&<p className="form-validation-summary" role="alert">{t('请检查标红的输入项。')}</p>}<ConnectionFormContext.Provider value={{errors:visibleFields,touch:name=>setTouched(previous=>new Set(previous).add(name)),clear:clearField}}><fieldset disabled={busy||locked}>
    <Field name="name" label="名称"><Input required maxLength={120} value={editing.name} onChange={e=>setEditing({...editing,name:e.target.value})} autoFocus/></Field><Field name="description" label="说明"><Input maxLength={4000} value={editing.description} onChange={e=>setEditing({...editing,description:e.target.value})}/></Field>
    <Field label="连接类型"><select className="ui-input" value={editing.config.kind} onChange={e=>{resetValidation();setError('');setSecretValue('');setEditing({...editing,secret_configured:false,config:defaults(e.target.value as Config['kind'])});}}><option value="qq_bot">QQ Bot</option><option value="mqtt">MQTT Client</option><option value="webhook">Webhook</option><option value="http_client">HTTP Client</option></select></Field>
    {(editing.config.kind==='webhook'||editing.config.kind==='qq_bot')&&<div className="ui-field"><span>{t('接收地址')}</span><div className="card-address"><code>{address(editing)}</code><CopyButton value={address(editing)} label={t('复制地址')}/></div></div>}
    <ConnectionFields secretValue={secretValue} onSecretChange={setSecretValue} secretConfigured={original?.secret_configured===true&&original.config.kind==='qq_bot'&&editing.config.kind==='qq_bot'&&original.config.app_id===editing.config.app_id} config={editing.config} change={changeConfig}/>
   </fieldset></ConnectionFormContext.Provider></div><div className="ui-dialog-footer"><Button disabled={busy} onClick={close}>{t('取消')}</Button><Button type="submit" variant="primary" disabled={busy||locked}>{t('保存连接')}</Button></div></form>}
  </Dialog.Content></Dialog.Portal></Dialog.Root>
  <AlertDialog.Root open={!!removing&&!pending} onOpenChange={open=>{if(!open)close();}}><AlertDialog.Portal><AlertDialog.Overlay className="ui-overlay"/><AlertDialog.Content className="ui-dialog ui-dialog--confirm" onCloseAutoFocus={focus}><div className="ui-dialog-header"><AlertDialog.Title>{t('删除连接')}</AlertDialog.Title></div><div className="ui-dialog-body"><p>{removing?.name}</p><AlertDialog.Description>{t('同时移除路由中的引用；没有目标的路由一并删除。历史消息保留。')}</AlertDialog.Description>{error&&<p className="error" role="alert">{t(error)}</p>}</div><div className="ui-dialog-footer"><AlertDialog.Cancel asChild><Button disabled={busy}>{t('取消')}</Button></AlertDialog.Cancel><Button variant="danger" disabled={busy||locked} onClick={()=>{if(removing)save(null,removing.id);}}>{t('确认删除')}</Button></div></AlertDialog.Content></AlertDialog.Portal></AlertDialog.Root>
 </section>;
}
