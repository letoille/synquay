import { t } from '../i18n';
import { useEffect, useState, type ReactNode } from 'react';
import { BrandMark, Icon } from '../ui/Icon';
import { LanguageSwitcher } from '../ui/LanguageSwitcher';

export type ConsolePage = 'connections' | 'mcp' | 'routes';
const pages = [['mcp', 'MCP 服务'], ['connections', '连接'], ['routes', '路由']] as const;

export function ConsoleShell({ page, onNavigate, connected, workspace, suspended = false, navigationBlocked = false, children }: {
  page: ConsolePage; onNavigate: (page: ConsolePage) => void; connected: boolean; workspace?: string;
  suspended?: boolean; navigationBlocked?: boolean; children: ReactNode;
}) {
  const [theme, setTheme] = useState<'light' | 'dark'>(() => {
    try { return localStorage.getItem('synquay-theme') === 'dark' ? 'dark' : 'light'; } catch { return 'light'; }
  });
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try { localStorage.setItem('synquay-theme', theme); } catch { /* Appearance remains usable without storage. */ }
  }, [theme]);
  const status = connected ? suspended ? t('空间已停用') : t('配置已加载') : t('配置未就绪');
  return <div className="console-shell">
    <a className="skip-link" href="#main-content">{t('跳转到内容')}</a>
    <header className="console-header"><div className="header-inner">
      <div className="brand"><BrandMark /><span>synquay</span></div>
      <nav className="primary-nav" aria-label={t('主导航')}>{pages.map(([id, label]) => <button key={id}
        aria-current={page === id ? 'page' : undefined}
        disabled={navigationBlocked && id !== page} onClick={() => onNavigate(id)}>{t(label)}</button>)}</nav>
      <div className="header-actions">
        <span className={`workspace-status ${connected && !suspended ? 'is-connected' : ''}`} title={`${workspace ?? t('工作空间')} · ${status}`}>
          <i aria-hidden="true" /><span className="workspace-name">{workspace ?? t('工作空间')}</span><span className="sr-only"> · {status}</span>
        </span>
        <LanguageSwitcher /><button className="icon-button" onClick={() => setTheme(theme === 'light' ? 'dark' : 'light')} aria-label={theme === 'light' ? t('切换深色模式') : t('切换浅色模式')} title={theme === 'light' ? t('切换深色模式') : t('切换浅色模式')}><Icon name={theme === 'light' ? 'moon' : 'sun'} size={18} /></button>
      </div>
    </div></header>
    <main className="console-main" id="main-content" tabIndex={-1}><div className="page-content">{children}</div></main>
    <footer className="page-footer"><span>{t('Synquay · AI 与连接')}</span><span>{t('连接 · 开放 · 协作')}</span></footer>
  </div>;
}
