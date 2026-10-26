import { useEffect, useState, useRef } from 'react';
import { t } from '../../shared/i18n';
import { Input } from '../../shared/ui/Controls';
import type { CommandMapping } from './api';

export function CommandFields({value,onChange,onValidity,action=false,response=false}:{response?:boolean;action?:boolean;value:CommandMapping;onChange:(v:CommandMapping)=>void;onValidity:(valid:boolean)=>void}) {
  const [template,setTemplate]=useState(()=>value.output.kind==='json'?JSON.stringify(value.output.template,null,2):'{"input":{"$command":"arguments"}}');
  const [jsonValid,setJsonValid]=useState(true);
  const external=value.output.kind==='json'?JSON.stringify(value.output.template,null,2):'';
  const previous=useRef(external);
  useEffect(()=>{if(previous.current!==external){setTemplate(external);setJsonValid(true);previous.current=external;}},[external]);
  const valid=!!value.trigger.trim()&&value.trigger===value.trigger.trim()&&(value.text_pointer===''||value.text_pointer.startsWith('/'))&&(value.output.kind==='original'||(jsonValid&&!!value.output.event_type.trim()));
  useEffect(()=>{onValidity(valid);return()=>onValidity(true);},[valid,onValidity]);
  return <div className="command-fields">
    <small>{t('所有命中的路由都会执行。')}</small>
    <label><span>{t('匹配方式')}</span><select className="ui-input" value={value.mode} onChange={e=>onChange({...value,mode:e.target.value as CommandMapping['mode']})}><option value="exact">{t('全文匹配')}</option><option value="prefix">{t('指令与参数')}</option></select></label>
    <label><span>{t('指令内容')}</span><Input maxLength={128} placeholder="/assistant" value={value.trigger} onChange={e=>onChange({...value,trigger:e.target.value})}/></label>
    <label><span>{t('消息字段')}</span><Input maxLength={256} placeholder="/content" value={value.text_pointer} onChange={e=>onChange({...value,text_pointer:e.target.value})}/></label>
    <small>{t('JSON Pointer，例如 /content 或 /text；留空读取 JSON 字符串。')}</small>
    {!action&&<label><span>{t('发送内容')}</span><select className="ui-input" value={value.output.kind} onChange={e=>{setJsonValid(true);onChange({...value,output:e.target.value==='original'?{kind:'original'}:{kind:'json',event_type:'command.request',template:{input:{$command:'arguments'}}}});setTemplate('{"input":{"$command":"arguments"}}');}}><option value="original">{t('原始内容')}</option><option value="json">{t('映射为 JSON')}</option></select></label>}
    {value.output.kind==='json'&&<>
      {!action&&<label><span>{t('输出事件类型')}</span><Input maxLength={128} value={value.output.event_type} onChange={e=>{if(value.output.kind==='json')onChange({...value,output:{...value.output,event_type:e.target.value}});}}/></label>}
      <label><span>{t(response?'回复内容':action?'调用参数':'消息模板')}</span><textarea className="ui-input mono" rows={5} maxLength={8192} value={template} aria-invalid={!jsonValid} onChange={e=>{setTemplate(e.target.value);try{const next:unknown=JSON.parse(e.target.value);previous.current=JSON.stringify(next,null,2);setJsonValid(true);if(value.output.kind==='json')onChange({...value,output:{...value.output,template:next}});}catch{setJsonValid(false);}}}/></label>
      {!jsonValid&&<small className="error" role="alert">{t('请输入有效的 JSON。')}</small>}
      <small>{t('参数使用 {"$command":"arguments"}，原消息字段使用 {"$payload":"/字段"}。')}</small>
    </>}
  </div>;
}
