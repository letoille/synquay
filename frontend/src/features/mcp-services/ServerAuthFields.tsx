import { Input, Checkbox } from '../../shared/ui/Controls';
import { t } from '../../shared/i18n';
import type { ServerAuth } from './types';
export function authLabel(mode: string) {
  return mode === 'basic' ? 'Basic Auth' : mode === 'jwt' ? 'JWT' : mode === 'oauth' ? 'OAuth' : t(mode === 'none' ? '无鉴权' : '令牌认证');
}
export function newAuth(mode: ServerAuth['mode']): ServerAuth {
  if (mode === 'basic') return {mode, username:'',password_env:''};
  if (mode === 'jwt') return {mode, issuer:'',audience:'',subject:'',public_key_pem:''};
  if (mode === 'oauth') return {mode,issuer:'',resource:'',introspection_endpoint:'',introspection_client_id:'',introspection_secret_env:'',required_scope:'mcp',subject:'',oauth_client_id:'',allow_loopback_http:false};
  return {mode};
}
export function ServerAuthFields({value, change, existingMode}: {value:ServerAuth;change:(auth:ServerAuth)=>void;existingMode?:string}) {
  const labels: Record<string,string> = {username:'用户名', password_env:'密码环境变量', issuer:'签发者 Issuer', audience:'目标资源 Audience', subject:'允许的主体 Subject', public_key_pem:'RSA 公钥 PEM', resource:'公开 MCP 地址', introspection_endpoint:'令牌验证端点', introspection_client_id:'验证端点 Client ID', introspection_secret_env:'客户端密钥环境变量', required_scope:'所需 Scope', oauth_client_id:'允许的 OAuth Client ID'};
  return <div className="auth-fields">
    <label className="ui-field"><span>{t('认证方式')}</span><select className="ui-input" value={value.mode === 'inherited' ? '' : value.mode} required={!existingMode} onChange={e=>change(newAuth(e.target.value as ServerAuth['mode']))}>
      <option value="" disabled hidden>{existingMode ? authLabel(existingMode) : t('选择认证方式')}</option><option value="basic">Basic Auth</option><option value="jwt">JWT · RS256</option><option value="oauth">OAuth</option><option value="none">{t('无鉴权')}</option>
    </select></label>
    {Object.entries(value).filter(([key])=>key!=='mode' && key!=='allow_loopback_http' && key!=='resource').map(([key,field])=><label className="ui-field" key={key}><span>{t(labels[key])}</span>{key==='public_key_pem' ? <textarea className="ui-input mono" rows={5} required maxLength={8192} value={String(field)} onChange={e=>change({...value,[key]:e.target.value} as ServerAuth)} /> : <Input required maxLength={key==='username'?256:2048} autoComplete="off" value={String(field)} onChange={e=>change({...value,[key]:e.target.value} as ServerAuth)} />}</label>)}
    {value.mode==='oauth'&&<Checkbox checked={value.allow_loopback_http} onCheckedChange={checked=>change({...value,allow_loopback_http:checked===true})}>{t('允许本机 HTTP 授权服务')}</Checkbox>}
  </div>;
}
