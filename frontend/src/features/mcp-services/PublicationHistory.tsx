import { t, getLocale } from '../../shared/i18n';
import { useEffect, useRef, useState } from 'react';
import type { ControlApi } from './api';
import { errorMessage } from './api';
import type { HistoryEntry } from './types';

export function PublicationHistory({ api }: { api: ControlApi }) {
  const [items, setItems] = useState<HistoryEntry[]>([]), [cursor, setCursor] = useState<number | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState<unknown>(null);
  const request = useRef<AbortController | null>(null);
  async function load(before?: number) {
    if (request.current) return;
    const c = new AbortController(); request.current = c; setBusy(true); setError(null);
    try { const result = await api.history(before, c.signal); if (!c.signal.aborted) { setItems(old => before ? [...old, ...result.items] : result.items); setCursor(result.next_before_revision); } }
    catch (e) { if (!c.signal.aborted) { setError(e); } }
    finally { if (request.current === c) request.current = null; if (!c.signal.aborted) setBusy(false); }
  }
  useEffect(() => { void load(); return () => { request.current?.abort(); request.current = null; }; }, [api]); // Stable session client.
  return <section aria-label={t("发布历史")}>
    <div className="section-bar"><p className="muted small">{t("版本记录包含文件发布和管理台发布。")}</p><button disabled={busy} onClick={() => void load()}>{t("刷新记录")}</button></div>
    {error !== null && <p className="error" role="alert">{t(errorMessage(error))}</p>}
    <table className="data-table history-table"><thead><tr><th>{t("版本")}</th><th>{t("发布时间")}</th><th>{t("开放能力")}</th><th>{t("详情")}</th></tr></thead><tbody>
      {items.map(item => <tr key={item.revision}><td><strong>v{item.revision}</strong></td><td className="muted"><time dateTime={new Date(item.created_ms).toISOString()}>{new Date(item.created_ms).toLocaleString(getLocale(), { hour12: false })}</time></td><td>{item.exposed_tools.length} {t("项")}</td><td><details><summary>{t("查看")}</summary><div className="history-detail"><p>{item.exposed_tools.join('、') || t("未开放任何能力")}</p><p className="small muted">{t("操作者")}</p><p className="mono small">{item.actor_id}</p></div></details></td></tr>)}
    </tbody></table>
    {!busy && !error && !items.length && <p className="empty">{t("尚无发布记录。")}</p>}
    <div className="section-bar"><span className="small muted" role="status">{busy ? t("正在读取记录…") : t('已显示 {0} 条', [items.length])}</span>{cursor !== null && <button disabled={busy} onClick={() => void load(cursor)}>{t("加载更早记录")}</button>}</div>
  </section>;
}
