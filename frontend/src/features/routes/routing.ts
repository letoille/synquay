import { ControlError } from '../mcp-services/api.ts';
export type RouteBinding={adapter:string;version:number;config:Record<string,unknown>};
export type ConnectionRouting={source:RouteBinding|null;targets:Record<string,RouteBinding>};
export type RoutingField={key:string;label:string;control:'text'|'conversation'|'conversations';conversation_kind:string|null};
export type RoutingDescriptor={adapter:string;version:number;modes:{id:string;label:string;fields:RoutingField[]}[]};
function obj(v:unknown):Record<string,unknown>{if(!v||typeof v!=='object'||Array.isArray(v))throw new ControlError(0,'invalid_response');return v as Record<string,unknown>;}
function str(v:unknown):string{if(typeof v!=='string')throw new ControlError(0,'invalid_response');return v;}
export function parseBinding(v:unknown):RouteBinding {const b=obj(v);if(b.version!==1)throw new ControlError(0,'unsupported_route_binding');return{adapter:str(b.adapter),version:1,config:obj(b.config)};}
export function parseRouting(v:unknown):ConnectionRouting|undefined {if(v==null)return;const r=obj(v);return {source:r.source==null?null:parseBinding(r.source),targets:Object.fromEntries(Object.entries(obj(r.targets)).map(([id,b])=>[id,parseBinding(b)]))};}
export function parseDescriptor(v:unknown):RoutingDescriptor|undefined {if(v==null)return;const d=obj(v);if(d.version!==1||!Array.isArray(d.modes)||d.modes.length>20)throw new ControlError(0,'unsupported_route_binding');return {adapter:str(d.adapter),version:1,modes:d.modes.map(raw=>{const m=obj(raw);if(!Array.isArray(m.fields)||m.fields.length>10)throw new ControlError(0,'invalid_response');return {id:str(m.id),label:str(m.label),fields:m.fields.map(raw=>{const f=obj(raw),control=str(f.control);if(!['text','conversation','conversations'].includes(control))throw new ControlError(0,'unsupported_route_binding');return{key:str(f.key),label:str(f.label),control:control as RoutingField['control'],conversation_kind:f.conversation_kind==null?null:str(f.conversation_kind)}})}})};}
export function bindingLabel(b?:RouteBinding|null):string|undefined {
 if(!b)return;const c=b.config;
 if(typeof c.filter==='string')return c.filter;
 if(typeof c.topic==='string')return c.topic;
 if(typeof c.id==='string')return c.id;
 if(Array.isArray(c.ids))return c.ids.join(', ');
 const labels:Record<string,string>={reply:'回复来源会话',all:'全部会话',all_groups:'所有群',all_direct:'所有单聊'};
 return labels[String(c.mode)];
}
export function selectRouteConnection(routing:ConnectionRouting|undefined):ConnectionRouting {return {...routing,source:null,targets:routing?.targets??{}};}
export function selectRouteTargets(routing:ConnectionRouting|undefined,ids:string[]):ConnectionRouting {return {source:routing?.source??null,targets:Object.fromEntries(Object.entries(routing?.targets??{}).filter(([id])=>ids.includes(id)))};}
