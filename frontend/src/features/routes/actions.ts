import type {Endpoint,Processor,Route} from './api.ts';
import {selectRouteTargets} from './routing.ts';

export function changeProcessor(route:Route,kind:Processor['kind'],targets:Endpoint[]):Route {
  if(kind==='invoke_capability')return {...route,response:{kind:'none'},processor:{kind,tool_id:'',revision:1,allowed_senders:[]},targets:[],routing:selectRouteTargets(route.routing,[]),command:{text_pointer:route.command?.text_pointer??'/content',trigger:route.command?.trigger??'',mode:route.command?.mode??'prefix',output:{kind:'json',event_type:'command.invoke',template:{}}}};
  if(kind==='respond')return {...route,processor:{kind},response:{kind:'source'},targets:targets.filter(t=>t.owner_connection_id===route.source_id).map(t=>({rule_id:'',destination_id:t.destination_id})),routing:{source:route.routing?.source??null,targets:{}},command:{text_pointer:'/content',trigger:route.command?.trigger??'',mode:'exact',output:{kind:'json',event_type:'command.response',template:{content:'OK'}}}};
  return {...route,response:{kind:'none'},routing:selectRouteTargets(route.routing,route.targets.map(t=>t.destination_id)),processor:{kind:'forward'}};
}
export function selectActionTarget(route:Route,target?:Endpoint):Route {
  if(route.processor?.kind!=='invoke_capability')return route;
  const action=target?.actions?.[0],ids=target?[target.destination_id]:[];
  return {...route,response:target?.owner_connection_id===route.source_id?route.response:{kind:'none'},targets:ids.map(destination_id=>({rule_id:route.targets.find(t=>t.destination_id===destination_id)?.rule_id??'',destination_id})),routing:selectRouteTargets(route.routing,ids),processor:{...route.processor,tool_id:action?.definition.id??'',revision:action?.revision??1},command:route.command?{...route.command,output:{kind:'json',event_type:'command.invoke',template:{}}}:route.command};
}
export function actionValid(route:Route):boolean {
  if(route.processor?.kind==='respond')return route.response?.kind==='source'&&route.targets.length===1&&route.command?.output.kind==='json';
  if(route.processor?.kind!=='invoke_capability')return true;
  const p=route.processor;
  return !!p.tool_id&&p.revision>0&&route.targets.length===1&&p.allowed_senders.length>0&&p.allowed_senders.length<=100&&p.allowed_senders.every(s=>/^[a-zA-Z0-9_-]{1,128}$/.test(s))&&new Set(p.allowed_senders).size===p.allowed_senders.length&&route.command?.output.kind==='json';
}
