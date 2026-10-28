import {useCallback, useEffect, useState} from 'react';
import {t} from '../../shared/i18n';
import {Button, Checkbox, Input} from '../../shared/ui/Controls';
import {ActionTargetFields} from './ActionTargetFields';
import {RoutingFields} from './RoutingFields';
import {selectRouteTargets} from './routing';
import type {CombinedAction, CommandSetProcessor, Endpoint, Route, RouteCommand} from './api';

const emptyAction=():CombinedAction=>({destination_id:'',tool_id:'',revision:1,allowed_senders:[],arguments:{}});
const emptyCommand=():RouteCommand=>({id:crypto.randomUUID(),mapping:{text_pointer:'/content',trigger:'',mode:'exact',output:{kind:'original'}},action:emptyAction(),reply:null});
export function CommandSetFields({route,targets,onChange,onValidity}:{route:Route;targets:Endpoint[];onChange:(r:Route)=>void;onValidity:(v:boolean)=>void}) {
  const p=route.processor as CommandSetProcessor;
  const [invalid,setInvalid]=useState<Record<string,boolean>>({});
  useEffect(()=>{onValidity(!Object.values(invalid).some(Boolean));return()=>onValidity(true)},[invalid,onValidity]);
  const validity=useCallback((id:string,valid:boolean)=>setInvalid(old=>old[id]===!valid?old:{...old,[id]:!valid}),[]);
  const update=(patch:Partial<CommandSetProcessor>,other:Partial<Route>={})=>onChange({...route,...other,processor:{...p,...patch}});
  const updateCommand=(id:string,command:RouteCommand,routing=route.routing)=>update({commands:p.commands.map(c=>c.id===id?command:c)},{routing});
  const forwards=route.targets.filter(v=>v.branch==='forward');
  return <>
    <section><h3>{t('处理')}</h3><div className="command-fields">
      <Checkbox checked={p.forward} onCheckedChange={checked=>update({forward:!!checked},checked?{}:{targets:route.targets.filter(v=>v.branch!=='forward')})}>{t('转发')}</Checkbox>
      <small>{t('按顺序匹配，只执行第一条匹配的指令。')}</small>
      <details><summary>{t('筛选条件')}</summary><div className="route-message-types"><span>{t('消息类型')}</span>{(['message','state','command'] as const).map((type,i)=><Checkbox key={type} checked={route.message_types?.includes(type)??false} onCheckedChange={checked=>onChange({...route,message_types:checked?[...(route.message_types??[]),type]:(route.message_types??[]).filter(v=>v!==type)})}>{t(['普通消息','状态上报','指令消息'][i])}</Checkbox>)}</div><label>{t('事件类型')}<Input maxLength={128} placeholder={t('留空表示全部')} value={route.event_type??''} onChange={e=>onChange({...route,event_type:e.target.value||null})}/></label></details>
      {p.forward&&<div className="command-fields"><b>{t('转发目标')}</b>{targets.map(target=><div key={target.destination_id}><Checkbox checked={forwards.some(v=>v.destination_id===target.destination_id)} onCheckedChange={checked=>{const chosen=checked?[...forwards,{rule_id:'',destination_id:target.destination_id,branch:'forward' as const}]:forwards.filter(v=>v.destination_id!==target.destination_id);onChange({...route,targets:[...route.targets.filter(v=>v.branch!=='forward'),...chosen],routing:selectRouteTargets(route.routing,[...chosen.map(v=>v.destination_id),...p.commands.map(c=>c.action.destination_id)])});}} description={<span className="mono">{target.address}</span>}>{target.name||target.protocol||'HTTP'} · {t('发送')}</Checkbox>{forwards.some(v=>v.destination_id===target.destination_id)&&<RoutingFields descriptor={target.routing} connectionId={target.owner_connection_id} value={route.routing?.targets[target.destination_id]} onChange={binding=>onChange({...route,routing:{source:route.routing?.source??null,targets:{...route.routing?.targets,[target.destination_id]:binding}}})}/>}</div>)}</div>}
    </div></section>
    <section><h3>{t('指令与操作')}</h3><div className="command-set-list">{p.commands.map((command,index)=><CommandEntry key={command.id} index={index} command={command} route={route} targets={targets} onValidity={validity} onChange={(value,routing)=>updateCommand(command.id,value,routing)} onDelete={p.commands.length>1?()=>{setInvalid(old=>{const next={...old};delete next[command.id];return next;});update({commands:p.commands.filter(c=>c.id!==command.id)});}:undefined}/>)}<Button disabled={p.commands.length>=16} onClick={()=>update({commands:[...p.commands,emptyCommand()]})}>{t('添加指令')}</Button></div></section>
  </>;
}
function CommandEntry({index,command,route,targets,onChange,onDelete,onValidity}:{index:number;command:RouteCommand;route:Route;targets:Endpoint[];onChange:(v:RouteCommand,routing?:Route['routing'])=>void;onDelete?:()=>void;onValidity:(id:string,v:boolean)=>void}) {
  const [text,setText]=useState(()=>JSON.stringify(command.action.arguments,null,2));
  const [valid,setValid]=useState(true);
  useEffect(()=>{onValidity(command.id,valid)},[command.id,valid,onValidity]);
  const projection:Route={...route,processor:{kind:'invoke_capability',tool_id:command.action.tool_id,revision:command.action.revision,allowed_senders:command.action.allowed_senders},command:{...command.mapping,output:{kind:'json',event_type:'command.invoke',template:command.action.arguments}},targets:[{rule_id:'',destination_id:command.action.destination_id}]};
  const change=(patch:Partial<RouteCommand>)=>onChange({...command,...patch});
  return <div className="command-entry"><div className="command-entry-heading"><b>{t('指令 {0}',[index+1])}</b>{onDelete&&<Button onClick={onDelete}>{t('删除')}</Button>}</div><div className="command-fields">
    <label><span>{t('匹配方式')}</span><select className="ui-input" value={command.mapping.mode} onChange={e=>change({mapping:{...command.mapping,mode:e.target.value as 'exact'|'prefix'}})}><option value="exact">{t('全文匹配')}</option><option value="prefix">{t('指令与参数')}</option></select></label>
    <label><span>{t('指令内容')}</span><Input maxLength={128} placeholder="开启开关" value={command.mapping.trigger} onChange={e=>change({mapping:{...command.mapping,trigger:e.target.value}})}/></label>
    <label><span>{t('消息字段')}</span><Input maxLength={256} value={command.mapping.text_pointer} onChange={e=>change({mapping:{...command.mapping,text_pointer:e.target.value}})}/></label>
    <ActionTargetFields route={projection} targets={targets} onChange={next=>{if(next.processor?.kind!=='invoke_capability')return;const action={destination_id:next.targets[0]?.destination_id??'',tool_id:next.processor.tool_id,revision:next.processor.revision,allowed_senders:next.processor.allowed_senders,arguments:next.command?.output.kind==='json'?next.command.output.template:command.action.arguments};setText(JSON.stringify(action.arguments,null,2));setValid(true);onChange({...command,action},{source:next.routing?.source??route.routing?.source??null,targets:{...route.routing?.targets,...next.routing?.targets}})}}/>
    <label><span>{t('调用参数')}</span><textarea className="ui-input mono" rows={4} value={text} aria-invalid={!valid} onChange={e=>{setText(e.target.value);try{const value:unknown=JSON.parse(e.target.value);setValid(true);change({action:{...command.action,arguments:value}})}catch{setValid(false)}}}/></label>{!valid&&<small className="error">{t('请输入有效的 JSON。')}</small>}
    <label><span>{t('回复')}</span><select className="ui-input" value={command.reply?.kind??'none'} onChange={e=>change({reply:e.target.value==='action_result'?{kind:'action_result'}:e.target.value==='fixed'?{kind:'fixed',text:''}:null})}><option value="none">{t('不回复')}</option><option value="fixed">{t('固定文本')}</option><option value="action_result">{t('能力执行结果')}</option></select></label>
    {command.reply?.kind==='fixed'&&<label><span>{t('文本')}</span><Input maxLength={4000} value={command.reply.text} onChange={e=>change({reply:{kind:'fixed',text:e.target.value}})}/></label>}
  </div></div>;
}
