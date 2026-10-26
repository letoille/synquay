import {useEffect,useState} from 'react';
import {t} from '../../shared/i18n';
import {Button,Checkbox,Input} from '../../shared/ui/Controls';
import type {RouteBinding,RoutingDescriptor} from './routing';

/** Renders the connector's declarative contract; no QQ/MQTT branches in the route editor. */
export function RoutingFields({descriptor,value,connectionId,onChange}:{descriptor?:RoutingDescriptor;value?:RouteBinding|null;connectionId?:string;onChange:(v:RouteBinding)=>void}){
 if(!descriptor||(descriptor.modes.length===1&&descriptor.modes[0].fields.length===0))return null;
 const current=String(value?.config.mode??(descriptor.modes.some(m=>m.id==='default')?'default':''));
 const mode=descriptor.modes.find(m=>m.id===current);
 const config=value?.config??{};
 const update=(patch:Record<string,unknown>)=>onChange({adapter:descriptor.adapter,version:descriptor.version,config:{...config,...patch}});
 return <div className="route-binding-fields">
  <label className="ui-field"><span>{t('范围')}</span><select className="ui-input" value={current} onChange={e=>{const m=descriptor.modes.find(m=>m.id===e.target.value)!;onChange({adapter:descriptor.adapter,version:descriptor.version,config:{mode:m.id,...Object.fromEntries(m.fields.map(f=>[f.key,f.control==='conversations'?[]:'']))}})}}><option value="" disabled>{t('选择范围')}</option>{descriptor.modes.map(m=><option value={m.id} key={m.id}>{t(m.label)}</option>)}</select></label>
  {mode?.fields.map(f=>f.control==='text'?<label key={f.key} className="ui-field"><span>{t(f.label)}</span><Input maxLength={512} value={typeof config[f.key]==='string'?config[f.key] as string:''} onChange={e=>update({[f.key]:e.target.value})}/></label>:<ConversationPicker key={`${connectionId}:${f.key}:${f.conversation_kind}`} label={f.label} kind={f.conversation_kind??'group'} connectionId={connectionId} multiple={f.control==='conversations'} value={f.control==='conversations'?(Array.isArray(config[f.key])?config[f.key] as string[]:[]):typeof config[f.key]==='string'&&config[f.key]?[config[f.key] as string]:[]} onChange={ids=>update({[f.key]:f.control==='conversations'?ids:ids[0]??''})}/>)}
 </div>
}
function ConversationPicker({connectionId,kind,label,multiple,value,onChange}:{connectionId?:string;kind:string;label:string;multiple:boolean;value:string[];onChange:(ids:string[])=>void}){
 const [search,setSearch]=useState(''),[page,setPage]=useState(1),[data,setData]=useState<{items:{id:string}[];total:number}|null>(null),[error,setError]=useState(false),[busy,setBusy]=useState(false),[revision,setRevision]=useState(0);
 useEffect(()=>{const abort=new AbortController();setBusy(true);setError(false);const timer=setTimeout(()=>{if(!connectionId){setError(true);setBusy(false);return;}const q=new URLSearchParams({kind,search,page:String(page),page_size:'10'});void fetch(`/api/message-connections/${encodeURIComponent(connectionId)}/conversations?${q}`,{signal:abort.signal,cache:'no-store',credentials:'omit'}).then(async r=>{if(!r.ok)throw new Error();const d=await r.json();if(!Array.isArray(d.items)||!d.items.every((i:{id:unknown})=>typeof i.id==='string')||!Number.isSafeInteger(d.total))throw new Error();if(!abort.signal.aborted)setData(d);}).catch(()=>{if(!abort.signal.aborted)setError(true)}).finally(()=>{if(!abort.signal.aborted)setBusy(false)});},200);return()=>{clearTimeout(timer);abort.abort();};},[connectionId,kind,search,page,revision]);
 return <div className="route-conversation-picker"><div className="route-picker-heading"><b>{t(label)}</b><Button onClick={()=>setRevision(r=>r+1)} disabled={busy}>{t('刷新')}</Button></div><Input aria-label={t('搜索会话')} placeholder={t('搜索 OpenID')} maxLength={128} value={search} onChange={e=>{setSearch(e.target.value);setPage(1)}}/>
 {value.length>0&&<div className="route-selected-conversations">{value.map(id=><Button key={id} title={t('移除')} onClick={()=>onChange(value.filter(v=>v!==id))}>{id} ×</Button>)}</div>}
 {error?<p role="alert" className="error">{t('无法读取会话')}</p>:busy?<small>{t('正在读取…')}</small>:!data?.items.length?<small className="muted">{t(search?'没有匹配的会话':'暂无会话，先向机器人发送一条消息。')}</small>:<div className="route-conversation-options">{data.items.map(c=><Checkbox key={c.id} checked={value.includes(c.id)} onCheckedChange={checked=>onChange(checked?(multiple?[...value.filter(v=>v!==c.id),c.id]:[c.id]):value.filter(v=>v!==c.id))}>{c.id}</Checkbox>)}</div>}
 {data&&data.total>10&&<div className="actions"><Button disabled={busy||page===1} onClick={()=>setPage(p=>p-1)}>{t('上一页')}</Button><small>{page} / {Math.ceil(data.total/10)}</small><Button disabled={busy||page*10>=data.total} onClick={()=>setPage(p=>p+1)}>{t('下一页')}</Button></div>}
 </div>
}
