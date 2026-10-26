import { Dialog } from 'radix-ui';
import { Button } from '../../shared/ui/Controls';
import { Icon } from '../../shared/ui/Icon';
import { t } from '../../shared/i18n';
import type { ControlView, McpServer, CapabilityKind } from './types';
function Parameters({schema}: {schema:Record<string,unknown>}) {
  const properties=schema.properties && typeof schema.properties==='object' && !Array.isArray(schema.properties) ? Object.entries(schema.properties) : [];
  const required=Array.isArray(schema.required)?schema.required:[];
  return <div className="tool-parameters"><h3>{t('工具参数')}</h3>{properties.length ? <div className="parameter-list">{properties.map(([name,raw])=>{
    const spec=raw && typeof raw==='object' && !Array.isArray(raw)?raw as Record<string,unknown>:{};
    return <div key={name}><div className="parameter-name"><code>{name}</code><span>{Array.isArray(spec.type)?spec.type.filter(v=>typeof v==='string').join(' | '):typeof spec.type==='string'?spec.type:'—'}</span>{required.includes(name)&&<small>{t('必填')}</small>}</div>{typeof spec.description==='string'&&<p>{spec.description}</p>}</div>;
  })}</div>:<p className="small muted">{t('无固定参数')}</p>}<details className="parameter-schema"><summary>{t('完整参数定义')}</summary><pre>{JSON.stringify(schema,null,2)}</pre></details></div>;
}
export function ServerDetails({ server, view, kind, close, restoreFocus }: {
  server: McpServer; view: ControlView; kind:CapabilityKind; close: () => void; restoreFocus: (event: Event) => void;
}) {
  const selected=kind==='tools'?server.exposed_tools:server.content_scope?.[kind]??[];
  return <Dialog.Root open onOpenChange={open => { if (!open) close(); }}><Dialog.Portal>
    <Dialog.Overlay className="ui-overlay" /><Dialog.Content className="ui-dialog" aria-describedby={undefined} onCloseAutoFocus={restoreFocus}>
      <div className="ui-dialog-header"><Dialog.Title>{t(kind==='tools'?'开放工具':kind==='resources'?'开放资源':'开放提示词')}</Dialog.Title><Dialog.Close asChild><Button variant="ghost" className="ui-close" aria-label={t('关闭')}><Icon name="close" size={18} /></Button></Dialog.Close></div>
      <div className="ui-dialog-body server-details"><div className="details-tool-list">{selected.map(name => {
        if(kind==='tools') { const tool=view.tools.find(tool=>tool.public_name===name);return <div key={name}><h3>{name}</h3><p>{tool?.definition.description ?? t('暂无说明')}</p>{tool && <Parameters schema={tool.definition.input_schema} />}</div>; }
        if(kind==='resources') { const r=view.content?.resources.find(r=>r.id===name);return <div key={name}><h3>{name}{r?.kind==='template'&&<span className="content-kind">{t('模板')}</span>}</h3><p>{r?.description??t('暂无说明')}</p>{r&&<p className="small muted">{r.mime_type}</p>}{r?.parameter&&<div className="parameter-list"><div><div className="parameter-name"><code>{r.parameter}</code><small>{t('必填')}</small></div></div></div>}</div>; }
        const prompt=view.content?.prompts.find(p=>p.id===name);
        return <div key={name}><h3>{name}</h3><p>{prompt?.description??t('暂无说明')}</p>{prompt&&<div className="tool-parameters"><h3>{t('参数')}</h3>{prompt.arguments.length?<div className="parameter-list">{prompt.arguments.map(a=><div key={a.name}><div className="parameter-name"><code>{a.name}</code>{a.required&&<small>{t('必填')}</small>}</div><p>{a.description}</p>{a.choices.length>0&&<p className="small muted">{a.choices.join(' · ')}</p>}</div>)}</div>:<p className="small muted">{t('无固定参数')}</p>}</div>}</div>;
      })}</div>
        {!selected.length && <p className="empty">{t(kind==='tools'?'尚未开放工具':kind==='resources'?'尚未开放资源':'尚未开放提示词')}</p>}
      </div>
    </Dialog.Content></Dialog.Portal></Dialog.Root>;
}
