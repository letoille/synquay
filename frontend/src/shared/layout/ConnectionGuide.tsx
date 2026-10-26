import { t } from '../i18n';
import { Icon } from '../ui/Icon';

export function ConnectionGuide({ tools, published, needsPublication, disabled, onOpenMcp }: {
  tools: number; published: number; needsPublication: boolean; disabled: boolean;
  onOpenMcp: (tab: 'capabilities' | 'connection') => void;
}) {
  const ready = published > 0 && !needsPublication;
  return <section className="connection-guide" aria-label={t('使用指引')}>
    <div className="guide-heading"><div><h2>{tools ? t('继续配置你的连接') : t('从一个连接开始')}</h2><p>{t('连接决定接入什么，路由决定消息去哪里。MCP 是面向 Agent 的连接。')}</p></div><span className="subtle-badge">{t('使用指引')}</span></div>
    <ol className="guide-steps">
      <li><span className={`guide-number ${tools ? 'is-complete' : ''}`}>{tools ? <Icon name="check" size={15} /> : '1'}</span><div><strong>{t('添加软件或设备')}</strong><p>{t('配置接口地址，并说明它能执行什么操作。')}</p></div></li>
      <li><span className={`guide-number ${ready ? 'is-complete' : ''}`}>{ready ? <Icon name="check" size={15} /> : '2'}</span><div><strong>{t('配置 MCP 连接')}</strong><p>{t('选择允许 Agent 使用的能力，预览并发布。')}</p></div></li>
      <li><span className="guide-number">3</span><div><strong>{t('在 Agent 中接入')}</strong><p>{t('复制 MCP 地址，在客户端授权并验证调用。')}</p></div></li>
    </ol>
    <div className="guide-next"><p>{tools ? ready ? t('能力已发布，下一步在 Agent 中配置 MCP 地址。') : t('已有可配置的能力，下一步选择开放范围。') : t('点击下方“添加连接”，接入第一个 HTTP 服务。')}</p>{tools > 0 && <button className="text-button" disabled={disabled} onClick={() => onOpenMcp(ready ? 'connection' : 'capabilities')}>{ready ? t('查看接入方式') : t('选择并发布能力')}<Icon name="arrow" size={15} /></button>}</div>
    <details className="message-paths"><summary>{t('只转发消息，或交给 Agent 处理？')}</summary><div className="message-path-grid"><div><strong>{t('转发')}</strong><p>{t('入口连接 → 路由 → 出口连接')}</p><small>{t('例如：Webhook 接收告警，通过邮件连接发送。无需 MCP。')}</small></div><div><strong>{t('Agent 处理')}</strong><p>{t('入口连接 → 任务 → MCP 连接 ↔ Agent → 结果投递')}</p><small>{t('Agent 通过 MCP 领取并提交结果，再按规则回复原会话或指定渠道。')}</small></div></div><p className="small muted">{t('消息路由的在线配置尚未开放。MCP 接入不代表 Agent 会自动领取任务。')}</p></details>
  </section>;
}
