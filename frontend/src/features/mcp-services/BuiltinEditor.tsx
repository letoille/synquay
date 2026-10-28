import { useEffect, useState } from 'react';
import { t } from '../../shared/i18n';
import { request, type Connection } from '../message-connections/api';
import type { BuiltinScope, BuiltinTool, ConversationGrant } from './types';

const options: {id:BuiltinTool; label:string}[] = [
  {id:'connections_list',label:'查询连接'}, {id:'connection_status',label:'查询连接状态'},
  {id:'connection_statistics',label:'查询收发数量'}, {id:'recent_messages',label:'查询近期消息'},
  {id:'qq_bot_info',label:'查询 QQ 机器人信息'}, {id:'qq_send_text',label:'发送 QQ 文本消息'},
];
export function BuiltinEditor({value,change}:{value:BuiltinScope;change:(value:BuiltinScope)=>void}) {
  const [connections,setConnections]=useState<Connection[]>([]),[error,setError]=useState(false);
  useEffect(()=>{let active=true;void request().then(result=>{if(active){if(!result.available||!result.snapshot)throw new Error();setConnections(result.snapshot.connections.filter(c=>c.enabled));setError(false);}}).catch(()=>{if(active)setError(true);});return()=>{active=false;};},[]);
  function selectTool(id:BuiltinTool,checked:boolean){change({...value,tools:checked?[...value.tools,id]:value.tools.filter(x=>x!==id)});}
  function selectConnection(id:string,checked:boolean){change({...value,connections:checked?[...value.connections,id]:value.connections.filter(x=>x!==id),conversations:checked?value.conversations:value.conversations.filter(x=>x.connection_id!==id),qq_send_conversations:checked?value.qq_send_conversations:value.qq_send_conversations.filter(x=>x.connection_id!==id)});}
  return <section className="builtin-editor"><h3>{t('内置工具')}</h3>
    <div>{options.map(item=><label key={item.id} className="builtin-choice"><input type="checkbox" checked={value.tools.includes(item.id)} onChange={e=>selectTool(item.id,e.target.checked)}/>{t(item.label)}</label>)}</div>
    {value.tools.length>0&&<><h4>{t('允许使用的连接')}</h4>{error?<p role="alert">{t('读取连接失败，请稍后重试。')}</p>:connections.length===0?<p className="muted">{t('暂无可用连接')}</p>:connections.map(c=><div key={c.id}><label className="builtin-choice"><input type="checkbox" checked={value.connections.includes(c.id)} disabled={!value.connections.includes(c.id)&&value.connections.length>=32} onChange={e=>selectConnection(c.id,e.target.checked)} />{c.name}</label>{value.tools.includes('recent_messages')&&value.connections.includes(c.id)&&c.config.kind==='qq_bot'&&<Conversations connection={c} value={value} change={change}/>}{value.tools.includes('qq_send_text')&&value.connections.includes(c.id)&&c.config.kind==='qq_bot'&&(c.config.allow_proactive?<Conversations connection={c} value={value} change={change} send/>:<p className="muted small">{t('请先在 QQ 连接中启用主动发送。')}</p>)}{value.tools.includes('recent_messages')&&value.connections.includes(c.id)&&(c.config.kind==='webhook'||c.config.kind==='mqtt'&&!!c.config.subscription)&&<StreamGrant connection={c} value={value} change={change}/>}</div>)}</>}
  </section>;
}
function Conversations({connection,value,change,send=false}:{connection:Connection;value:BuiltinScope;change:(value:BuiltinScope)=>void;send?:boolean}) {
  const [search,setSearch]=useState(''),[items,setItems]=useState<ConversationGrant[]>([]),[names,setNames]=useState<Record<string,string>>({}),[failed,setFailed]=useState(false);
  const appId=connection.config.kind==='qq_bot'?connection.config.app_id:'';
  useEffect(()=>{const controller=new AbortController();const timer=setTimeout(()=>{void Promise.all((['group','direct'] as const).map(async kind=>{const query=new URLSearchParams({kind,search,page:'1',page_size:'100'});const response=await fetch(`/api/message-connections/${encodeURIComponent(connection.id)}/conversations?${query}`,{cache:'no-store',credentials:'omit',signal:controller.signal});if(!response.ok)throw new Error();const page=await response.json();if(!Array.isArray(page.items)||!page.items.every((x:{id:unknown})=>typeof x.id==='string'))throw new Error();return page.items.map((x:{id:string;display_name?:string|null})=>({grant:{connection_id:connection.id,account:appId,kind,conversation_id:x.id} as ConversationGrant,name:typeof x.display_name==='string'?x.display_name:''}));})).then(pages=>{const all=pages.flat();setItems(all.map(x=>x.grant));setNames(old=>({...old,...Object.fromEntries(all.filter(x=>x.name).map(x=>[`${x.grant.kind}:${x.grant.conversation_id}`,x.name]))}));setFailed(false);}).catch(()=>{if(!controller.signal.aborted)setFailed(true);});},200);return()=>{clearTimeout(timer);controller.abort();};},[connection.id,appId,search]);
  const grants=send?value.qq_send_conversations:value.conversations;
  const selected=grants.filter(c=>c.connection_id===connection.id);
  const combined=[...selected,...items.filter(c=>!selected.some(s=>s.kind===c.kind&&s.conversation_id===c.conversation_id))];
  function toggle(grant:ConversationGrant,checked:boolean){const next=checked?[...grants,grant]:grants.filter(x=>!(x.connection_id===grant.connection_id&&x.account===grant.account&&x.kind===grant.kind&&x.conversation_id===grant.conversation_id));change(send?{...value,qq_send_conversations:next}:{...value,conversations:next});}
  return <div className="builtin-conversations"><p className="muted small">{t(send?'选择允许发送的群或单聊':'选择允许查询的群或单聊')}</p><input aria-label={t('搜索会话')} value={search} onChange={e=>setSearch(e.target.value)} placeholder={t('搜索会话')} />{failed&&<p role="alert">{t('读取会话失败')}</p>}{combined.map(c=><label className="builtin-choice" key={`${c.account}:${c.kind}:${c.conversation_id}`}><input type="checkbox" checked={selected.some(x=>x.account===c.account&&x.kind===c.kind&&x.conversation_id===c.conversation_id)} disabled={!selected.some(x=>x.account===c.account&&x.kind===c.kind&&x.conversation_id===c.conversation_id)&&grants.length>=100} onChange={e=>toggle(c,e.target.checked)}/>{t(c.kind==='group'?'群':'单聊')} · {names[`${c.kind}:${c.conversation_id}`]?`${names[`${c.kind}:${c.conversation_id}`]} · `:''}{c.conversation_id}</label>)}</div>;
}

function StreamGrant({connection,value,change}:{connection:Connection;value:BuiltinScope;change:(value:BuiltinScope)=>void}) {
  const grant:ConversationGrant={connection_id:connection.id,account:'connection',kind:'stream',conversation_id:'default'};
  const selected=value.conversations.some(item=>item.connection_id===connection.id&&item.kind==='stream');
  return <label className="builtin-choice"><input type="checkbox" checked={selected} disabled={!selected&&value.conversations.length>=100} onChange={event=>change({...value,conversations:event.target.checked?[...value.conversations,grant]:value.conversations.filter(item=>!(item.connection_id===connection.id&&item.kind==='stream'))})}/>{t('允许读取此连接的全部近期消息')}</label>;
}
