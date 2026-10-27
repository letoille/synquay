export type Binding={source:'literal';value:unknown}|{source:'argument';pointer:string}|{source:'invocation_id'};
export type MqttTool={id:string;description:string;input_schema:unknown;topic:string;payload:Record<string,Binding>;acknowledgement:null|{topic:string;correlation_pointer:string;result_pointer:string;expected:unknown;timeout_ms:number}};
export type StateField={name:string;pointer:string;value_type:'boolean'|'number'|'string';equals?:unknown};
export type StateGroup={id:string;name:string;topic:string;max_age_ms:number;replace:boolean;fields:StateField[];identity:null|{pointer:string;value:unknown};source_time_pointer:string|null;refresh_tool:string|null};
export type DeviceResource={id:string;name:string;description:string;groups:StateGroup[]};
export type MqttCapabilities={tools:MqttTool[];resources:DeviceResource[]};
export const emptySchema=()=>({type:'object',properties:{},additionalProperties:false});
export const newTool=(topic:string):MqttTool=>({id:'',description:'',input_schema:emptySchema(),topic,payload:{messageId:{source:'invocation_id'}},acknowledgement:null});
export const newGroup=(topic:string):StateGroup=>({id:'',name:'',topic,max_age_ms:60000,replace:false,fields:[],identity:null,source_time_pointer:null,refresh_tool:null});
export function switchExample(pub:string,sub:string):MqttCapabilities {
 const tools:MqttTool[]=[0,1].map(key=>({...newTool(pub),id:key?'switch_on':'switch_off',description:key?'开启智能开关':'关闭智能开关',payload:{type:{source:'literal',value:'event'},key:{source:'literal',value:key},messageId:{source:'invocation_id'}},acknowledgement:{topic:sub,correlation_pointer:'/messageId',result_pointer:'/key',expected:key,timeout_ms:10000}}));
 for(const [id,type,command] of [['query_info','info','info-all'],['query_power','statistic','info-statistic']])tools.push({...newTool(pub),id,description:id==='query_info'?'查询设备基础信息':'查询设备用电信息',payload:{type:{source:'literal',value:type},messageId:{source:'invocation_id'}},acknowledgement:{topic:sub,correlation_pointer:'/messageId',result_pointer:'/commandName',expected:command,timeout_ms:10000}});
 const field=(name:string,value_type:StateField['value_type']):StateField=>({name,pointer:`/${name}`,value_type});
 return {tools,resources:[{id:'switch',name:'智能开关',description:'开关基础信息与用电状态',groups:[
 {...newGroup(sub),id:'basic',name:'基础信息',max_age_ms:0,refresh_tool:'query_info',fields:[field('mac','string'),field('type','string'),field('version','string'),field('ip','string'),field('ssid','string')]},
 {...newGroup(sub),id:'power',name:'用电状态',refresh_tool:'query_power',fields:[{name:'power_on',pointer:'/key',value_type:'boolean',equals:1},field('voltage','number'),field('current','number'),field('power','number'),field('energy','number')]}
 ]}]};
}
const id=(s:string)=>/^[a-zA-Z0-9_.-]{1,40}$/.test(s);
const ptr=(s:string)=>s.startsWith('/')&&!/~(?:[^01]|$)/.test(s);
export function capabilityErrors(c:MqttCapabilities):Record<string,string>{
 const errors:Record<string,string>={};const names=new Set<string>();
 c.tools.forEach((v,i)=>{const p=`config.capabilities.tools.${i}`;if(!id(v.id)||names.has(v.id))errors[`${p}.id`]='标识必填且不能重复';names.add(v.id);if(!v.description.trim())errors[`${p}.description`]='请填写说明';if(!v.topic||/[+#]/.test(v.topic))errors[`${p}.topic`]='请填写有效的发布 Topic';if(!v.input_schema||typeof v.input_schema!=='object'||Array.isArray(v.input_schema))errors[`${p}.input_schema`]='请输入有效的 JSON 对象';if(Object.keys(v.payload).length===0)errors[`${p}.payload`]='至少添加一个消息字段';Object.entries(v.payload).forEach(([key,b])=>{if(!key||(b.source==='argument'&&!ptr(b.pointer)))errors[`${p}.payload`]='请检查消息字段和参数路径';});if(v.acknowledgement&&(!v.acknowledgement.topic||v.acknowledgement.topic===v.topic||!ptr(v.acknowledgement.correlation_pointer)||!ptr(v.acknowledgement.result_pointer)))errors[`${p}.acknowledgement`]='请检查回执 Topic 和字段路径';});
 c.resources.forEach((v,i)=>{const p=`config.capabilities.resources.${i}`;if(!id(v.id)||names.has(`state_${v.id}`))errors[`${p}.id`]='标识必填且不能重复';names.add(`state_${v.id}`);if(!v.name.trim())errors[`${p}.name`]='请填写名称';if(!v.groups.length)errors[`${p}.groups`]='至少添加一个状态分组';const groups=new Set<string>();v.groups.forEach((g,j)=>{const q=`${p}.groups.${j}`;if(!id(g.id)||groups.has(g.id))errors[`${q}.id`]='标识必填且不能重复';groups.add(g.id);if(!g.name.trim())errors[`${q}.name`]='请填写名称';if(!g.topic||/[+#]/.test(g.topic))errors[`${q}.topic`]='请填写有效的发布 Topic';if(!g.fields.length)errors[`${q}.fields`]='至少添加一个状态字段';const fields=new Set<string>();g.fields.forEach(f=>{if(!id(f.name)||fields.has(f.name)||!ptr(f.pointer))errors[`${q}.fields`]='请检查字段名称和 JSON 路径';fields.add(f.name);});});});return errors;
}
