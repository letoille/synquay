import { useEffect, useRef, useState } from 'react';
import { Icon } from './Icon';
import { IconButton } from './IconButton';
import { t } from '../i18n';
export function CopyButton({value,label}: {value:string;label?:string}) {
  const [state,setState]=useState<'idle'|'copied'|'failed'>('idle');
  const timer=useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const mounted=useRef(true);
  useEffect(()=>{mounted.current=true;return()=>{mounted.current=false;clearTimeout(timer.current);};},[]);
  return <span className="copy-control"><IconButton variant="ghost" className={`card-copy ${state==='copied'?'is-copied':''}`} label={state==='copied'?t('已复制。'):label??t('复制地址')} onClick={async()=>{
    try {await navigator.clipboard.writeText(value);if(!mounted.current)return;setState('copied');} catch {if(!mounted.current)return;setState('failed');}
    clearTimeout(timer.current);timer.current=setTimeout(()=>setState('idle'),1800);
  }}><Icon name={state==='copied'?'check':'copy'} size={15}/></IconButton>{state==='failed'&&<span className="copy-failure" role="alert">{t('无法自动复制，请选中地址手动复制。')}</span>}</span>;
}
