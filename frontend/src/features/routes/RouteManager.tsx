import {CombinedFields} from './CombinedFields';
import {CommandSetFields} from './CommandSetFields';
import {combineRoute, combinedValid, commandSetValid} from './combined';
import {ActionTargetFields} from './ActionTargetFields';
import {changeProcessor,actionValid} from './actions';
import {RoutingFields} from './RoutingFields';
import {bindingLabel,selectRouteConnection,selectRouteTargets} from './routing';
import { CommandFields } from './CommandFields';
import { useCallback, useEffect, useRef, useState } from 'react';
import { AlertDialog, Dialog } from 'radix-ui';
import { t } from '../../shared/i18n';
import { Button, Checkbox, Input } from '../../shared/ui/Controls';
import { Icon } from '../../shared/ui/Icon';
import { IconButton } from '../../shared/ui/IconButton';
import { eventTypeLabel } from './eventLabels';
import { ControlError, errorMessage } from '../mcp-services/api';
import { endpointLabel, routeSave, routeTotals, routesApi, type Route, type RouteInput, type RoutesView, type Source, type Processor, type MessageType } from './api';

import { normalizeRoutePreferences, parseRoutePreferences, routeViewQuery, selectRouteSource, type RoutePreferences } from './preferences';

export function readPendingRoute(key:string):RouteInput|null {
  try {const raw=sessionStorage.getItem(key);if(!raw)return null;const v=JSON.parse(raw) as RouteInput;return typeof v.operation_id==='string'&&typeof v.route_id==='string'&&Number.isSafeInteger(v.expected_revision)&&['save','delete'].includes(v.change?.kind)?v:null;}catch{return null;}
}
export function RouteManager({storageKey,onBlock}:{storageKey:string;onBlock:(blocked:boolean)=>void}) {
  const [view,setView]=useState<RoutesView|null>(null),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  const [pending,setPending]=useState<RouteInput|null>(()=>readPendingRoute(storageKey));
  const [editingSection,setEditingSection]=useState<'all'|'processing'>('all');
  const [editing,setEditing]=useState<Route|null>(null),[confirm,setConfirm]=useState<{route:Route;remove:boolean}|null>(null);
  const preferenceKey=`${storageKey}:view`;
  const [preferences,setPreferences]=useState<RoutePreferences>(()=>{try{return parseRoutePreferences(localStorage.getItem(preferenceKey));}catch{return parseRoutePreferences(null);}});
  const [ready,setReady]=useState(false);
  const [commandValid,setCommandValid]=useState(true);
  const loadId=useRef(0);
  const mode=preferences.mode, query=routeViewQuery(preferences);
  const queryKey=JSON.stringify(query);
  useEffect(()=>{try{localStorage.setItem(preferenceKey,JSON.stringify(preferences));}catch{/* Preferences are optional when browser storage is unavailable. */}},[preferenceKey,preferences]);
  function setPage(page:number){setPreferences(p=>({...p,[p.mode==='list'?'listPage':'graphPage']:page}));}
  function setMode(mode:'list'|'graph'){setPreferences(p=>({...p,mode}));}
  const blocked=busy||!!pending||!!editing||!!confirm;
  useEffect(()=>{onBlock(blocked);return()=>onBlock(false);},[blocked,onBlock]);
  useEffect(()=>{if(!blocked)return;const leave=(e:BeforeUnloadEvent)=>e.preventDefault();window.addEventListener('beforeunload',leave);return()=>window.removeEventListener('beforeunload',leave);},[blocked]);
  const refresh=useCallback(async()=>{
    const id=++loadId.current;setBusy(true);setError('');
    try {
      // Read the current source catalog before sending persisted filters; old IDs may no longer exist.
      const next=await routesApi.view(ready?JSON.parse(queryKey):{});
      if(id!==loadId.current)return;
      if(!ready){setPreferences(p=>normalizeRoutePreferences(p,next.sources?.map(s=>s.connection_id)??[]));setReady(true);}
      if(ready&&next.pagination&&next.pagination.total>0&&next.pagination.page>next.pagination.total_pages){
        setPreferences(p=>({...p,[p.mode==='list'?'listPage':'graphPage']:next.pagination!.total_pages}));
      }else setView(next);
    }catch(e){if(id===loadId.current)setError(errorMessage(e));}
    finally{if(id===loadId.current)setBusy(false);}
  },[ready,queryKey]);
  useEffect(()=>{void refresh();return()=>{loadId.current++;};},[refresh]);
  useEffect(()=>{
    if(blocked||!ready)return;
    let disposed=false;let timer:ReturnType<typeof setTimeout>;
    async function poll(){
      if(!document.hidden)try {const next=await routesApi.view(JSON.parse(queryKey));if(!disposed)setView(next);}catch{/* Keep the last successful snapshot; explicit refresh reports errors. */}
      if(!disposed)timer=setTimeout(poll,5000);
    }
    timer=setTimeout(poll,5000);
    return()=>{disposed=true;clearTimeout(timer);};
  },[blocked,ready,queryKey]);
  function remember(input:RouteInput|null) {setPending(input);try{if(input)sessionStorage.setItem(storageKey,JSON.stringify(input));else sessionStorage.removeItem(storageKey);}catch{/* In-memory recovery remains available. */}}
  async function apply(input:RouteInput) {
    setBusy(true);setError('');remember(input);
    let refreshing=false;
    try {
      const receipt=await routesApi.change(input);
      if(receipt.operation_id!==input.operation_id)throw new ControlError(0,'invalid_response');
      remember(null);setEditing(null);setConfirm(null);refreshing=true;await refresh();
    }catch(e) {
      if(e instanceof ControlError&&e.status>=400&&e.status<500&&e.status!==408)remember(null);
      setError(errorMessage(e));
    }finally{if(!refreshing)setBusy(false);}
  }
  async function reconcile() {
    if(!pending)return;setBusy(true);setError('');
    let refreshing=false;
    try {const receipt=await routesApi.operation(pending.operation_id);if(receipt){remember(null);setEditing(null);setConfirm(null);refreshing=true;await refresh();}else setError('尚未确认提交，可使用原编号重试。');}catch(e){setError(errorMessage(e));}finally{if(!refreshing)setBusy(false);}
  }
  function save(route:Route,remove=false) {if(!view?.snapshot)return;void apply({operation_id:crypto.randomUUID(),expected_revision:view.snapshot.revision,route_id:route.id,change:remove?{kind:'delete'}:routeSave(route)});}
  const routes=view?.snapshot?.routes??[], targets=view?.targets??[], sources=view?.sources??[];
  const sourceFor=(r:Route)=>sources.find(s=>s.connection_id===(r.source_id??view?.source_id));
  const recovery=<section className="notice" role="status"><p>{t('操作结果待确认')}</p><div className="actions"><Button disabled={busy} onClick={()=>void reconcile()}>{t('查询发布结果')}</Button><Button disabled={busy} onClick={()=>pending&&void apply(pending)}>{t('使用原编号重试')}</Button></div></section>;
  return <>
    <div className="route-toolbar"><div className="route-view-switch" aria-label={t('路由视图')}><Button disabled={blocked} aria-pressed={mode==='list'} onClick={()=>setMode('list')}>{t('路径列表')}</Button><Button disabled={blocked} aria-pressed={mode==='graph'} onClick={()=>setMode('graph')}>{t('关系图')}</Button></div><div className="actions"><Button disabled={busy||!!pending||!!editing||!!confirm} onClick={()=>void refresh()}>{t('刷新')}</Button><Button variant="primary" disabled={!view?.available||blocked} onClick={()=>{setEditingSection('all');setEditing(combineRoute({id:crypto.randomUUID(),name:'',message_types:['message','command'],source_id:view?.supports_processors?view.source_id:undefined,enabled:true,event_type:null,revision:0,targets:[]}));}}><Icon name="plus" size={16}/>{t('新建路由')}</Button></div></div>
    {view?.available&&view.pagination&&<div className="route-source-filters" role="group" aria-label={t(mode==='list'?'入口筛选（单选）':'入口筛选（多选）')}>
      <Button aria-pressed={query.sourceIds.length===0} disabled={blocked} onClick={()=>setPreferences(p=>selectRouteSource(p,null))}>{t('全部')}</Button>
      {sources.map(source=><Button key={source.connection_id} aria-pressed={query.sourceIds.includes(source.connection_id)} disabled={blocked} onClick={()=>setPreferences(p=>selectRouteSource(p,source.connection_id))}>{source.name}</Button>)}
    </div>}
    {error&&!editing&&!confirm&&<p className="error" role="alert">{t(error)}</p>}
    {pending&&!editing&&!confirm&&recovery}
    {!view&&<p className="small muted" role="status">{t(busy?'正在读取路由…':'路由暂不可用')}</p>}
    {view&&!view.available&&<section className="empty"><span className="empty-icon"><Icon name="connections" size={28}/></span><h2>{t('尚未配置消息路径')}</h2><p>{t('先在部署配置中连接 Webhook 与 HTTP 发送端，再管理它们之间的路由。')}</p></section>}
    {view?.available&&<>
      {busy?<p className="small muted" role="status">{t('正在读取路由…')}</p>:error?null:routes.length===0?<section className="empty"><h2>{t(query.sourceIds.length?'没有符合条件的路由':'尚未添加路由')}</h2>{!query.sourceIds.length&&<p>{t('选择来源和目标，建立第一条消息路径。')}</p>}</section>:mode==='graph'?<RouteGraph view={view} edit={r=>{setEditingSection('all');setEditing(combineRoute(r));}} disabled={blocked}/>:<div className="route-list">{routes.map(route=>{
        const totals=routeTotals(route,view.counts);
        return <article key={route.id} className={`route-card ${!route.enabled?'is-paused':''}`}>
          <div className="route-card-heading"><div><h2>{route.name}</h2><span className={`route-status ${route.enabled?'is-enabled':''}`}><i/>{t(route.enabled?'已启用':'已停止')}</span></div><div className="actions server-card-actions">
            <IconButton label={t(route.enabled?'停止':'启用')} disabled={blocked} onClick={()=>route.enabled?setConfirm({route,remove:false}):save({...route,enabled:true})}><Icon name={route.enabled?'stop':'play'} size={16}/></IconButton>
            <IconButton label={t('编辑')} disabled={blocked} onClick={()=>{setEditingSection('all');setEditing(combineRoute(route));}}><Icon name="edit" size={16}/></IconButton>
            <IconButton label={t('删除')} disabled={blocked} onClick={()=>setConfirm({route,remove:true})}><Icon name="trash" size={16}/></IconButton>
          </div></div>
          <PathPreview routing={route.routing} source={sourceFor(route)} processor={route.processor} targets={route.targets.map(r=>targets.find(e=>e.destination_id===r.destination_id)).filter(e=>!!e)} filter={route.event_type} command={route.command?.trigger} onEditProcessing={blocked?undefined:()=>{setEditingSection('processing');setEditing(combineRoute(route));}}/>
          <div className="route-card-footer"><span>{t('匹配 {0}',[view.matched?.[route.id]??(view.matched?0:'—')])}</span><span>{t('已提交 {0}',[totals.submitted])}</span><span>{t('待发送 {0}',[totals.pending])}</span><span className={totals.failed?'route-failed':''}>{t('需处理 {0}',[totals.failed])}</span></div>
        </article>;
      })}</div>}
      {!busy&&!error&&view.pagination&&view.pagination.total>0&&<nav className="route-pagination" aria-label={t('路由分页')}>
        <span>{t('共 {0} 条',[view.pagination.total])}</span>
        {view.pagination.total_pages>1&&<div className="actions"><Button disabled={blocked||view.pagination.page<=1} onClick={()=>setPage(view.pagination!.page-1)}>{t('上一页')}</Button><span>{t('第 {0} / {1} 页',[view.pagination.page,view.pagination.total_pages])}</span><Button disabled={blocked||view.pagination.page>=view.pagination.total_pages} onClick={()=>setPage(view.pagination!.page+1)}>{t('下一页')}</Button></div>}
      </nav>}
      <p className="small muted route-footnote">{t('匹配数按消息累计，多出口只计一次；投递状态为保留期内记录，已提交不代表设备已执行。')}</p>
    </>}
    <Dialog.Root open={!!editing} onOpenChange={open=>{if(!open&&!busy&&!pending){setEditing(null);setError('');}}}><Dialog.Portal><Dialog.Overlay className="ui-overlay"/><Dialog.Content className="ui-dialog route-dialog" aria-describedby={undefined} onInteractOutside={e=>{if(busy||pending)e.preventDefault();}} onEscapeKeyDown={e=>{if(busy||pending)e.preventDefault();}}>
      <header className="ui-dialog-header"><Dialog.Title>{t(editingSection==='processing'?'编辑处理':editing?.revision?'编辑路由':'新建路由')}</Dialog.Title><Dialog.Close asChild><IconButton label={t('关闭')} disabled={busy||!!pending}><Icon name="close" size={18}/></IconButton></Dialog.Close></header>
      {editing&&<><div className="ui-dialog-body route-editor">
        {error&&<p className="error" role="alert">{t(error)}</p>}{pending&&recovery}
        <fieldset disabled={busy||!!pending}>
          {editingSection==='all'&&<label className="route-name"><span>{t('名称')}</span><Input maxLength={120} autoFocus value={editing.name} onChange={e=>setEditing({...editing,name:e.target.value})}/></label>}
          <div className={`route-steps ${editingSection==='processing'?'route-steps--processing':''}`}>
            {editingSection==='all'&&<section><h3><span>1</span>{t('来源')}</h3><div className="route-endpoint">{sources.length>1?<select className="ui-input" aria-label={t('来源')} value={editing.source_id??view?.source_id} onChange={e=>{const r={...editing,source_id:e.target.value,routing:selectRouteConnection(editing.routing)};const qq=sources.find(s=>s.connection_id===e.target.value)?.protocol==='QQ';setEditing(r.processor?.kind==='command_set'&&!qq?{...r,processor:{kind:'combined',forward:true,action:null,reply:null},targets:r.targets.filter(t=>t.branch==='forward'),command:null}:r.processor?.kind==='combined'&&!qq?{...r,processor:{...r.processor,action:null,reply:null,forward:true},targets:r.targets.filter(t=>t.branch==='forward')}:r.processor&&(r.processor.kind==='invoke_capability'||r.processor.kind==='respond')?changeProcessor(r,qq?r.processor.kind:'forward',targets):r);}}>{sources.map(s=><option key={s.connection_id} value={s.connection_id}>{s.name}</option>)}</select>:<b>{sourceFor(editing)?.name??'Webhook'}</b>}<small className="mono">{sourceFor(editing)?.address}</small></div><RoutingFields descriptor={sourceFor(editing)?.routing} connectionId={sourceFor(editing)?.connection_id} value={editing.routing?.source} onChange={source=>setEditing({...editing,routing:{source,targets:editing.routing?.targets??{}}})}/></section>}
            {editing.processor?.kind==='command_set'?<CommandSetFields route={editing} targets={targets} onChange={setEditing} onValidity={setCommandValid}/>:editing.processor?.kind==='combined'?<CombinedFields route={editing} source={sourceFor(editing)} targets={targets} onChange={setEditing} onValidity={setCommandValid}/>:<>            <section><h3><span>2</span>{t('处理')}</h3><div className="route-processing">{view?.supports_processors?<select className="ui-input" aria-label={t('处理')} value={editing.processor?.kind??'forward'} onChange={e=>setEditing(changeProcessor(editing,e.target.value as Exclude<Processor['kind'],'combined'|'command_set'>,targets))}><option value="forward">{t('转发')}</option>{sourceFor(editing)?.protocol==='QQ'&&targets.some(t=>t.actions?.length)&&<><option value="respond">{t('直接回复')}</option><option value="invoke_capability">{t('调用能力')}</option></>}</select>:t('转发')}{editing.processor?.kind==='invoke_capability'?null:<small>{t(editing.command?.output.kind==='json'?'按指令映射生成消息。':'保留事件类型、标识和原始内容。')}</small>}</div>{view?.supports_commands&&<div className="command-settings"><Checkbox disabled={editing.processor?.kind==='invoke_capability'||editing.processor?.kind==='respond'} checked={!!editing.command} onCheckedChange={checked=>{setCommandValid(true);setEditing({...editing,command:checked?{text_pointer:'/content',trigger:'',mode:'prefix',output:{kind:'original'}}:null});}}>{t('指令映射')}</Checkbox>{editing.command&&<CommandFields response={editing.processor?.kind==='respond'} action={editing.processor?.kind==='invoke_capability'||editing.processor?.kind==='respond'} key={`${editing.id}:${editing.processor?.kind==='invoke_capability'?editing.processor.tool_id:''}`} value={editing.command} onValidity={setCommandValid} onChange={command=>setEditing({...editing,command})}/>}</div>}<details><summary>{t('筛选条件')}</summary><div className="route-message-types"><span>{t('消息类型')}</span><small>{t('至少选择一种消息类型')}</small><small>{t('指令由当前来源已启用的指令规则识别')}</small>{([['message','普通消息'],['state','状态上报'],['command','指令消息']] as [MessageType,string][]).map(([type,label])=><Checkbox key={type} checked={editing.message_types?.includes(type)??false} onCheckedChange={checked=>{const next=checked?[...(editing.message_types??[]),type]:(editing.message_types??[]).filter(v=>v!==type);setEditing({...editing,message_types:next});}}>{t(label)}</Checkbox>)}</div><label>{t('事件类型')}<Input maxLength={128} placeholder={t('留空表示全部')} value={editing.event_type??''} onChange={e=>setEditing({...editing,event_type:e.target.value||null})}/></label></details></section>
            <section><h3><span>3</span>{t('目标')}</h3>{editing.processor?.kind==='respond'?<p>{t('回复来源会话')}</p>:editing.processor?.kind==='invoke_capability'?<ActionTargetFields route={editing} targets={targets} onChange={setEditing}/>:<div className="route-target-picker">{targets.map(target=><div key={target.destination_id}><Checkbox checked={editing.targets.some(t=>t.destination_id===target.destination_id)} onCheckedChange={checked=>{const chosen=checked?[...editing.targets,{rule_id:'',destination_id:target.destination_id}]:editing.targets.filter(t=>t.destination_id!==target.destination_id);setEditing({...editing,targets:chosen,routing:selectRouteTargets(editing.routing,chosen.map(t=>t.destination_id))})}} description={<span className="mono">{target.address}</span>}>{target.name || target.protocol || 'HTTP'} · {t('发送')}</Checkbox>{editing.targets.some(t=>t.destination_id===target.destination_id)&&<RoutingFields descriptor={target.routing} connectionId={target.owner_connection_id} value={editing.routing?.targets[target.destination_id]} onChange={binding=>setEditing({...editing,routing:{source:editing.routing?.source??null,targets:{...editing.routing?.targets,[target.destination_id]:binding}}})}/>}</div>)}</div>}</section>
            </>}
          </div>
          {(editing.processor?.kind==='invoke_capability'||editing.processor?.kind==='respond')&&<section className="route-response"><h3>{t('回复')}</h3><Checkbox checked={editing.response?.kind==='source'} disabled={editing.processor.kind==='respond'||sourceFor(editing)?.protocol!=='QQ'} onCheckedChange={checked=>setEditing({...editing,response:{kind:checked===true?'source':'none'}})}>{t('回复原会话')}</Checkbox></section>}
          <div className="route-preview"><PathPreview routing={editing.routing} source={sourceFor(editing)} processor={editing.processor} targets={targets.filter(t=>editing.targets.some(e=>e.destination_id===t.destination_id))} filter={editing.event_type} command={editing.command?.trigger}/></div>
        </fieldset>
      </div><footer className="ui-dialog-footer"><Button disabled={busy||!!pending} onClick={()=>{setEditing(null);setError('');}}>{t('取消')}</Button><Button variant="primary" disabled={busy||!!pending||!editing.name.trim()||!editing.message_types?.length||!commandValid||(editing.processor?.kind==='command_set'?!commandSetValid(editing):editing.processor?.kind==='combined'?!combinedValid(editing):!editing.targets.length||!actionValid(editing))} onClick={()=>save(editing)}>{t('保存')}</Button></footer></>}
    </Dialog.Content></Dialog.Portal></Dialog.Root>
    <AlertDialog.Root open={!!confirm} onOpenChange={open=>{if(!open&&!busy&&!pending){setConfirm(null);setError('');}}}><AlertDialog.Portal><AlertDialog.Overlay className="ui-overlay"/><AlertDialog.Content className="ui-dialog ui-dialog--confirm"><header className="ui-dialog-header"><AlertDialog.Title>{t(confirm?.remove?'删除路由':'停止路由')}</AlertDialog.Title></header><div className="ui-dialog-body"><p><strong>{confirm?.route.name}</strong></p><AlertDialog.Description>{t((confirm?.route.processor?.kind==='invoke_capability'||confirm?.route.processor?.kind==='command_set'||confirm?.route.processor?.kind==='combined'&&!!confirm.route.processor.action)?'未开始的能力调用将停止，已完成的调用不会撤销。':'仅影响尚未分发的消息，已进入发送队列的消息继续处理。恢复启用不会重放已处理的消息。')}</AlertDialog.Description>{error&&<p className="error" role="alert">{t(error)}</p>}{pending&&recovery}</div><footer className="ui-dialog-footer"><AlertDialog.Cancel asChild><Button disabled={busy||!!pending}>{t('取消')}</Button></AlertDialog.Cancel><Button variant={confirm?.remove?'danger':'primary'} disabled={busy||!!pending} onClick={()=>confirm&&save({...confirm.route,enabled:false},confirm.remove)}>{t(confirm?.remove?'删除':'停止')}</Button></footer></AlertDialog.Content></AlertDialog.Portal></AlertDialog.Root>
  </>;
}
function PathPreview({source,processor,targets,filter,command,routing,onEditProcessing}:{routing?:Route['routing'];source?:Source;processor?:Processor;targets:NonNullable<RoutesView['targets']>;filter:string|null;command?:string;onEditProcessing?:()=>void}) {
  return <div className="route-path"><span className="route-path-node"><Icon name="connections" size={16}/><span>{source?.name??'Webhook'}<small>{t(bindingLabel(routing?.source)??'接收')}</small></span></span><Icon name="arrow" size={18}/><span className={`route-path-process ${processor&&processor.kind!=='forward'?'route-agent':''}`}>{processor&&processor.kind!=='forward'?<><Icon name="agent" size={16}/>{processor.kind==='command_set'?t('指令 {0}',[processor.commands.length]):processor.kind==='combined'?[processor.forward?t('转发'):null,processor.action?t('调用能力'):null,processor.reply?t('回复'):null].filter(Boolean).join(' · '):processor.kind==='invoke_capability'?t('调用能力'):t('直接回复')}<small>{processor.kind==='combined'||processor.kind==='command_set'?null:processor.kind==='invoke_capability'?t(targets.flatMap(t=>t.actions??[]).find(a=>a.definition.id===processor.tool_id)?.definition.description??processor.tool_id):t('回复原会话')}</small></>:t('转发')}{command&&<small className="mono" title={t('指令映射')}>{command}</small>}{filter&&<small title={filter}>{eventTypeLabel(filter)}</small>}{onEditProcessing&&<button type="button" className="route-process-edit" onClick={onEditProcessing}>{t('编辑处理')}</button>}</span><Icon name="arrow" size={18}/><div className="route-path-targets">{targets.length?targets.slice(0,3).map(target=><span className="route-path-node" key={`${target.destination_id}-${targets.indexOf(target)}`} title={target.address}><span className="route-protocol">{target.protocol??'HTTP'}</span><span className="route-address">{bindingLabel(routing?.targets[target.destination_id])?t(bindingLabel(routing?.targets[target.destination_id])!):endpointLabel(target)}</span></span>):<span className="muted">{t('选择目标')}</span>}{targets.length>3&&<small>+{targets.length-3}</small>}</div></div>;
}
function RouteGraph({view,edit,disabled}:{view:RoutesView;edit:(r:Route)=>void;disabled:boolean}) {
  const routes=view.snapshot?.routes??[];
  const targetKey=(route:Route,target:Route['targets'][number])=>`${target.destination_id}:${target.branch==='action'||route.processor?.kind==='invoke_capability'?'action':'send'}:${JSON.stringify(route.routing?.targets[target.destination_id]??null)}`;
  const targets=Array.from(new Map(routes.flatMap(route=>route.targets.flatMap(t=>{const endpoint=view.targets?.find(e=>e.destination_id===t.destination_id);return endpoint?[[targetKey(route,t),{...endpoint,isAction:t.branch==='action'||route.processor?.kind==='invoke_capability',graphKey:targetKey(route,t),scopeLabel:bindingLabel(route.routing?.targets[t.destination_id])}] as const]:[]}))).values());
  const sources=(view.sources??[]).filter(source=>routes.some(r=>(r.source_id??view.source_id)===source.connection_id));
  const height=Math.max(routes.length,targets.length,sources.length,2)*94+48;
  const y=(i:number,count:number)=>((i+1)/(count+1))*height;
  return <div className="route-graph-scroll"><div className="route-graph" style={{height}}>
    <svg viewBox={`0 0 1000 ${height}`} preserveAspectRatio="none" aria-hidden="true">
      <defs><marker id="route-flow-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="5" markerHeight="5" orient="auto"><polygon points="0,0 10,5 0,10"/></marker></defs>
      {routes.flatMap((route,i)=>{
        const origin=sources.findIndex(s=>s.connection_id===(route.source_id??view.source_id));
        return [<path key={`${route.id}-in`} markerEnd="url(#route-flow-arrow)" className={route.enabled?'active':''} d={`M210 ${y(origin,sources.length)} C290 ${y(origin,sources.length)},310 ${y(i,routes.length)},398 ${y(i,routes.length)}`}/>,
          ...route.targets.map(target=>{const j=targets.findIndex(e=>e.graphKey===targetKey(route,target));return <path key={`${route.id}-${target.rule_id}`} markerEnd="url(#route-flow-arrow)" className={route.enabled?'active':''} d={`M600 ${y(i,routes.length)} C680 ${y(i,routes.length)},680 ${y(j,targets.length)},768 ${y(j,targets.length)}`}/>;})];
      })}
    </svg>
    {sources.map((s,i)=><div key={s.connection_id} className="route-graph-node" style={{left:'1%',top:y(i,sources.length)}} title={s.address}><b>{s.name}</b><small>{s.protocol??'Webhook'} · {t('接收')}</small></div>)}
    {routes.map((r,i)=><button key={r.id} className={`route-graph-node route-graph-rule ${r.processor&&r.processor.kind!=='forward'?'route-graph-agent':''} ${!r.enabled?'is-paused':''}`} style={{left:'40%',top:y(i,routes.length)}} disabled={disabled} onClick={()=>edit(r)}><b>{r.processor&&r.processor.kind!=='forward'&&<Icon name="agent" size={15}/>} {r.name}</b>{bindingLabel(r.routing?.source)&&<small>{t(bindingLabel(r.routing?.source)!)}</small>}{r.command&&<small className="mono">{r.command.trigger}</small>}<small>{r.processor&&r.processor.kind!=='forward'?`${r.processor.kind==='command_set'?t('指令 {0}',[r.processor.commands.length]):r.processor.kind==='combined'?[r.processor.forward?t('转发'):null,r.processor.action?t('调用能力'):null,r.processor.reply?t('回复'):null].filter(Boolean).join(' · '):r.processor.kind==='invoke_capability'?t('调用能力'):t('直接回复')} · ${t(r.enabled?'已启用':'已停止')}`:`${t(r.enabled?'已启用':'已停止')} · ${t('转发')}`}</small><small>{t('匹配 {0}',[view.matched?.[r.id]??(view.matched?0:'—')])}</small></button>)}
    {targets.map((target,i)=><div key={target.graphKey} className="route-graph-node" style={{left:'77%',top:y(i,targets.length)}} title={target.address}><b>{target.protocol??'HTTP'} · {t(target.isAction?'执行':'发送')}</b><small>{target.scopeLabel?t(target.scopeLabel):endpointLabel(target)}</small></div>)}
  </div></div>;
}
