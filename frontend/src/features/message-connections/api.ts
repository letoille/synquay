import { generatedOrigin } from '../../shared/urls.ts';
export type BodyMode='payload'|'event';
export type HttpAuth={kind:'none'}|{kind:'bearer';token_env:string}|{kind:'basic';username:string;password_env:string};
export type Config=
 |{kind:'qq_bot';app_id:string;group_openid:string;allowed_senders:string[];allow_proactive:boolean}
 |{kind:'webhook';secret_env:string;policy:{authentication:'bearer'|'hmac_sha256';format:{kind:'envelope'}|{kind:'json';event_type:string;id_header:string}}}
 |{kind:'http_client';url:string;method:string;allow_http:boolean;auth:HttpAuth;body:BodyMode}
 |{kind:'mqtt';host:string;port:number;username:string|null;password_env:string|null;tls:null|{ca_env:string|null;client_cert_env:string|null;client_key_env:string|null};subscription:null|{topic:string;qos:number;event_type:string;include_metadata?:boolean};publish:null|{topic:string;qos:number;retain:boolean;body:BodyMode}};
export type Connection={id:string;name:string;description:string;enabled:boolean;config:Config;secret_configured?:boolean};
export type Snapshot={revision:number;connections:Connection[]};
export type Input={operation_id:string;expected_revision:number;connection_id:string;connection:Connection|null;secret?:string};
export class ConnectionError extends Error {constructor(public status:number,public code:string,public fields:{path:string;code:string}[]=[]){super(code);}}
export async function request(input?:Input):Promise<{available?:boolean;snapshot?:Snapshot;revision?:number;operation_id?:string}> {
 const response=await fetch('/api/message-connections',{method:input?'POST':'GET',cache:'no-store',credentials:'omit',redirect:'error',signal:AbortSignal.timeout(40000),headers:{Accept:'application/json',...(input?{'Content-Type':'application/json'}:{})},...(input?{body:JSON.stringify(input,(key,value)=>key==='secret_configured'?undefined:value)}:{})});
 const value=await response.json();if(!response.ok)throw new ConnectionError(response.status,typeof value.code==='string'?value.code:'request_failed',Array.isArray(value.fields)?value.fields.filter((f:unknown)=>f!==null&&typeof f==='object'&&typeof (f as {path:unknown}).path==='string'&&typeof (f as {code:unknown}).code==='string'):[]);
 if(input){if(value.operation_id!==input.operation_id||!Number.isSafeInteger(value.revision))throw new Error('invalid_response');}
 else if(typeof value.available!=='boolean'||(value.available&&(!Number.isSafeInteger(value.snapshot?.revision)||!Array.isArray(value.snapshot?.connections))))throw new Error('invalid_response');
 return value;
}
export function defaults(kind:Config['kind']):Config {
 if(kind==='qq_bot')return{kind,app_id:'',group_openid:'',allowed_senders:[],allow_proactive:false};
 if(kind==='webhook')return{kind,secret_env:'',policy:{authentication:'bearer',format:{kind:'json',event_type:'message.received',id_header:'x-event-id'}}};
 if(kind==='http_client')return{kind,url:'',method:'POST',allow_http:false,auth:{kind:'none'},body:'payload'};
 return{kind,host:'',port:1883,username:null,password_env:null,tls:null,subscription:{topic:'',qos:1,event_type:'mqtt.message',include_metadata:true},publish:null};
}
export function address(c:Connection):string {const f=c.config;return f.kind==='qq_bot'?`${generatedOrigin()}/api/qq/${c.id}/webhook`:f.kind==='webhook'?`${generatedOrigin()}/api/webhooks/${c.id}`:f.kind==='http_client'?f.url:`${f.tls?'mqtts':'mqtt'}://${f.host}:${f.port}`;}
export function rememberPending(key:string,input:Input):void {
 // Credentials live only in this form/request until completion, never browser storage.
 if(input.secret!==undefined)sessionStorage.removeItem(key);
 else sessionStorage.setItem(key,JSON.stringify(input));
}
export function readPending(key:string):Input|null {try{const input=JSON.parse(sessionStorage.getItem(key)??'null') as Input|null;if(input&&Object.hasOwn(input,'secret')){sessionStorage.removeItem(key);return null;}return input&&typeof input.operation_id==='string'&&typeof input.connection_id==='string'&&Number.isSafeInteger(input.expected_revision)?input:null;}catch{return null;}}
