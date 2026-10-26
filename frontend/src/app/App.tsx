import { t, getLocale, subscribeLocale, syncDocumentLocale } from '../shared/i18n';
import { useCallback, useState, useSyncExternalStore, useEffect } from 'react';
import { McpServices } from '../features/mcp-services/McpServices';
import { controlApi, errorMessage } from '../features/mcp-services/api';
import type { ControlView } from '../features/mcp-services/types';
import { Icon } from '../shared/ui/Icon';
import { ConsoleShell, type ConsolePage } from '../shared/layout/ConsoleShell';

export default function App() {
  const locale = useSyncExternalStore(subscribeLocale, getLocale, getLocale);
  useEffect(syncDocumentLocale, [locale]);
  const [view, setView] = useState<ControlView | null>(null);
  const [page, setPage] = useState<ConsolePage>('mcp');
  const [attempt, setAttempt] = useState(0);
  const [loading, setLoading] = useState(true);
  const [problem, setProblem] = useState<unknown>(null);
  const configurationChanged = useCallback(() => { setView(null); setAttempt(value => value + 1); }, []);
  useEffect(() => {
    const controller = new AbortController();
    setLoading(true); setProblem(null);
    void controlApi().view(controller.signal).then(result => {
      if (!controller.signal.aborted) setView(result);
    }).catch(error => {
      if (!controller.signal.aborted) setProblem(error);
    }).finally(() => {
      if (!controller.signal.aborted) setLoading(false);
    });
    return () => controller.abort();
  }, [attempt]);
  if (view) return <McpServices initial={view} initialPage={page} configurationChanged={configurationChanged} />;
  return <ConsoleShell page={page} onNavigate={setPage} connected={false}>
    <div className="page-heading"><div><p className="overline">{t('工作空间')}</p>
      <h1>{page === 'connections' ? t('连接') : page === 'routes' ? t('路由') : t('MCP 服务')}</h1>
    </div></div>
    <section className="notice" role="status" aria-live="polite">
      <p>{loading ? t('正在读取工作空间…') : t(errorMessage(problem))}</p>
      {!loading && <button onClick={() => setAttempt(value => value + 1)}>{t('重新检查')}</button>}
    </section>
    <section className="empty" aria-label={t('配置暂不可用')}>
      <span className="empty-icon"><Icon name={page === 'connections' ? 'connections' : 'agent'} size={28} /></span>
      <h2>{t('配置暂不可用')}</h2><p>{t('读取工作空间后即可管理连接与 Agent 能力。')}</p>
    </section>
  </ConsoleShell>;
}
