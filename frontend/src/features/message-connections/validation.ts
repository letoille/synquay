import {capabilityErrors} from './capabilities.ts';
import type {Connection} from './api';
export type FieldErrors=Record<string,string>;
export type ServerFieldError={path:string;code:string};
const bytes=(s:string)=>new TextEncoder().encode(s).length;
const env=/^[A-Za-z_][A-Za-z0-9_]{0,127}$/;
const openid=/^[A-Za-z0-9_-]{1,128}$/;
const eventType=(s:string)=>!!s.trim()&&bytes(s)<=128&&!/[\x00-\x1f\x7f-\x9f]/.test(s);
export function validateConnection(connection:Connection,secret:string,original?:Connection):FieldErrors {
 const errors:FieldErrors={};const c=connection.config;
 if(!connection.name.trim())errors.name='请填写名称。';
 else if(bytes(connection.name)>120||/[\x00-\x1f\x7f]/.test(connection.name))errors.name='名称过长或含有控制字符。';
 if(bytes(connection.description)>4000)errors.description='说明不能超过 4000 字节。';
 const envField=(path:string,value:string|null|undefined,required=false)=>{if(required||value!==null&&value!==undefined){if(!value||!env.test(value))errors[path]='请填写有效的环境变量名称，以字母或下划线开头。';}};
 if(c.kind==='qq_bot') {
  if(!/^[0-9]{1,32}$/.test(c.app_id))errors['config.app_id']='AppID 必须为 1–32 位数字。';
  const retained=original?.secret_configured&&original.config.kind==='qq_bot'&&original.config.app_id===c.app_id;
  if(!secret&&!retained)errors.secret='请填写此 AppID 对应的 AppSecret。';
  else if(secret&&(bytes(secret)>4096||!/^[\x21-\x7e]+$/.test(secret)))errors.secret='密钥不能包含空格、换行或非 ASCII 字符。';
  if(c.group_openid&&!openid.test(c.group_openid))errors['config.group_openid']='群 OpenID 只能包含字母、数字、下划线和短横线，最多 128 字符。';
  const members=c.allowed_senders.map(s=>s.trim()).filter(Boolean);
  if(members.length>100||members.some(s=>!openid.test(s)))errors['config.allowed_senders']='最多填写 100 个有效的成员 OpenID，使用英文逗号分隔。';
 } else if(c.kind==='http_client') {
  try {const u=new URL(c.url);if(!u.hostname||u.username||u.password||u.hash||!(u.protocol==='https:'||u.protocol==='http:'&&c.allow_http)||bytes(c.url)>2048)throw Error();}
  catch {errors['config.url']='填写完整的 HTTPS 地址；使用 HTTP 时需勾选允许普通 HTTP。';}
  if(!['POST','PUT','PATCH'].includes(c.method))errors['config.method']='请选择 POST、PUT 或 PATCH。';
  if(c.auth.kind==='basic'){
   if(!c.auth.username||c.auth.username.includes(':')||bytes(c.auth.username)>256)errors['config.auth.username']='请填写用户名，不能包含冒号，最多 256 字节。';
   envField('config.auth.password_env',c.auth.password_env,true);
  }else if(c.auth.kind==='bearer')envField('config.auth.token_env',c.auth.token_env,true);
 } else if(c.kind==='webhook') {
  envField('config.secret_env',c.secret_env,true);
  if(c.policy.format.kind==='json'){
   if(!eventType(c.policy.format.event_type))errors['config.policy.format.event_type']='请填写事件类型，不能包含控制字符，最多 128 字节。';
   if((!/^x-[a-z0-9-]{0,62}$/.test(c.policy.format.id_header)||['x-synquay-timestamp','x-synquay-signature'].includes(c.policy.format.id_header)))errors['config.policy.format.id_header']='请求头需以 x- 开头，仅含小写字母、数字和短横线，不能使用签名专用头。';
  }
 }else{
  if(!c.host||bytes(c.host)>253||/[\s/@?#]/.test(c.host))errors['config.host']='只填写 Broker 主机名或 IP，不包含协议、端口或路径。';
  if(!Number.isInteger(c.port)||c.port<1||c.port>65535)errors['config.port']='端口必须为 1–65535 的整数。';
  if(c.username!==null&&(!c.username||bytes(c.username)>256||c.username.includes('\0')))errors['config.username']='用户名不能为空或含有空字符，最多 256 字节。';
  if(c.password_env&&!c.username)errors['config.username']='配置密码时需要填写用户名。';
  envField('config.password_env',c.password_env);
  if(!c.subscription&&!c.publish)errors['config.subscription']='至少启用订阅接收或发布发送。';
  for(const [type,item] of [['subscription',c.subscription],['publish',c.publish]] as const){
   if(!item)continue;
   const levels=item.topic.split('/');
   const valid=!!item.topic&&bytes(item.topic)<=512&&!item.topic.includes('\0')&&(type==='publish'?!/[#+]/.test(item.topic):levels.every((s,i)=>(!s.includes('+')||s==='+')&&(!s.includes('#')||s==='#'&&i===levels.length-1)));
   if(!valid)errors[`config.${type}.topic`]=type==='publish'?'请填写发布 Topic，不能包含 + 或 # 通配符。':'请填写订阅 Topic；+ 占一个层级，# 只能位于最后一个层级。';
   if(![0,1].includes(item.qos))errors[`config.${type}.qos`]='QoS 仅支持 0 或 1。';
  }
  if(c.subscription&&!eventType(c.subscription.event_type))errors['config.subscription.event_type']='请填写事件类型，不能包含控制字符，最多 128 字节。';
  if(c.tls){
   envField('config.tls.ca_env',c.tls.ca_env);
   envField('config.tls.client_cert_env',c.tls.client_cert_env);
   envField('config.tls.client_key_env',c.tls.client_key_env);
   if(!!c.tls.client_cert_env!==!!c.tls.client_key_env)errors[c.tls.client_cert_env?'config.tls.client_key_env':'config.tls.client_cert_env']='客户端证书和私钥必须同时配置。';
  }
 }
 if(c.kind==='mqtt'&&c.capabilities)Object.assign(errors,capabilityErrors(c.capabilities));
 return errors;
}
export const fieldMessages:Record<string,string>={invalid_mqtt_capabilities:'请检查工具和资源状态配置。',unsupported_schema:'参数定义只能使用本地 JSON Schema 引用。',invalid_schema:'参数定义不是有效的 JSON Schema。',resource_topic_not_subscribed:'状态分组的 Topic 必须包含在订阅范围内。',
 invalid_event_type:'请填写事件类型，不能包含控制字符，最多 128 字节。',invalid_event_header:'请求头需以 x- 开头，仅含小写字母、数字和短横线，不能使用签名专用头。',invalid_qos:'QoS 仅支持 0 或 1。',invalid_subscription_topic:'请填写订阅 Topic；+ 占一个层级，# 只能位于最后一个层级。',invalid_publish_topic:'请填写发布 Topic，不能包含 + 或 # 通配符。',

 invalid_connection_name:'名称过长或含有控制字符。',invalid_connection_description:'说明不能超过 4000 字节。',
 invalid_qq_app_id:'AppID 必须为 1–32 位数字。',duplicate_qq_app:'此 AppID 已有 QQ 连接，请编辑已有连接。',qq_secret_required:'请填写此 AppID 对应的 AppSecret。',invalid_qq_secret:'密钥不能包含空格、换行或非 ASCII 字符。',invalid_qq_group:'群 OpenID 格式不正确。',invalid_qq_members:'成员 OpenID 格式不正确或超过 100 个。',
 invalid_secret_reference:'请填写有效的环境变量名称，以字母或下划线开头。',missing_connection_secret:'后端未配置此环境变量。',invalid_connection_secret:'此环境变量的值为空或过长。',
 invalid_http_url:'填写完整的 HTTPS 地址；使用 HTTP 时需勾选允许普通 HTTP。',invalid_http_method:'请选择 POST、PUT 或 PATCH。',invalid_http_username:'用户名不能包含冒号，最多 256 字节。',invalid_http_auth:'此认证信息无法构成有效的 HTTP 请求头。',
 invalid_mqtt_host:'只填写 Broker 主机名或 IP，不包含协议、端口或路径。',invalid_mqtt_port:'端口必须为 1–65535 的整数。',invalid_mqtt_username:'用户名不能为空或含有空字符，最多 256 字节。',invalid_mqtt_password:'此环境变量中的密码无效。',mqtt_receive_or_send_required:'至少启用订阅接收或发布发送。',mqtt_username_required:'配置密码时需要填写用户名。',invalid_mqtt_subscription:'请检查订阅 Topic、QoS 和事件类型。',invalid_mqtt_publish:'请检查发布 Topic 和 QoS。',mqtt_topic_loop:'发布 Topic 不能匹配同一连接的订阅 Topic。',mqtt_client_certificate_pair:'客户端证书和私钥必须同时配置。',invalid_mqtt_ca:'此环境变量中的 CA 证书无效。',invalid_mqtt_certificate:'客户端证书无效或与私钥不匹配。',invalid_mqtt_key:'此环境变量中的私钥无效。',invalid_webhook_secret:'Webhook 密钥须为 32–4096 个可见 ASCII 字符。',
};
export const formMessages:Record<string,string>={invalid_mqtt_capabilities:'请检查工具和资源状态配置。',resource_topic_not_subscribed:'状态分组的 Topic 必须包含在订阅范围内。',origin_denied:'当前页面地址未被后端允许，请使用配置的管理台地址或配置允许的来源。',permission_denied:'当前操作没有权限。',invalid_input:'请求格式不兼容，请刷新页面并确认前后端版本一致。',version_conflict:'配置已更新，请刷新后重试。',connection_has_pending_work:'连接仍有待处理消息，请处理后再修改或删除。',rate_limited:'请求过于频繁，请稍后重试。',payload_too_large:'提交内容过大，请缩短配置内容。',not_found:'此连接已不存在，请刷新列表。',not_supported:'当前后端不支持此操作，请检查服务版本。',invalid_connection_id:'连接编号无效，请重新打开表单。'};
