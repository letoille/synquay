import { Tabs } from 'radix-ui';
import { Checkbox } from '../../shared/ui/Controls';
import { t } from '../../shared/i18n';
import type { ContentScope, ControlView } from './types';
export function CapabilityEditor({view,tools,scope,disabled,changeTools,changeScope}:{view:ControlView;tools:string[];scope:ContentScope;disabled:boolean;changeTools:(v:string[])=>void;changeScope:(v:ContentScope)=>void}) {
  const toggle=(list:string[],id:string,checked:boolean)=>checked?[...list.filter(v=>v!==id),id]:list.filter(v=>v!==id);
  return <Tabs.Root defaultValue="tools" className="capability-tabs">
    <Tabs.List aria-label={t('开放能力')} className="capability-tabs-list">
      <Tabs.Trigger value="tools">{t('工具')}<span>{tools.length}</span></Tabs.Trigger>
      <Tabs.Trigger value="resources">{t('资源')}<span>{scope.resources.length}</span></Tabs.Trigger>
      <Tabs.Trigger value="prompts">{t('提示词')}<span>{scope.prompts.length}</span></Tabs.Trigger>
    </Tabs.List>
    <Tabs.Content value="tools" className="capability-tab-panel">
      {view.tools.length?<div className="server-tool-options">{view.tools.map(tool=><Checkbox key={tool.public_name} checked={tools.includes(tool.public_name)} disabled={disabled} onCheckedChange={checked=>changeTools(toggle(tools,tool.public_name,checked===true))} description={tool.definition.description}><strong>{tool.public_name}</strong></Checkbox>)}</div>:<p className="empty">{t('暂无工具')}</p>}
    </Tabs.Content>
    <Tabs.Content value="resources" className="capability-tab-panel">
      {view.content?.resources.length?<div className="server-tool-options">{view.content.resources.map(r=><Checkbox key={r.id} checked={scope.resources.includes(r.id)} disabled={disabled} onCheckedChange={checked=>changeScope({...scope,resources:toggle(scope.resources,r.id,checked===true)})} description={r.description}><strong>{r.id}</strong>{r.kind==='template'&&<span className="content-kind">{t('模板')}</span>}</Checkbox>)}</div>:<p className="empty">{t(view.content?'暂无资源':'内容目录暂不可用')}</p>}
    </Tabs.Content>
    <Tabs.Content value="prompts" className="capability-tab-panel">
      {view.content?.prompts.length?<div className="server-tool-options">{view.content.prompts.map(p=><Checkbox key={p.id} checked={scope.prompts.includes(p.id)} disabled={disabled} onCheckedChange={checked=>changeScope({...scope,prompts:toggle(scope.prompts,p.id,checked===true)})} description={p.description}><strong>{p.id}</strong></Checkbox>)}</div>:<p className="empty">{t(view.content?'暂无提示词':'内容目录暂不可用')}</p>}
    </Tabs.Content>
  </Tabs.Root>;
}
