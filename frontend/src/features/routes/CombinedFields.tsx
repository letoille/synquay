import {useEffect, useState} from 'react';
import {t} from '../../shared/i18n';
import {Button, Checkbox, Input} from '../../shared/ui/Controls';
import {makeCommandSet} from './combined';
import {ActionTargetFields} from './ActionTargetFields';
import {CommandFields} from './CommandFields';
import {RoutingFields} from './RoutingFields';
import {selectRouteTargets} from './routing';
import type {CombinedProcessor, Endpoint, Route, Source} from './api';

const defaultCommand = () => ({text_pointer:'/content',trigger:'',mode:'prefix' as const,output:{kind:'original' as const}});
export function CombinedFields({route,source,targets,onChange,onValidity}:{route:Route;source?:Source;targets:Endpoint[];onChange:(r:Route)=>void;onValidity:(v:boolean)=>void}) {
  const p=route.processor as CombinedProcessor;
  const [argumentsText,setArgumentsText]=useState(()=>JSON.stringify(p.action?.arguments??{},null,2));
  const [jsonValid,setJsonValid]=useState(true);
  const [commandValid,setCommandValid]=useState(true);
  useEffect(()=>{onValidity(jsonValid&&commandValid);return()=>onValidity(true)},[jsonValid,commandValid,onValidity]);
  const update=(patch:Partial<CombinedProcessor>,other:Partial<Route>={})=>onChange({...route,...other,processor:{...p,...patch}});
  const forwardTargets=route.targets.filter(v=>v.branch==='forward');
  const qq=source?.protocol==='QQ';
  const projected:Route=p.action?{...route,processor:{kind:'invoke_capability',tool_id:p.action.tool_id,revision:p.action.revision,allowed_senders:p.action.allowed_senders},targets:[{rule_id:'',destination_id:p.action.destination_id}],command:route.command?{...route.command,output:{kind:'json',event_type:'command.invoke',template:p.action.arguments}}:defaultCommand()}:route;
  return <>
    <section><h3>{t('处理')}</h3><div className="command-fields">
      <Checkbox checked={p.forward} onCheckedChange={checked=>update({forward:!!checked},checked?{}:{targets:route.targets.filter(v=>v.branch!=='forward')})}>{t('转发')}</Checkbox>
      {qq&&<Checkbox checked={!!p.action} onCheckedChange={checked=>{setJsonValid(true);setArgumentsText('{}');update({action:checked?{destination_id:'',tool_id:'',revision:1,allowed_senders:[],arguments:{}}:null,...(!checked&&p.reply?.kind==='action_result'?{reply:null}:{})},{command:route.command??defaultCommand()});}}>{t('调用能力')}</Checkbox>}
      {qq&&<Checkbox checked={!!p.reply} onCheckedChange={checked=>update({reply:checked?{kind:'fixed',text:''}:null},{command:route.command??defaultCommand()})}>{t('回复')}</Checkbox>}
      <Checkbox disabled={!!p.action||!!p.reply} checked={!!route.command} onCheckedChange={checked=>onChange({...route,command:checked?defaultCommand():null})}>{t('指令映射')}</Checkbox>
      {route.command&&<CommandFields value={route.command} onValidity={setCommandValid} onChange={command=>onChange({...route,command})}/>}
      {p.action&&<Button onClick={()=>onChange(makeCommandSet(route))}>{t('添加指令')}</Button>}
      <details><summary>{t('筛选条件')}</summary><div className="route-message-types"><span>{t('消息类型')}</span>{(['message','state','command'] as const).map((type,i)=><Checkbox key={type} checked={route.message_types?.includes(type)??false} onCheckedChange={checked=>onChange({...route,message_types:checked?[...(route.message_types??[]),type]:(route.message_types??[]).filter(v=>v!==type)})}>{t(['普通消息','状态上报','指令消息'][i])}</Checkbox>)}</div><label>{t('事件类型')}<Input maxLength={128} placeholder={t('留空表示全部')} value={route.event_type??''} onChange={e=>onChange({...route,event_type:e.target.value||null})}/></label></details>
    </div></section>
    <section><h3>{t('配置')}</h3>
      {p.forward&&<div className="command-fields"><b>{t('转发目标')}</b>{targets.map(target=><div key={target.destination_id}><Checkbox checked={forwardTargets.some(v=>v.destination_id===target.destination_id)} onCheckedChange={checked=>{const chosen=checked?[...forwardTargets,{rule_id:'',destination_id:target.destination_id,branch:'forward' as const}]:forwardTargets.filter(v=>v.destination_id!==target.destination_id);const all=[...route.targets.filter(v=>v.branch!=='forward'),...chosen];onChange({...route,targets:all,routing:selectRouteTargets(route.routing,[...chosen.map(v=>v.destination_id),...(p.action?[p.action.destination_id]:[])])});}} description={<span className="mono">{target.address}</span>}>{target.name||target.protocol||'HTTP'} · {t('发送')}</Checkbox>{forwardTargets.some(v=>v.destination_id===target.destination_id)&&<RoutingFields descriptor={target.routing} connectionId={target.owner_connection_id} value={route.routing?.targets[target.destination_id]} onChange={binding=>onChange({...route,routing:{source:route.routing?.source??null,targets:{...route.routing?.targets,[target.destination_id]:binding}}})}/>}</div>)}</div>}
      {p.action&&<div className="command-fields"><b>{t('调用能力')}</b><ActionTargetFields route={projected} targets={targets} onChange={next=>{if(next.processor?.kind!=='invoke_capability')return;const a={destination_id:next.targets[0]?.destination_id??'',tool_id:next.processor.tool_id,revision:next.processor.revision,allowed_senders:next.processor.allowed_senders,arguments:next.command?.output.kind==='json'?next.command.output.template:p.action!.arguments};setArgumentsText(JSON.stringify(a.arguments,null,2));setJsonValid(true);update({action:a},{routing:{source:next.routing?.source??route.routing?.source??null,targets:{...route.routing?.targets,...next.routing?.targets}}});}}/><label><span>{t('调用参数')}</span><textarea className="ui-input mono" rows={5} value={argumentsText} aria-invalid={!jsonValid} onChange={e=>{setArgumentsText(e.target.value);try {const value:unknown=JSON.parse(e.target.value);setJsonValid(true);update({action:{...p.action!,arguments:value}})}catch{setJsonValid(false)}}}/></label>{!jsonValid&&<small className="error">{t('请输入有效的 JSON。')}</small>}</div>}
      {p.reply&&<div className="command-fields"><b>{t('回复')}</b><label><span>{t('回复内容')}</span><select className="ui-input" value={p.reply.kind} onChange={e=>update({reply:e.target.value==='action_result'?{kind:'action_result'}:{kind:'fixed',text:''}})}><option value="fixed">{t('固定文本')}</option>{p.action&&<option value="action_result">{t('能力执行结果')}</option>}</select></label>{p.reply.kind==='fixed'&&<label><span>{t('文本')}</span><Input maxLength={4000} value={p.reply.text} onChange={e=>update({reply:{kind:'fixed',text:e.target.value}})}/></label>}</div>}
    </section>
  </>;
}
