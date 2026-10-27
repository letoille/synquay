import {useEffect,useState} from 'react';
import {Dialog} from 'radix-ui';
import {Button} from '../../shared/ui/Controls';
import {t} from '../../shared/i18n';
type Field={value:unknown;freshness:string;received_at_ms?:number;source_at_ms?:number;origin?:string};
type States=Record<string,{name:string;groups:Record<string,{name:string;fields:Record<string,Field>}>}>;
export function ResourceStates({id,close}:{id:string;close:()=>void}){
 const [data,setData]=useState<States|null>(null),[error,setError]=useState('');
 useEffect(()=>{const abort=new AbortController();let timer:ReturnType<typeof setTimeout>;async function load(){try{const r=await fetch(`/api/message-connections/${id}/states`,{credentials:'omit',cache:'no-store',signal:abort.signal});if(!r.ok)throw Error();setData(await r.json());setError('');}catch{if(!abort.signal.aborted)setError(t('读取状态失败'));}if(!abort.signal.aborted)timer=setTimeout(load,5000);}void load();return()=>{abort.abort();clearTimeout(timer);};},[id]);
 return <Dialog.Root open onOpenChange={open=>{if(!open)close();}}><Dialog.Portal><Dialog.Overlay className="ui-overlay"/><Dialog.Content className="ui-dialog" aria-describedby={undefined}><div className="ui-dialog-header"><Dialog.Title>{t('资源状态')}</Dialog.Title><Button onClick={close}>{t('关闭')}</Button></div><div className="ui-dialog-body">{error&&<p className="error" role="alert">{error}</p>}{data&&Object.entries(data).map(([id,r])=><section key={id}><h3>{r.name}</h3>{Object.entries(r.groups).map(([id,g])=><section key={id}><h4>{g.name}</h4><table className="resource-state-table"><thead><tr><th>{t('字段')}</th><th>{t('值')}</th><th>{t('状态')}</th><th>{t('接收时间')}</th></tr></thead><tbody>{Object.entries(g.fields).map(([name,f])=><tr key={name}><td>{name}</td><td>{f.value==null?'—':typeof f.value==='string'?f.value:JSON.stringify(f.value)}</td><td>{t(f.freshness==='fresh'?'有效':f.freshness==='stale'?'已过期':'未知')}{f.origin==='retained'?' · Retained':''}</td><td>{f.received_at_ms?new Date(f.received_at_ms).toLocaleString():'—'}</td></tr>)}</tbody></table></section>)}</section>)}{data&&!Object.keys(data).length&&<p>{t('尚未配置资源')}</p>}</div></Dialog.Content></Dialog.Portal></Dialog.Root>
}
