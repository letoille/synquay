import {t} from '../../shared/i18n';
import {RoutingFields} from './RoutingFields';
import {selectActionTarget} from './actions';
import type {Route,Endpoint,ActionProcessor} from './api';

export function ActionTargetFields({route,targets,onChange}:{route:Route;targets:Endpoint[];onChange:(r:Route)=>void}) {
  if(route.processor?.kind!=='invoke_capability')return null;
  const p=route.processor,target=targets.find(t=>t.destination_id===route.targets[0]?.destination_id);
  const update=(patch:Partial<ActionProcessor>)=>onChange({...route,processor:{...p,...patch}});
  const action=target?.actions?.find(a=>a.definition.id===p.tool_id&&a.revision===p.revision);
  return <div className="command-fields">
    <label><span>{t('连接')}</span><select className="ui-input" value={target?.destination_id??''} onChange={e=>onChange(selectActionTarget(route,targets.find(t=>t.destination_id===e.target.value)))}><option value="">{t('选择连接')}</option>{targets.filter(t=>t.actions?.length).map(t=><option key={t.destination_id} value={t.destination_id}>{t.name||t.protocol}</option>)}</select></label>
    {target&&<>
      <label><span>{t('操作')}</span><select className="ui-input" value={p.tool_id} onChange={e=>{const a=target.actions?.find(a=>a.definition.id===e.target.value);if(a)onChange({...route,routing:a.target_required&&!route.routing?.targets[target.destination_id]&&target.owner_connection_id===route.source_id&&target.routing?.modes.some(m=>m.id==='reply')?{source:route.routing?.source??null,targets:{...route.routing?.targets,[target.destination_id]:{adapter:target.routing!.adapter,version:target.routing!.version,config:{mode:'reply'}}}}:route.routing,processor:{...p,tool_id:a.definition.id,revision:a.revision},command:route.command?{...route.command,output:{kind:'json',event_type:'command.invoke',template:a.definition.id==='qq.send_text'?{content:{$command:'arguments'}}:{}}}:route.command});}}>{target.actions?.map(a=><option key={a.definition.id} value={a.definition.id}>{t(a.definition.description)}</option>)}</select></label>
      {action&&<details><summary>{t('参数格式')}</summary><pre className="mono action-schema">{JSON.stringify(action.definition.input_schema,null,2)}</pre></details>}
      {action?.target_required&&<RoutingFields descriptor={target.routing} connectionId={target.owner_connection_id} value={route.routing?.targets[target.destination_id]} onChange={binding=>onChange({...route,routing:{source:route.routing?.source??null,targets:{...route.routing?.targets,[target.destination_id]:binding}}})}/>}
    </>}
    <label><span>{t('允许触发的用户')}</span><textarea className="ui-input mono" rows={3} maxLength={13000} placeholder={t('用户 OpenID，每行一个')} value={p.allowed_senders.join('\n')} onChange={e=>update({allowed_senders:e.target.value.split('\n')})} onBlur={()=>update({allowed_senders:p.allowed_senders.map(s=>s.trim()).filter(Boolean)})}/></label>

  </div>;
}
