import test from 'node:test';
import assert from 'node:assert/strict';
import { controlApi, ControlError } from '../src/features/mcp-services/api.ts';
import { draftNames, readPending, storePending } from '../src/features/mcp-services/model.ts';
import type { ControlView } from '../src/features/mcp-services/types.ts';

const initial: ControlView = { revision: 2, profile_id: 'profile-a', admin_id: 'admin-a', tools: [], draft: { generation: 1, base_revision: 1, exposed_tools: ['old-tool'], actor_id: 'admin-a' }, connection: { resource: null, authentication: 'bearer', issuer: null } };
const operation = { operation_id: '00000000-0000-0000-0000-000000000099', expected_revision: 2, draft_generation: 2 };

test('an old draft cannot re-enable tools removed by a newer publication', () => {
  assert.deepEqual(draftNames(initial), []);
  assert.deepEqual(draftNames({ ...initial, revision: 1 }), ['old-tool']);
});

test('recovery metadata is scoped by profile and administrator and contains no credential', () => {
  const values = new Map<string, string>();
  const old = Object.getOwnPropertyDescriptor(globalThis, 'sessionStorage');
  Object.defineProperty(globalThis, 'sessionStorage', { configurable: true, value: { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => values.set(key, value), removeItem: (key: string) => values.delete(key) } });
  try {
    assert.equal(storePending(initial, operation), true);
    assert.deepEqual(readPending(initial), operation);
    assert.equal(readPending({ ...initial, admin_id: 'other-admin' }), null);
    assert.equal(readPending({ ...initial, profile_id: 'other-profile' }), null);
    assert.deepEqual(Object.keys(JSON.parse([...values.values()][0])).sort(), ['draft_generation', 'expected_revision', 'operation_id']);
    values.set([...values.keys()][0], JSON.stringify({ ...operation, expected_revision: Number.MAX_SAFE_INTEGER + 1 }));
    assert.equal(readPending(initial), null);
    storePending(initial, null); assert.equal(values.size, 0);
  } finally { if (old) Object.defineProperty(globalThis, 'sessionStorage', old); else Reflect.deleteProperty(globalThis, 'sessionStorage'); }
});

test('publication retries preserve the exact request and never auto-retry an ambiguous response', async () => {
  const saved = globalThis.fetch; const calls: { url: string; input: RequestInit }[] = [];
  globalThis.fetch = async (url, input) => { calls.push({ url: String(url), input: input! }); if (calls.length === 1) throw Error('private upstream details'); return Response.json({ ...operation, revision: 3, replayed: true }); };
  try {
    const api = controlApi();
    await assert.rejects(api.publish(operation), (e: unknown) => e instanceof ControlError && e.code === 'network_error' && !e.message.includes('private'));
    assert.equal(calls.length, 1);
    assert.equal((await api.publish(operation)).replayed, true);
    assert.equal(calls[0].input.body, calls[1].input.body);
    assert.equal(calls[0].input.credentials, 'omit');
    assert.equal(calls[0].input.redirect, 'error');
    assert.equal(calls[0].input.cache, 'no-store');
    assert.equal(new Headers(calls[0].input.headers).has('Authorization'), false);
  } finally { globalThis.fetch = saved; }
});

test('unknown result queries are read-only and never interpreted as failure or success', async () => {
  const saved = globalThis.fetch; let method: string | undefined;
  globalThis.fetch = async (_url, input) => { method = input?.method; return Response.json({ status: 'unconfirmed', receipt: null }); };
  try { assert.deepEqual(await controlApi().result(operation), { status: 'unconfirmed', receipt: null }); assert.equal(method, 'GET'); }
  finally { globalThis.fetch = saved; }
});

test('rejects unsafe revisions and malformed committed receipts', async () => {
  const saved = globalThis.fetch;
  try {
    globalThis.fetch = async () => Response.json({ ...initial, revision: Number.MAX_SAFE_INTEGER + 1 });
    await assert.rejects(controlApi().view(), ControlError);
    globalThis.fetch = async () => Response.json({ status: 'committed', receipt: null });
    await assert.rejects(controlApi().result(operation), ControlError);
  } finally { globalThis.fetch = saved; }
});

test('error responses keep a safe code and request ID without displaying server bodies', async () => {
  const saved = globalThis.fetch;
  globalThis.fetch = async () => Response.json({ code: 'version_conflict', error: 'sensitive debug contents' }, { status: 409, headers: { 'x-request-id': 'request-1' } });
  try { await assert.rejects(controlApi().save({ expected_generation: 1, base_revision: 2, exposed_tools: [] }), (e: unknown) => e instanceof ControlError && e.status === 409 && e.requestId === 'request-1' && e.message === 'version_conflict'); }
  finally { globalThis.fetch = saved; }
});

test('new connection recovery never resends a creation and verification preserves invocation identity', async () => {
  const saved = globalThis.fetch; const calls: { url: string; init?: RequestInit }[] = [];
  globalThis.fetch = async (url, init) => { calls.push({url: String(url), init}); return calls.length === 1 ? Response.json({status:'unconfirmed', receipt:null}) : Response.json({invocation_id:operation.operation_id,status:'unknown'}); };
  try {
    const api=controlApi();
    assert.equal((await api.connectionResult(operation.operation_id)).status, 'unconfirmed');
    assert.equal(calls[0].init?.method,'GET');
    assert.equal((await api.verify({mcp_token:'call-token',invocation_id:operation.operation_id,action:{kind:'inspect'}})).status,'unknown');
    assert.equal(calls.length,2);
    assert.ok(!calls[1].url.includes('call-token'));
    assert.deepEqual(JSON.parse(String(calls[1].init?.body)).action,{kind:'inspect'});
  } finally { globalThis.fetch=saved; }
});

test('console locales are complete, preserve interpolation and leave external text unchanged', async () => {
  const { catalog } = await import('../src/shared/i18n/catalog.ts');
  const { translate, resolveLocale } = await import('../src/shared/i18n/index.ts');
  for (const [key, values] of Object.entries(catalog)) {
    assert.equal(values.length, 3, key);
    for (const value of values) {
      assert.ok(value.trim().length, key);
      assert.deepEqual(value.match(/\{\d+\}/g) ?? [], key.match(/\{\d+\}/g) ?? [], key);
    }
  }
  assert.equal(resolveLocale('es-MX'), 'es');
  assert.equal(resolveLocale('ja-JP'), 'ja');
  assert.equal(resolveLocale('zh-TW'), 'zh-CN');
  assert.equal(resolveLocale('fr-FR'), 'en');
  assert.equal(translate('已显示 {0} 条', 'en', [12]), '12 records shown');
  assert.equal(translate('发布已确认：v7。', 'es'), 'Publicación confirmada: v7.');
  assert.equal(translate('custom_device_control_42', 'ja'), 'custom_device_control_42');
});

test('locale subscriptions work without browser storage and unsubscribe cleanly', async () => {
  const { getLocale, setLocale, subscribeLocale } = await import('../src/shared/i18n/index.ts');
  let updates = 0;
  const unsubscribe = subscribeLocale(() => updates++);
  setLocale('ja');
  assert.equal(getLocale(), 'ja');
  setLocale('es');
  assert.equal(updates, 2);
  unsubscribe();
  setLocale('zh-CN');
  assert.equal(updates, 2);
});

test('MCP server API validates independent endpoints and preserves mutation identity', async () => {
  const saved=globalThis.fetch;
  const id='00000000-0000-0000-0000-000000000123';
  const entry={id,name:'Research',enabled:true,deleted:false,revision:1,tool_revision:1,path:`/mcp/${id}`,address:null,exposed_tools:[],is_default:false,authentication:'bearer'};
  const input={operation_id:'00000000-0000-0000-0000-000000000999',expected_revision:0,expected_tool_revision:0,name:'Research',enabled:true,exposed_tools:[]};
  const calls: RequestInit[]=[];
  globalThis.fetch=async (_url,init)=>{calls.push(init!);return Response.json(entry);};
  try {
    const api=controlApi();
    assert.equal((await api.saveServer(id,input)).id,id);
    await api.saveServer(id,input);
    assert.equal(calls[0].body,calls[1].body);
    assert.equal(calls[0].method,'PUT');
    globalThis.fetch=async()=>Response.json({items:[{...entry,path:'/mcp/a7f3c9d2',address:'https://mcp.example.test/mcp/a7f3c9d2'}]});
    assert.equal((await api.servers())[0].path,'/mcp/a7f3c9d2');
    globalThis.fetch=async()=>Response.json({items:[{...entry,path:'/api/tools'}]});
    await assert.rejects(api.servers(),(error:unknown)=>error instanceof ControlError && error.code==='invalid_response');
  } finally {globalThis.fetch=saved;}
});

test('server configuration retains independent auth, description and transport and rejects unknown modes', async () => {
  const original=globalThis.fetch;
  const id='00000000-0000-0000-0000-000000000123';
  const entry={id,name:'Devices',description:'Home devices',enabled:true,deleted:false,revision:2,tool_revision:2,path:`/mcp/${id}`,address:null,exposed_tools:[],is_default:false,authentication:'basic',transport:'streamable_http',auth:{mode:'basic',username:'agent',password_env:'DEVICE_PASSWORD'}};
  try {
    globalThis.fetch=async()=>Response.json(entry);
    const result=await controlApi().server(id);
    assert.equal(result.description,'Home devices');assert.equal(result.transport,'streamable_http');assert.deepEqual(result.auth,entry.auth);
    globalThis.fetch=async()=>Response.json({...entry,auth:{mode:'unrecognized'}});
    await assert.rejects(controlApi().server(id),ControlError);
    globalThis.fetch=async()=>Response.json({...entry,transport:'unsupported'});
    await assert.rejects(controlApi().server(id),ControlError);
  } finally {globalThis.fetch=original;}
});

test('content catalogs and service exposure preserve typed boundaries and revision checks', async () => {
  const saved=globalThis.fetch;
  const content={revision:3,resources:[{id:'device',description:'Device metadata',mime_type:'application/json',kind:'template',parameter:'device_id'}],prompts:[{id:'plan',description:'Plan a task',arguments:[{name:'device',description:'Target device',required:true,choices:['heater']}]}]};
  const id='00000000-0000-0000-0000-000000000123';
  const scope={resources:['device'],prompts:['plan']};
  const server={id,name:'Devices',enabled:true,deleted:false,revision:2,tool_revision:2,path:'/mcp/devices',address:null,exposed_tools:[],is_default:false,authentication:'bearer',content_scope:scope,content_revision:3};
  try {
    globalThis.fetch=async()=>Response.json({...initial,content});
    assert.deepEqual((await controlApi().view()).content,content);
    let body: unknown;
    globalThis.fetch=async(_url,init)=>{body=JSON.parse(String(init?.body));return Response.json(server);};
    const input={operation_id:id,expected_revision:1,expected_tool_revision:1,name:'Devices',enabled:true,exposed_tools:[],content_scope:scope,expected_content_revision:3};
    assert.deepEqual((await controlApi().saveServer(id,input)).content_scope,scope);
    assert.deepEqual(body,input);
    globalThis.fetch=async()=>Response.json({...initial,content:{...content,resources:[...content.resources,...content.resources]}});
    await assert.rejects(controlApi().view(),ControlError);
    globalThis.fetch=async()=>Response.json({...initial,content:{...content,resources:[{...content.resources[0],kind:'unknown'}]}});
    await assert.rejects(controlApi().view(),ControlError);
    globalThis.fetch=async()=>Response.json({...server,content_scope:{resources:['device','device'],prompts:[]}});
    await assert.rejects(controlApi().server(id),ControlError);
  } finally {globalThis.fetch=saved;}
});

test('connection edits and deletes retain operation identity and do not resend during recovery', async () => {
  const saved=globalThis.fetch;const calls:{url:string;input:RequestInit}[]=[];
  globalThis.fetch=async(url,input)=>{calls.push({url:String(url),input:input!});return input?.method==='GET'?Response.json({status:'unconfirmed',receipt:null}):Response.json({operation_id:operation.operation_id,revision:3,replayed:false});};
  try {
    const api=controlApi();
    const input={operation_id:operation.operation_id,expected_revision:2,change:{kind:'delete' as const}};
    await api.changeConnection('connection-a',input);
    assert.equal(calls[0].input.method,'DELETE');
    assert.equal(calls[0].url,'/api/tools/connections/connection-a');
    assert.deepEqual(JSON.parse(calls[0].input.body as string),input);
    await api.connectionResult(input.operation_id);
    assert.equal(calls[1].input.method,'GET');
    assert.equal(calls[1].input.body,undefined);
    assert.equal(calls.length,2);
  } finally {globalThis.fetch=saved;}
});

test('route API validates availability, preserves uncertain mutation identity, and counts submitted separately', async () => {
  const {parseRoutes,routesApi,routeTotals}=await import('../src/features/routes/api.ts');
  assert.deepEqual(parseRoutes({available:false,reason:'pipeline_not_configured'}),{available:false,reason:'pipeline_not_configured'});
  assert.throws(()=>parseRoutes({available:true,reason:null,snapshot:{revision:1,routes:[]},targets:[]}));
  const route={id:'route',name:'path',enabled:true,event_type:null,revision:1,targets:[{rule_id:'branch',destination_id:'target'}]};
  assert.deepEqual(routeTotals(route,{branch:{pending:2,leased:1,sending:1,submitted:5,reconcile:1,failed:2}}),{pending:4,submitted:5,failed:3});
  const prior=globalThis.fetch;const calls:RequestInit[]=[];
  globalThis.fetch=async(_url,input)=>{calls.push(input!);if(calls.length===1)throw Error('lost response');return Response.json({operation_id:'op',revision:2});};
  const change={operation_id:'op',expected_revision:1,route_id:'route',change:{kind:'delete' as const}};
  try {
    await assert.rejects(routesApi.change(change));assert.equal(calls.length,1);
    await routesApi.change(change);assert.equal(calls[0].body,calls[1].body);
  }finally{globalThis.fetch=prior;}
});

test('route views retain source, processor and explicit message types', async () => {
  const {parseRoutes,routeSave}=await import('../src/features/routes/api.ts');
  const route={id:'route',name:'analysis',enabled:false,event_type:'app.message',revision:1,targets:[{rule_id:'branch',destination_id:'target'}]};
  const legacy={available:true,reason:null,source_id:'primary',source_address:'/api/webhooks/primary',counts:{},targets:[],snapshot:{revision:1,routes:[route]}};
  const old=parseRoutes(legacy);
  assert.equal(old.supports_processors,false);
  assert.deepEqual(old.sources,[{connection_id:'primary',name:'Webhook',address:'/api/webhooks/primary'}]);
  assert.equal('source_id' in routeSave(old.snapshot!.routes[0]),false);
  assert.equal('processor' in routeSave(old.snapshot!.routes[0]),false);
  const sources=[...old.sources!,{connection_id:'app',name:'Application',address:'/api/webhooks/app'}];
  const processor={kind:'forward' as const};
  const current={...legacy,sources,supports_processors:true,snapshot:{revision:2,routes:[{...route,source_id:'app',processor}]}};
  const parsed=parseRoutes(current);
  assert.deepEqual(parsed.sources,sources);
  assert.equal(parsed.supports_processors,true);
  const filtered=parseRoutes({...current,snapshot:{revision:2,routes:[{...route,message_types:['state','command']}]}});
  assert.deepEqual(routeSave(filtered.snapshot!.routes[0]).message_types,['state','command']);
  assert.throws(()=>parseRoutes({...current,snapshot:{revision:2,routes:[{...route,message_types:['state','state']}]}}));
  assert.throws(()=>parseRoutes({...current,snapshot:{revision:2,routes:[{...route,message_types:['unknown']}]}}));
  assert.deepEqual(routeSave(parsed.snapshot!.routes[0]),{kind:'save',message_types:['message','command'],source_id:'app',processor,name:'analysis',enabled:false,event_type:'app.message',destination_ids:['target']});
  assert.throws(()=>parseRoutes({...current,snapshot:{revision:2,routes:[{...route,processor:{kind:'unknown'}}]}}));
  for (const kind of ['agent_draft','agent_http']) assert.throws(()=>parseRoutes({...current,snapshot:{revision:2,routes:[{...route,processor:{kind,name:'legacy'}}]}}));
});

test('route source filters remember independent view choices and prune removed sources', async () => {
  const {parseRoutePreferences,normalizeRoutePreferences,selectRouteSource,routeViewQuery}=await import('../src/features/routes/preferences.ts');
  const a='00000000-0000-0000-0000-000000000001',b='00000000-0000-0000-0000-000000000002';
  let p=parseRoutePreferences(null);
  assert.deepEqual(routeViewQuery(p),{sourceIds:[],page:1,pageSize:10});
  p=selectRouteSource(p,a);p=selectRouteSource(p,b);
  assert.deepEqual(routeViewQuery(p).sourceIds,[b]);
  p={...p,mode:'graph'};p=selectRouteSource(p,a);p=selectRouteSource(p,b);
  assert.deepEqual(routeViewQuery(p).sourceIds,[a,b]);
  p={...p,graphPage:2,listPage:3};
  const restored=parseRoutePreferences(JSON.stringify(p));
  assert.deepEqual(restored,p);
  assert.deepEqual(routeViewQuery({...restored,mode:'list'}),{sourceIds:[b],page:3,pageSize:10});
  const pruned=normalizeRoutePreferences(restored,[a]);
  assert.equal(pruned.listSource,null);assert.equal(pruned.listPage,1);
  assert.deepEqual(pruned.graphSources,[a]);assert.equal(pruned.graphPage,1);
  assert.deepEqual(selectRouteSource(pruned,a).graphSources,[],'deselecting the last source restores all');
  assert.deepEqual(selectRouteSource(restored,null).graphSources,[]);
  assert.deepEqual(parseRoutePreferences('{broken'),parseRoutePreferences(null));
  assert.deepEqual(parseRoutePreferences(JSON.stringify({mode:'invalid',listPage:-1,graphPage:Infinity,listSource:'bad',graphSources:[a,a,'bad',null]})).graphSources,[a]);
});

test('route requests send server filters and pagination and validate page metadata', async () => {
  const {routesApi,parseRoutes}=await import('../src/features/routes/api.ts');
  const source='00000000-0000-0000-0000-000000000001';
  const base={available:true,reason:null,source_id:source,source_address:'/webhook',sources:[{connection_id:source,name:'A',address:'/webhook'}],counts:{},targets:[],snapshot:{revision:8,routes:[]},pagination:{page:2,page_size:10,total:11,total_pages:2}};
  const previous=globalThis.fetch;let called='';
  globalThis.fetch=async(input)=>{called=String(input);return Response.json(base);};
  try {
    const result=await routesApi.view({sourceIds:[source],page:2,pageSize:10});
    const url=new URL(called,'http://localhost');
    assert.equal(url.searchParams.get('source_ids'),source);assert.equal(url.searchParams.get('page'),'2');assert.equal(url.searchParams.get('page_size'),'10');
    assert.deepEqual(result.pagination,base.pagination);
    await routesApi.view({sourceIds:[],page:1,pageSize:64});
    assert.equal(new URL(called,'http://localhost').searchParams.has('source_ids'),false);
  }finally{globalThis.fetch=previous;}
  assert.throws(()=>parseRoutes({...base,pagination:{...base.pagination,page:0}}));
  assert.throws(()=>parseRoutes({...base,pagination:{...base.pagination,total_pages:3}}));
});

test('message connections retain mutation IDs and route catalog protocol metadata', async()=>{
 const {request,defaults}=await import('../src/features/message-connections/api.ts');
 const {parseRoutes}=await import('../src/features/routes/api.ts');
 const prior=globalThis.fetch;const sent:string[]=[];const input={operation_id:'op',expected_revision:0,connection_id:'connection',connection:{id:'connection',name:'Broker',description:'',enabled:false,config:defaults('mqtt')}};
 globalThis.fetch=async(_url,init)=>{sent.push(String(init?.body));if(sent.length===1)throw Error('disconnected');return Response.json({operation_id:'op',revision:1});};
 try{await assert.rejects(request(input));await request(input);assert.equal(sent[0],sent[1]);}finally{globalThis.fetch=prior;}
 const sources=[{connection_id:'rx',name:'Broker',address:'mqtt://localhost:1883/home/+/state',protocol:'MQTT'}];
 const targets=[{connection_id:'tx',destination_id:'tx',name:'Broker',address:'mqtt://localhost:1883/home/heater/set',protocol:'MQTT'}];
 const view=parseRoutes({available:true,reason:null,source_id:'primary',source_address:'/api/webhooks/primary',sources,targets,counts:{},snapshot:{revision:1,routes:[]}});
 assert.deepEqual(view.sources,sources);assert.deepEqual(view.targets,targets);
});

test('command mappings survive edits and explicit removal without dropping filters', async () => {
  const {parseRoutes,routeSave}=await import('../src/features/routes/api.ts');
  const command={text_pointer:'/content',trigger:'/assistant',mode:'prefix',output:{kind:'json',event_type:'agent.request',template:{request:{$command:'arguments'}}}};
  const response={available:true,reason:null,source_id:'in',source_address:'/hook',counts:{},targets:[],snapshot:{revision:1,routes:[{id:'r',name:'Assistant',enabled:false,event_type:null,revision:1,targets:[],command}]}};
  const route=parseRoutes(response).snapshot!.routes[0];
  const saved=routeSave({...route,enabled:true});
  assert.equal(saved.kind,'save');
  if(saved.kind==='save')assert.deepEqual(saved.command,command);
  const removed=routeSave({...route,command:null});
  if(removed.kind==='save')assert.equal(removed.command,null);
  const legacy=routeSave({...route,command:undefined});
  assert.equal(Object.hasOwn(legacy,'command'),false);
  assert.throws(()=>parseRoutes({...response,snapshot:{revision:1,routes:[{...response.snapshot.routes[0],command:{...command,mode:'unknown'}}]}}));
});

import {parseBinding,parseDescriptor,selectRouteConnection,selectRouteTargets} from '../src/features/routes/routing.ts';
test('connector route bindings survive edits and removed destinations cannot retain hidden routing',()=>{
 const source=parseBinding({adapter:'qq',version:1,config:{mode:'groups',ids:['group_A','group_B']}});
 const target=parseBinding({adapter:'mqtt',version:1,config:{mode:'topic',topic:'home/heater/set'}});
 const routing={source,targets:{one:target,two:target}};
 assert.deepEqual(selectRouteConnection(routing),{source:null,targets:routing.targets});
 assert.deepEqual(selectRouteTargets(routing,['two']),{source,targets:{two:target}});
 assert.throws(()=>parseBinding({adapter:'qq',version:2,config:{mode:'all'}}));
 assert.throws(()=>parseDescriptor({adapter:'unknown',version:1,modes:[{id:'x',label:'X',fields:[{key:'cmd',label:'Cmd',control:'execute_script'}]}]}));
});

test('command action selection clears fan-out and requires an explicit sender grant',async()=>{
 const {changeProcessor,selectActionTarget,actionValid}=await import('../src/features/routes/actions.ts');
 const {routeSave,parseRoutes}=await import('../src/features/routes/api.ts');
 const route={id:'r',name:'Command',enabled:false,revision:1,event_type:null,source_id:'qq',targets:[{rule_id:'old',destination_id:'http'}]};
 const target={owner_connection_id:'qq',connection_id:'qq-send',destination_id:'qq-send',name:'QQ',protocol:'QQ',address:'QQ',actions:[{revision:1,target_required:false,consumes_reply:false,definition:{id:'qq.bot_info',description:'查询机器人信息',input_schema:{type:'object'},effect:'ReadOnly'}}]};
 let next=changeProcessor(route,'invoke_capability',[target]);
 assert.equal(next.targets.length,0);assert.equal(actionValid(next),false);
 next=selectActionTarget(next,target);assert.equal(next.processor?.kind,'invoke_capability');assert.equal(actionValid(next),false);
 if(next.processor?.kind!=='invoke_capability')throw Error();
 next={...next,processor:{...next.processor,allowed_senders:['member_A']},command:{...next.command!,trigger:'/bot'}};
 assert.equal(actionValid(next),true);assert.equal(next.targets.length,1);
 const response={available:true,reason:null,source_id:'qq',source_address:'QQ',targets:[target],counts:{},snapshot:{revision:2,routes:[next]}};
 const parsed=parseRoutes(response);assert.deepEqual(routeSave(parsed.snapshot!.routes[0]),routeSave(next));
 assert.equal(parsed.targets![0].actions![0].definition.id,'qq.bot_info');
 assert.throws(()=>parseRoutes({...response,targets:[{...target,actions:[{...target.actions[0],target_required:'false'}]}]}));
 if(next.processor?.kind!=='invoke_capability')throw Error();
 assert.equal(actionValid({...next,processor:{...next.processor,allowed_senders:['member_A','member_A']}}),false);
 assert.equal(actionValid({...next,processor:{...next.processor,allowed_senders:['*']}}),false);
});

test('changing action connection removes stale addressing and cross-bot reply permission',async()=>{
 const {selectActionTarget}=await import('../src/features/routes/actions.ts');
 const route:import('../src/features/routes/api.ts').Route={id:'r',name:'Command',enabled:false,revision:1,event_type:null,source_id:'qq',response:{kind:'source'},processor:{kind:'invoke_capability',tool_id:'qq.bot_info',revision:1,allowed_senders:['user']},targets:[{rule_id:'rule',destination_id:'old'}],routing:{source:null,targets:{old:{adapter:'qq',version:1,config:{mode:'reply'}}}}};
 const next=selectActionTarget(route,{owner_connection_id:'other',connection_id:'new',destination_id:'new',address:'QQ',actions:[{revision:1,target_required:false,consumes_reply:false,definition:{id:'qq.bot_info',description:'Query',input_schema:{},effect:'ReadOnly'}}]});
 assert.deepEqual(next.routing?.targets,{});assert.equal(next.response?.kind,'none');
 assert.deepEqual(next.targets,[{rule_id:'',destination_id:'new'}]);
});

test('direct reply uses the source endpoint and keeps response policy at route level',async()=>{
 const {changeProcessor,actionValid}=await import('../src/features/routes/actions.ts');
 const {routeSave,parseRoutes}=await import('../src/features/routes/api.ts');
 const route={id:'r',name:'Reply',enabled:false,revision:1,event_type:null,source_id:'qq',targets:[{rule_id:'old',destination_id:'http'}]};
 const targets=[{owner_connection_id:'qq',connection_id:'qq-send',destination_id:'qq-send',address:'QQ'}];
 const reply=changeProcessor(route,'respond',targets);
 assert.deepEqual(reply.targets,[{rule_id:'',destination_id:'qq-send'}]);
 assert.deepEqual(reply.response,{kind:'source'});assert.equal(actionValid(reply),true);
 const parsed=parseRoutes({available:true,reason:null,source_id:'qq',source_address:'QQ',targets,counts:{},snapshot:{revision:2,routes:[reply]}});
 assert.deepEqual(routeSave(parsed.snapshot!.routes[0]),routeSave(reply));
 assert.deepEqual(changeProcessor(reply,'forward',targets).response,{kind:'none'});
});

test('QQ credentials are write-only and never persisted in browser recovery storage',async()=>{
 const {defaults,rememberPending,readPending,request}=await import('../src/features/message-connections/api.ts');
 const values=new Map<string,string>();const old=Object.getOwnPropertyDescriptor(globalThis,'sessionStorage');const prior=globalThis.fetch;
 Object.defineProperty(globalThis,'sessionStorage',{configurable:true,value:{getItem:(key:string)=>values.get(key)??null,setItem:(key:string,value:string)=>values.set(key,value),removeItem:(key:string)=>values.delete(key)}});
 const input={operation_id:'op',expected_revision:0,connection_id:'c',connection:{id:'c',name:'Bot',description:'',enabled:false,secret_configured:true,config:defaults('qq_bot')},secret:'test-only-secret'};
 try {
  assert.equal('secret_env' in input.connection.config,false);
  values.set('pending','old');rememberPending('pending',input);assert.equal(values.size,0);
  values.set('pending',JSON.stringify(input));assert.equal(readPending('pending'),null);assert.equal(values.size,0);
  let body='';globalThis.fetch=async(_url,init)=>{body=String(init?.body);return Response.json({operation_id:'op',revision:1});};
  await request(input);assert.equal(JSON.parse(body).secret,input.secret);assert.equal('secret_configured' in JSON.parse(body).connection,false);
  const {secret:_,...withoutSecret}=input;rememberPending('pending',withoutSecret);assert.ok(readPending('pending'));
 }finally{globalThis.fetch=prior;if(old)Object.defineProperty(globalThis,'sessionStorage',old);else Reflect.deleteProperty(globalThis,'sessionStorage');}
});

test('connection form reports each invalid QQ field and requires secret again after AppID change',async()=>{
 const {validateConnection,formMessages}=await import('../src/features/message-connections/validation.ts');
 const connection:import('../src/features/message-connections/api.ts').Connection={id:'c',name:' ',description:'',enabled:false,config:{kind:'qq_bot',app_id:'abc',group_openid:'invalid group',allowed_senders:['bad user'],allow_proactive:false}};
 assert.deepEqual(Object.keys(validateConnection(connection,'')).sort(),['name','config.app_id','config.group_openid','config.allowed_senders','secret'].sort());
 const valid={...connection,name:'Bot',secret_configured:true,config:{...connection.config,app_id:'123456',group_openid:'',allowed_senders:[]}} as typeof connection;
 assert.deepEqual(validateConnection(valid,'',valid),{});
 assert.ok(validateConnection({...valid,config:{...valid.config,app_id:'654321'} as typeof valid.config},'',valid).secret);
 assert.ok(validateConnection(valid,'has space',valid).secret);
 assert.ok(formMessages.origin_denied.includes('页面地址'));
 assert.equal('origin_denied' in validateConnection(valid,'valid-secret'),false);
});

test('connection form validates HTTP and MQTT cross-field constraints',async()=>{
 const {validateConnection}=await import('../src/features/message-connections/validation.ts');
 const {defaults}=await import('../src/features/message-connections/api.ts');
 const base={id:'c',name:'Test',description:'',enabled:false};
 const http=defaults('http_client');if(http.kind!=='http_client')throw Error();
 assert.ok(validateConnection({...base,config:{...http,url:'http://localhost:8000'}},'')['config.url']);
 assert.deepEqual(validateConnection({...base,config:{...http,url:'http://localhost:8000',allow_http:true}},''),{});
 const mqtt=defaults('mqtt');if(mqtt.kind!=='mqtt')throw Error();
 const errors=validateConnection({...base,config:{...mqtt,host:'mqtt://host',port:0,subscription:null,publish:null,password_env:'PASSWORD',tls:{ca_env:null,client_cert_env:'CERT',client_key_env:null}}},'');
 for(const path of ['config.host','config.port','config.username','config.subscription','config.tls.client_key_env'])assert.ok(errors[path]);
});

test('connection API preserves structured field errors without rejected input values',async()=>{
 const {request,ConnectionError}=await import('../src/features/message-connections/api.ts');const prior=globalThis.fetch;
 globalThis.fetch=async()=>Response.json({code:'invalid_qq_config',fields:[{path:'config.app_id',code:'invalid_qq_app_id'},null,{path:7}]},{status:400});
 try{await assert.rejects(request(),(e:unknown)=>e instanceof ConnectionError&&e.fields.length===1&&e.fields[0].path==='config.app_id');}finally{globalThis.fetch=prior;}
});


test('generated endpoints follow the browser site protocol, host and port',async()=>{
 const {generatedOrigin}=await import('../src/shared/urls.ts');
 assert.equal(generatedOrigin('http://localhost:5174'),'http://localhost:5174');
 assert.equal(generatedOrigin('https://localhost:8443'),'https://localhost:8443');
 assert.equal(generatedOrigin('http://127.0.0.1:5174'),'http://127.0.0.1:5174');
 assert.equal(generatedOrigin('https://console.example.com'),'https://console.example.com');
 const {address,defaults}=await import('../src/features/message-connections/api.ts');
 const prior=Object.getOwnPropertyDescriptor(globalThis,'window');
 try {
  for(const origin of ['https://synquary.p.kalandraeye.com','http://192.168.1.10:8080','https://console.example.com:8443','http://[::1]:8080']) {
   Object.defineProperty(globalThis,'window',{configurable:true,value:{location:{origin}}});
   const connection={id:'test-id',name:'Test',description:'',enabled:false};
   assert.equal(address({...connection,config:defaults('qq_bot')}),`${origin}/api/qq/test-id/webhook`);
   assert.equal(address({...connection,config:defaults('webhook')}),`${origin}/api/webhooks/test-id`);
   assert.equal(new URL('/mcp/test-id',generatedOrigin()).href,`${origin}/mcp/test-id`);
  }
 } finally {if(prior)Object.defineProperty(globalThis,'window',prior);else Reflect.deleteProperty(globalThis,'window');}
});

test('live connection observations distinguish enabled from connected and validate counters',async()=>{
 const {parseObservations,connectionState}=await import('../src/features/message-connections/observations.ts');
 const {defaults}=await import('../src/features/message-connections/api.ts');
 const c={id:'mqtt',name:'Broker',description:'',enabled:true,config:defaults('mqtt')};
 assert.equal(connectionState(c),'状态未知');
 const observation={status:{state:'reconnecting',code:'mqtt_connection_failed'},received:10,sent:4};
 assert.equal(connectionState(c,observation),'重连中');
 assert.equal(connectionState({...c,enabled:false},observation),'已停止');
 assert.deepEqual(parseObservations({mqtt:observation}),{mqtt:observation});
 assert.equal(parseObservations(undefined),undefined);
 assert.throws(()=>parseObservations({mqtt:{...observation,received:-1}}));
 assert.throws(()=>parseObservations({mqtt:{...observation,status:{state:'healthy'}}}));
});

test('MQTT switch example keeps multiple state groups and validates tool identifiers', async () => {
 const {switchExample,capabilityErrors}=await import('../src/features/message-connections/capabilities.ts');
 const example=switchExample('home/switch/sub','home/switch/pub');
 assert.deepEqual(capabilityErrors(example),{});
 assert.deepEqual(example.resources[0].groups.map(g=>g.id),['basic','power']);
 assert.equal(example.resources[0].groups[0].max_age_ms,0);
 assert.equal(example.resources[0].groups[1].refresh_tool,'query_power');
 assert.equal(example.tools[0].payload.messageId.source,'invocation_id');
 example.tools[1].id=example.tools[0].id;
 assert.ok(capabilityErrors(example)['config.capabilities.tools.1.id']);
 example.resources[0].groups[1].fields[0].pointer='key';
 assert.ok(capabilityErrors(example)['config.capabilities.resources.0.groups.1.fields']);
});
