import { readPending as readPendingMessage } from '../message-connections/api';
import { MessageConnections } from '../message-connections/MessageConnections';
import { RouteManager, readPendingRoute } from '../routes/RouteManager';
import { t } from '../../shared/i18n';
import { ConsoleShell } from '../../shared/layout/ConsoleShell';
import type { ConsolePage } from '../../shared/layout/ConsoleShell';
import { useMemo, useState } from 'react';
import { controlApi, ControlError } from './api';
import type { ControlView } from './types';
import { useControl } from './useControl';
import { ServerManager } from './ServerManager';
import { ConnectionSetup } from './ConnectionSetup';
import { operationKey, readOperation } from './connectionModel';

export function McpServices({ initial, initialPage = 'mcp', configurationChanged }: { initial: ControlView; initialPage?: ConsolePage; configurationChanged: () => void }) {
  const api = useMemo(() => controlApi(), []);
  const c = useControl(api, initial, configurationChanged);
  const [page, setPage] = useState<ConsolePage>(() => readPendingMessage(`synquay:message-connections:${initial.workspace?.id??initial.profile_id}:${initial.admin_id}`) ? 'connections' : readPendingRoute(`synquay:routes:${initial.workspace?.id??initial.profile_id}:${initial.admin_id}`) ? 'routes' : readOperation(operationKey(initial, 'connection')) ? 'connections' : initialPage);
  const [messageBlocked,setMessageBlocked]=useState(false);
  const [capabilityPage,setCapabilityPage]=useState(false);
  const [connectionBlocked, setConnectionBlocked] = useState(false);
  const [routeBlocked, setRouteBlocked] = useState(false);
  const [serverBlocked, setServerBlocked] = useState(false);
  const locked = !!c.busy || !!c.pending;
  return <ConsoleShell page={page} onNavigate={setPage} connected workspace={c.view.workspace?.name ?? t("当前空间")} suspended={c.view.workspace?.state === 'suspended'} navigationBlocked={messageBlocked || connectionBlocked || serverBlocked || routeBlocked}>
      <div className="page-heading" hidden={page === 'connections' && capabilityPage}><h1>{t(page === 'mcp' ? 'MCP 服务' : page === 'routes' ? '路由' : '连接')}</h1></div>
      {c.errorText && <div className="error" role="alert"><p>{t(c.errorText)}</p>{c.problem instanceof ControlError && c.problem.requestId && <p className="small mono">{t("请求编号：")}{c.problem.requestId}</p>}</div>}
      {page === 'connections' && !capabilityPage && c.message && <p className="feedback" role="status">{t(c.message)}</p>}
      {c.receipt && <details className="receipt"><summary>{t("已确认发布 v")}{c.receipt.revision}</summary><p className="small mono">{t("操作编号：")}{c.receipt.operation_id}</p></details>}
      {c.stale && !c.pending && <section className="notice"><p>{t("当前视图需要更新。读取最新版本会保留你的选择，请核对后重新保存。")}</p><button disabled={!!c.busy} onClick={() => c.refresh(true)}>{t("读取最新版本，保留选择")}</button></section>}
      {c.pending && <section className="notice" aria-label={t("发布结果待确认")}><h2>{t("发布结果待确认")}</h2><p>{t("操作可能仍在处理。核查完成前，暂停编辑与新发布。")}</p><p className="small mono">{t("操作编号：")}{c.pending.operation_id}</p><div className="actions"><button className="primary" disabled={!!c.busy} onClick={c.reconcile}>{t("查询发布结果")}</button><button disabled={!!c.busy} onClick={() => c.publish(true)}>{t("使用原编号重试")}</button></div></section>}
      {c.busy && <p className="small muted" role="status">{t(c.busy)}…</p>}
      <div hidden={page !== 'connections'}><MessageConnections storageKey={`synquay:message-connections:${initial.workspace?.id??initial.profile_id}:${initial.admin_id}`} locked={locked||connectionBlocked} onBlock={setMessageBlocked} onCapabilityPage={setCapabilityPage} completed={()=>c.refresh(false)}/><div hidden={capabilityPage}><ConnectionSetup api={api} view={c.view} locked={locked || messageBlocked || c.dirty || c.stale} completed={() => c.refresh(false)} onBlock={setConnectionBlocked} />{c.dirty && <p className="notice">{t("MCP 服务中有未保存的选择，请先保存后再添加连接。")}</p>}</div></div>
      {page === 'routes' && <RouteManager storageKey={`synquay:routes:${initial.workspace?.id??initial.profile_id}:${initial.admin_id}`} onBlock={setRouteBlocked} />}
      {page === 'mcp' && <ServerManager api={api} view={c.view} locked={locked || c.stale} changed={() => { void c.refresh(false); }} onBlock={setServerBlocked} />}
  </ConsoleShell>;
}
