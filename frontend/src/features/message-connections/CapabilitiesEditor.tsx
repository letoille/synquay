import {useEffect,useState} from 'react';
import {Button,Input,Checkbox} from '../../shared/ui/Controls';
import {IconButton} from '../../shared/ui/IconButton';
import {Icon} from '../../shared/ui/Icon';
import {Field,FormIssue} from './FormField';
import {t} from '../../shared/i18n';
import {type MqttCapabilities,type MqttTool,type StateGroup,newTool,newGroup,switchExample} from './capabilities';
function JsonValue({value,change,object=false}:{value:unknown;change:(v:unknown)=>void;object?:boolean}){
 const [draft,setDraft]=useState<string|null>(null);const [error,setError]=useState(false);
 useEffect(()=>{setDraft(null);setError(false);},[value]);
 return <div><Input aria-label={t(object?'JSON 参数定义':'JSON 值')} aria-invalid={error} data-invalid-json={error?'true':undefined} value={draft??JSON.stringify(value)} onChange={e=>{setDraft(e.target.value);try{const v=JSON.parse(e.target.value);if(object&&(!v||typeof v!=='object'||Array.isArray(v)))throw Error();setError(false);change(v);}catch{setError(true);}}}/>{error&&<small role="alert" className="error">{t('请输入有效的 JSON 值')}</small>}</div>
}
export function CapabilitiesEditor({value,change,pub,sub}:{value:MqttCapabilities;change:(v:MqttCapabilities)=>void;pub:string;sub:string}){
 const tool=(i:number,v:MqttTool)=>change({...value,tools:value.tools.map((o,j)=>j===i?v:o)});
 return <div className="capability-editor"><FormIssue name="config.capabilities"/>
 <div className="section-bar"><h3>{t('工具')}</h3><Button disabled={value.tools.length+value.resources.length>=32} onClick={()=>change({...value,tools:[...value.tools,newTool(pub)]})}>{t('添加工具')}</Button></div>
 {!value.tools.length&&!value.resources.length&&<Button onClick={()=>change(switchExample(pub,sub))}>{t('填入智能开关示例')}</Button>}
 {value.tools.map((v,i)=>{const p=`config.capabilities.tools.${i}`;return <details className="capability-panel" key={i} ref={element=>{if(element&&!element.dataset.initialized){element.open=!v.id;element.dataset.initialized='true';}}}><summary><span>{v.id||t('新工具')}</span><small>{v.description}</small></summary><div className="capability-panel-body">
 <Field name={`${p}.id`} label="工具标识"><Input value={v.id} onChange={e=>tool(i,{...v,id:e.target.value})}/></Field>
 <Field name={`${p}.description`} label="说明"><Input value={v.description} onChange={e=>tool(i,{...v,description:e.target.value})}/></Field>
 <Field name={`${p}.topic`} label="发布 Topic"><Input value={v.topic} onChange={e=>tool(i,{...v,topic:e.target.value})}/></Field>
 <details className="capability-advanced"><summary>{t('高级设置')}</summary><Field name={`${p}.input_schema`} label="输入参数"><JsonValue object value={v.input_schema} change={input_schema=>tool(i,{...v,input_schema})}/></Field></details>
 <FormIssue name={`${p}.payload`}/><div className="section-bar"><strong>{t('消息字段')}</strong><Button onClick={()=>{let key='field';while(Object.hasOwn(v.payload,key))key+='1';tool(i,{...v,payload:{...v.payload,[key]:{source:'literal',value:''}}});}}>{t('添加字段')}</Button></div>
 {Object.entries(v.payload).map(([key,b],j)=><div className="capability-mapping" key={j}>
 <Input aria-label={t('字段名称')} placeholder={t('字段名称')} value={key} onChange={e=>{if(e.target.value!==key&&Object.hasOwn(v.payload,e.target.value))return;tool(i,{...v,payload:Object.fromEntries(Object.entries(v.payload).map(([k,val])=>[k===key?e.target.value:k,val]))});}}/>
 <select className="ui-input" aria-label={t('值来源')} value={b.source} onChange={e=>tool(i,{...v,payload:{...v.payload,[key]:e.target.value==='literal'?{source:'literal',value:''}:e.target.value==='argument'?{source:'argument',pointer:'/value'}:{source:'invocation_id'}}})}><option value="literal">{t('固定值')}</option><option value="argument">{t('工具参数')}</option><option value="invocation_id">{t('调用 ID')}</option></select>
 {b.source==='literal'?<JsonValue value={b.value} change={val=>tool(i,{...v,payload:{...v.payload,[key]:{source:'literal',value:val}}})}/>:b.source==='argument'?<Input aria-label={t('参数路径')} value={b.pointer} onChange={e=>tool(i,{...v,payload:{...v.payload,[key]:{...b,pointer:e.target.value}}})}/>:<span className="muted">{t('自动生成')}</span>}
 <IconButton label={t('删除')} onClick={()=>tool(i,{...v,payload:Object.fromEntries(Object.entries(v.payload).filter(([k])=>k!==key))})}><Icon name="trash" size={16}/></IconButton></div>)}
 <Field label="完成条件"><select className="ui-input" value={v.acknowledgement?'confirmed':'submitted'} onChange={e=>tool(i,{...v,acknowledgement:e.target.value==='confirmed'?{topic:sub,correlation_pointer:'/messageId',result_pointer:'/key',expected:0,timeout_ms:10000}:null})}><option value="submitted">{t('Broker 已接受')}</option><option value="confirmed">{t('设备回报确认')}</option></select></Field>
 <FormIssue name={`${p}.acknowledgement`}/>{v.acknowledgement&&<>
 {(['topic','correlation_pointer','result_pointer'] as const).map((k,j)=><Field key={k} label={['回执 Topic','关联 ID 路径','结果字段路径'][j]}><Input value={v.acknowledgement![k]} onChange={e=>tool(i,{...v,acknowledgement:{...v.acknowledgement!,[k]:e.target.value}})}/></Field>)}
 <Field label="期望值"><JsonValue value={v.acknowledgement.expected} change={expected=>tool(i,{...v,acknowledgement:{...v.acknowledgement!,expected}})}/></Field>
 <Field label="超时（毫秒）"><Input type="number" min={100} max={15000} value={v.acknowledgement.timeout_ms} onChange={e=>tool(i,{...v,acknowledgement:{...v.acknowledgement!,timeout_ms:Number(e.target.value)}})}/></Field></>}
 <div className="capability-panel-footer"><Button variant="ghost" className="capability-delete" onClick={()=>change({...value,tools:value.tools.filter((_,j)=>j!==i)})}><Icon name="trash" size={15}/>{t('删除工具')}</Button></div>
 </div></details>})}
 <div className="section-bar"><h3>{t('资源状态')}</h3><Button disabled={value.tools.length+value.resources.length>=32} onClick={()=>change({...value,resources:[...value.resources,{id:'',name:'',description:'',groups:[newGroup(sub)]}]})}>{t('添加资源')}</Button></div>
 {value.resources.map((r,i)=>{const p=`config.capabilities.resources.${i}`;const update=(v:typeof r)=>change({...value,resources:value.resources.map((o,j)=>j===i?v:o)});return <details className="capability-panel" key={i} ref={element=>{if(element&&!element.dataset.initialized){element.open=!r.id;element.dataset.initialized='true';}}}><summary><span>{r.name||t('新资源')}</span><small>{r.description}</small></summary><div className="capability-panel-body">
 <Field name={`${p}.id`} label="资源标识"><Input value={r.id} onChange={e=>update({...r,id:e.target.value})}/></Field><Field name={`${p}.name`} label="名称"><Input value={r.name} onChange={e=>update({...r,name:e.target.value})}/></Field><Field label="说明"><Input value={r.description} onChange={e=>update({...r,description:e.target.value})}/></Field>
 <FormIssue name={`${p}.groups`}/>{r.groups.map((g,j)=><Group key={j} path={`${p}.groups.${j}`} value={g} tools={value.tools} change={v=>update({...r,groups:r.groups.map((o,k)=>k===j?v:o)})} remove={()=>update({...r,groups:r.groups.filter((_,k)=>k!==j)})}/>)}
 <div className="capability-panel-footer"><Button disabled={r.groups.length>=8} onClick={()=>update({...r,groups:[...r.groups,newGroup(sub)]})}>{t('添加状态分组')}</Button><Button variant="ghost" className="capability-delete" onClick={()=>change({...value,resources:value.resources.filter((_,j)=>j!==i)})}><Icon name="trash" size={15}/>{t('删除资源')}</Button></div></div></details>})}
 </div>
}
function Group({value:g,change,path,tools,remove}:{value:StateGroup;change:(v:StateGroup)=>void;path:string;tools:MqttTool[];remove:()=>void}){
 return <details className="capability-panel capability-group" ref={element=>{if(element&&!element.dataset.initialized){element.open=!g.id;element.dataset.initialized='true';}}}><summary><span>{g.name||t('状态分组')}</span><small>{g.topic}</small></summary><div className="capability-panel-body">
 <Field name={`${path}.id`} label="分组标识"><Input value={g.id} onChange={e=>change({...g,id:e.target.value})}/></Field><Field name={`${path}.name`} label="名称"><Input value={g.name} onChange={e=>change({...g,name:e.target.value})}/></Field><Field name={`${path}.topic`} label="上报 Topic"><Input value={g.topic} onChange={e=>change({...g,topic:e.target.value})}/></Field>
 <Field label="有效期（毫秒，0 不过期）"><Input type="number" min={0} max={86400000} value={g.max_age_ms} onChange={e=>change({...g,max_age_ms:Number(e.target.value)})}/></Field>
 <Field label="刷新工具"><select className="ui-input" value={g.refresh_tool??''} onChange={e=>change({...g,refresh_tool:e.target.value||null})}><option value="">{t('无')}</option>{tools.filter(t=>t.acknowledgement).map(t=><option key={t.id} value={t.id}>{t.id}</option>)}</select></Field>
 <details className="capability-advanced"><summary>{t('高级设置')}</summary><Field label="设备时间路径（毫秒）"><Input value={g.source_time_pointer??''} placeholder="/timestamp" onChange={e=>change({...g,source_time_pointer:e.target.value||null})}/></Field>
 <Checkbox checked={g.replace} onCheckedChange={v=>change({...g,replace:v===true})}>{t('整体替换分组；默认按字段合并')}</Checkbox>
 <Checkbox checked={!!g.identity} onCheckedChange={v=>change({...g,identity:v?{pointer:'/mac',value:''}:null})}>{t('仅匹配指定报文')}</Checkbox>
 {g.identity&&<div className="capability-mapping"><Input aria-label={t('匹配字段路径')} value={g.identity.pointer} onChange={e=>change({...g,identity:{...g.identity!,pointer:e.target.value}})}/><JsonValue value={g.identity.value} change={value=>change({...g,identity:{...g.identity!,value}})}/></div>}
 </details><div className="capability-subheading"><strong>{t('状态字段')}</strong><Button disabled={g.fields.length>=32} onClick={()=>change({...g,fields:[...g.fields,{name:'',pointer:'/',value_type:'number'}]})}>{t('添加状态字段')}</Button></div><FormIssue name={`${path}.fields`}/>{g.fields.map((f,i)=><div className="capability-mapping capability-state-field" key={i}><Input aria-label={t('状态字段名称')} placeholder={t('状态字段名称')} value={f.name} onChange={e=>change({...g,fields:g.fields.map((o,j)=>j===i?{...f,name:e.target.value}:o)})}/><Input aria-label={t('上报 JSON 路径')} placeholder={t('上报 JSON 路径')} value={f.pointer} onChange={e=>change({...g,fields:g.fields.map((o,j)=>j===i?{...f,pointer:e.target.value}:o)})}/><select className="ui-input" aria-label={t('数据类型')} value={f.value_type} onChange={e=>change({...g,fields:g.fields.map((o,j)=>j===i?{...f,value_type:e.target.value as typeof f.value_type,equals:undefined}:o)})}><option value="boolean">Boolean</option><option value="number">Number</option><option value="string">String</option></select>
 {f.value_type==='boolean'&&<label>{t('等于时为真')}<JsonValue value={f.equals??true} change={equals=>change({...g,fields:g.fields.map((o,j)=>j===i?{...f,equals}:o)})}/></label>}
 <IconButton label={t('删除')} onClick={()=>change({...g,fields:g.fields.filter((_,j)=>j!==i)})}><Icon name="trash" size={16}/></IconButton></div>)}
 <div className="capability-panel-footer"><Button variant="ghost" className="capability-delete" onClick={remove}><Icon name="trash" size={15}/>{t('删除分组')}</Button></div>
 </div></details>
}
