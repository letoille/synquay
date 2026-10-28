import {parseRouting,parseDescriptor,type ConnectionRouting,type RoutingDescriptor} from './routing.ts';
import { ControlError } from '../mcp-services/api.ts';
export type CommandMapping = {text_pointer:string;trigger:string;mode:'exact'|'prefix';output:{kind:'original'}|{kind:'json';event_type:string;template:unknown}};
export type ActionProcessor = {kind:'invoke_capability';tool_id:string;revision:number;allowed_senders:string[]};
export type CombinedAction = {destination_id:string;tool_id:string;revision:number;allowed_senders:string[];arguments:unknown};
export type CombinedProcessor = {kind:'combined';forward:boolean;action:CombinedAction|null;reply:{kind:'fixed';text:string}|{kind:'action_result'}|null};
export type RouteCommand = {id:string;mapping:CommandMapping;action:CombinedAction;reply:CombinedProcessor['reply']};
export type CommandSetProcessor = {kind:'command_set';forward:boolean;commands:RouteCommand[]};
export type ConnectionAction = {revision:number;target_required:boolean;consumes_reply:boolean;definition:{id:string;description:string;input_schema:Record<string,unknown>;effect:string}};
export type Processor = ActionProcessor | {kind:'forward'} | {kind:'respond'} | CombinedProcessor | CommandSetProcessor;
export type Source = {routing?:RoutingDescriptor;protocol?:string;connection_id:string;name:string;address:string};
export type MessageType = 'message'|'state'|'command';
export type Route = {message_types?:MessageType[];response?:{kind:'none'|'source'};routing?:ConnectionRouting; source_id?:string; processor?:Processor; command?:CommandMapping|null; id:string; name:string; enabled:boolean; event_type:string|null; revision:number; targets:{rule_id:string;destination_id:string;branch?:'legacy'|'forward'|'action'|'reply'}[] };
export type Endpoint = {actions?:ConnectionAction[];routing?:RoutingDescriptor;owner_connection_id?:string;protocol?:string;name?:string; connection_id:string; destination_id:string; address:string };
export type RoutesView = {available:boolean;reason:string|null;source_id?:string;source_address?:string;sources?:Source[];supports_processors?:boolean;supports_commands?:boolean;targets?:Endpoint[];snapshot?:{revision:number;routes:Route[]};pagination?:{page:number;page_size:number;total:number;total_pages:number};matched?:Record<string,number>;counts?:Record<string,Record<string,number>>};
export type RouteInput = {operation_id:string;expected_revision:number;route_id:string;change:{kind:'save';message_types?:MessageType[];response?:Route['response'];routing?:ConnectionRouting;source_id?:string;processor?:Processor;command?:CommandMapping|null;name:string;enabled:boolean;event_type:string|null;destination_ids:string[]}|{kind:'delete'}};
function invalid():never {throw new ControlError(0,'invalid_response');}
function object(v:unknown):Record<string,unknown> {return v!==null&&typeof v==='object'&&!Array.isArray(v)?v as Record<string,unknown>:invalid();}
function string(v:unknown):string {return typeof v==='string'?v:invalid();}
function integer(v:unknown):number {return typeof v==='number'&&Number.isSafeInteger(v)&&v>=0?v:invalid();}
function array(v:unknown,max:number):unknown[] {return Array.isArray(v)&&v.length<=max?v:invalid();}
export function parseRoutes(v:unknown):RoutesView {
  const o=object(v);if(typeof o.available!=='boolean')return invalid();
  const reason=o.reason===null?null:string(o.reason);
  if(!o.available)return {available:false,reason};
  const s=object(o.snapshot), counts:Record<string,Record<string,number>>={};
  for(const [key,raw] of Object.entries(object(o.counts))) {counts[key]={};for(const [state,count] of Object.entries(object(raw)))counts[key][state]=integer(count);}
  const matched=o.matched===undefined?undefined:Object.fromEntries(Object.entries(object(o.matched)).map(([id,count])=>[id,integer(count)]));
  let pagination:RoutesView['pagination'];
  if(o.pagination!==undefined){const p=object(o.pagination);pagination={page:integer(p.page),page_size:integer(p.page_size),total:integer(p.total),total_pages:integer(p.total_pages)};if(pagination.page<1||pagination.page_size<1||pagination.page_size>64||pagination.total_pages!==Math.ceil(pagination.total/pagination.page_size))return invalid();}
  return {available:true,reason,pagination,matched,source_id:string(o.source_id),source_address:string(o.source_address),counts,
    supports_processors:o.supports_processors===true,supports_commands:o.supports_commands===true,
    sources:o.sources===undefined?[{connection_id:string(o.source_id),name:'Webhook',address:string(o.source_address)}]:array(o.sources,48).map(v=>{const a=object(v);return{...(a.routing!=null?{routing:parseDescriptor(a.routing)}:{}),...(typeof a.protocol==='string'?{protocol:a.protocol}:{}),connection_id:string(a.connection_id),name:string(a.name),address:string(a.address)};}),
    targets:array(o.targets,64).map(v=>{const t=object(v);return {...(t.actions===undefined?{}:{actions:array(t.actions,64).map(v=>{const a=object(v),d=object(a.definition);if(typeof a.target_required!=='boolean'||typeof a.consumes_reply!=='boolean')return invalid();return {target_required:a.target_required,consumes_reply:a.consumes_reply,revision:integer(a.revision),definition:{id:string(d.id),description:string(d.description),input_schema:object(d.input_schema),effect:string(d.effect)}};})}),...(t.routing!=null?{routing:parseDescriptor(t.routing)}:{}),...(t.owner_connection_id!=null?{owner_connection_id:string(t.owner_connection_id)}:{}),...(typeof t.protocol==='string'?{protocol:t.protocol}:{}),...(typeof t.name==='string'?{name:t.name}:{}),connection_id:string(t.connection_id),destination_id:string(t.destination_id),address:string(t.address)};}),
    snapshot:{revision:integer(s.revision),routes:array(s.routes,64).map(v=>{const r=object(v);if(typeof r.enabled!=='boolean')return invalid();let processor:Processor|undefined;
      if(r.processor!=null){const p=object(r.processor);if(p.kind==='invoke_capability'){processor={kind:'invoke_capability',tool_id:string(p.tool_id),revision:integer(p.revision),allowed_senders:array(p.allowed_senders,100).map(string)};}else if(p.kind==='command_set'){if(typeof p.forward!=='boolean')return invalid();const commands=array(p.commands,16).map(v=>{const c=object(v),a=object(c.action);let reply:CombinedProcessor['reply']=null;if(c.reply!=null){const q=object(c.reply);if(q.kind==='fixed')reply={kind:'fixed',text:string(q.text)};else if(q.kind==='action_result')reply={kind:'action_result'};else return invalid();}return {id:string(c.id),mapping:parseCommand(c.mapping),action:{destination_id:string(a.destination_id),tool_id:string(a.tool_id),revision:integer(a.revision),allowed_senders:array(a.allowed_senders,100).map(string),arguments:a.arguments},reply};});processor={kind:'command_set',forward:p.forward,commands};}else if(p.kind==='combined'){if(typeof p.forward!=='boolean')return invalid();let action:CombinedAction|null=null,reply:CombinedProcessor['reply']=null;if(p.action!=null){const a=object(p.action);action={destination_id:string(a.destination_id),tool_id:string(a.tool_id),revision:integer(a.revision),allowed_senders:array(a.allowed_senders,100).map(string),arguments:a.arguments};}if(p.reply!=null){const q=object(p.reply);if(q.kind==='fixed')reply={kind:'fixed',text:string(q.text)};else if(q.kind==='action_result')reply={kind:'action_result'};else return invalid();}processor={kind:'combined',forward:p.forward,action,reply};}else if(p.kind==='forward'||p.kind==='respond')processor={kind:p.kind};else return invalid();}
      if(r.response!=null&& !['none','source'].includes(String(object(r.response).kind)))return invalid();const types=r.message_types===undefined?['message','command']:array(r.message_types,3).map(string);if(!types.length||new Set(types).size!==types.length||!types.every(v=>['message','state','command'].includes(v)))return invalid();return {message_types:types as MessageType[],...(r.response!=null?{response:{kind:object(r.response).kind as 'none'|'source'}}:{}),...(r.routing!=null?{routing:parseRouting(r.routing)}:{}),source_id:r.source_id==null?undefined:string(r.source_id),processor,...(r.command===null?{command:null}:r.command!=null?{command:parseCommand(r.command)}:{}),id:string(r.id),name:string(r.name),enabled:r.enabled,event_type:r.event_type===null?null:string(r.event_type),revision:integer(r.revision),targets:array(r.targets,32).map(v=>{const t=object(v);if(t.branch!==undefined&&!['legacy','forward','action','reply'].includes(String(t.branch)))return invalid();return {rule_id:string(t.rule_id),destination_id:string(t.destination_id),...(t.branch?{branch:t.branch as 'legacy'|'forward'|'action'|'reply'}:{})};})};})}};
}
async function request(path:string,input?:RouteInput):Promise<unknown> {
  let response:Response;
  try {response=await fetch(`/api/routes${path}`,{method:input?'POST':'GET',cache:'no-store',credentials:'omit',redirect:'error',signal:AbortSignal.timeout(40_000),headers:{Accept:'application/json',...(input?{'Content-Type':'application/json'}:{})},...(input?{body:JSON.stringify(input)}:{})});}
  catch {throw new ControlError(0,'network_error');}
  if(!response.ok) {let code='request_failed';try {code=string(object(await response.json()).code);}catch{/* Non-JSON proxy error */}throw new ControlError(response.status,code);}
  try {return await response.json();}catch{return invalid();}
}
export function parseCommand(value:unknown):CommandMapping {
  const c=object(value), output=object(c.output);
  if(c.mode!=='exact'&&c.mode!=='prefix')return invalid();
  if(output.kind!=='original'&&output.kind!=='json')return invalid();
  if(output.kind==='json'&&!Object.hasOwn(output,'template'))return invalid();
  return {text_pointer:string(c.text_pointer),trigger:string(c.trigger),mode:c.mode,output:output.kind==='original'?{kind:'original'}:{kind:'json',event_type:string(output.event_type),template:output.template}};
}
function receipt(value:unknown) {const r=object(value);return {operation_id:string(r.operation_id),revision:integer(r.revision)};}
export type RoutesQuery={sourceIds?:string[];page?:number;pageSize?:number};
export function routesQueryString(query:RoutesQuery={}) {
  const params=new URLSearchParams();
  if(query.sourceIds?.length)params.set('source_ids',query.sourceIds.join(','));
  if(query.page!==undefined)params.set('page',String(query.page));
  if(query.pageSize!==undefined)params.set('page_size',String(query.pageSize));
  return params.size?`?${params}`:'';
}
export const routesApi={
  view:async(query:RoutesQuery={})=>parseRoutes(await request(routesQueryString(query))),
  change:async(input:RouteInput)=>receipt(await request('',input)),
  operation:async(id:string)=>{const r=object(await request(`/operations/${encodeURIComponent(id)}`));return r.receipt===null?null:receipt(r.receipt);},
};
export function routeSave(route:Route):RouteInput['change'] {const destinations=route.targets.filter(t=>(route.processor?.kind!=='combined'&&route.processor?.kind!=='command_set')||t.branch==='forward').map(t=>t.destination_id);const allowed=new Set([...destinations,...(route.processor?.kind==='combined'&&route.processor.action?[route.processor.action.destination_id]:route.processor?.kind==='command_set'?route.processor.commands.map(c=>c.action.destination_id):[])]);const routing=route.routing&&(route.processor?.kind==='combined'||route.processor?.kind==='command_set')?{...route.routing,targets:Object.fromEntries(Object.entries(route.routing.targets).filter(([id])=>allowed.has(id)))}:route.routing;return {kind:'save',message_types:route.message_types??['message','command'],...(route.response?{response:route.response}:{}),...(routing?{routing}:{}),...(route.command!==undefined?{command:route.command}:{}),...(route.source_id?{source_id:route.source_id}:{}),...(route.processor?{processor:route.processor}:{}),name:route.name,enabled:route.enabled,event_type:route.event_type,destination_ids:destinations};}
export function routeTotals(route:Route,counts:RoutesView['counts']) {
  let pending=0,submitted=0,failed=0;
  for(const t of route.targets) {const c=counts?.[t.rule_id]??{};pending+=(c.pending??0)+(c.leased??0)+(c.sending??0);submitted+=c.submitted??0;failed+=(c.failed??0)+(c.reconcile??0);}
  return {pending,submitted,failed};
}
export function endpointLabel(endpoint?:Endpoint):string {
  if(!endpoint)return 'HTTP';
  try {const u=new URL(endpoint.address);return `${u.host}${u.pathname==='/'?'':u.pathname}`;}catch{return endpoint.address;}
}
