import { t } from '../../shared/i18n';
import { useRef, useState } from 'react';
import type { ControlApi } from './api';
import { ControlError, errorMessage } from './api';
import type { ControlView, Verification } from './types';
import { jsonObject, operationKey, readOperation, saveOperation } from './connectionModel';

export function VerifyConnection({ api, view }: { api: ControlApi; view: ControlView }) {
  const key = operationKey(view, 'verification');
  const [id, setId] = useState(() => readOperation(key));
  const [token, setToken] = useState(''), [name, setName] = useState(''), [args, setArgs] = useState('{}'), [confirmed, setConfirmed] = useState(false);
  const [resolved, setResolved] = useState(false);
  const [busy, setBusy] = useState(false), [error, setError] = useState(''), [result, setResult] = useState<Verification | null>(null);
  const gate = useRef(false);
  const tools = view.tools.filter(t => t.exposed);
  async function run(inspect: boolean) {
    if (gate.current) return; gate.current = true; setBusy(true); setError('');
    try {
      const argumentsValue = inspect ? {} : jsonObject(args);
      const invocation = inspect ? id! : crypto.randomUUID();
      if (!inspect && !saveOperation(key, invocation)) throw Error("无法保存调用编号，尚未执行。");
      setId(invocation);
      const next = await api.verify({ mcp_token: token, invocation_id: invocation, action: inspect ? { kind: 'inspect' } : { kind: 'execute', revision: view.revision, public_name: name, arguments: argumentsValue, confirmed } });
      setResult(next); setConfirmed(false);
      if (next.status === 'succeeded' || next.status === 'failed') { saveOperation(key, null); setId(null); }
    } catch (e) {
      // These outer service rejections occur before dispatch. Keep ambiguous transport/server failures pending.
      if (!inspect && e instanceof ControlError && [400, 401, 403, 404, 409, 413, 415, 429].includes(e.status)) { saveOperation(key, null); setId(null); setConfirmed(false); }
      setError(e instanceof ControlError ? errorMessage(e) : e instanceof Error ? e.message : "调用结果待确认。"); }
    finally { gate.current = false; setBusy(false); }
  }
  const status = { succeeded: t("调用已完成，响应通过验证。"), failed: t("调用未成功，请用调用编号核查服务端记录。"), unknown: t("执行结果未知，请核查目标服务，勿重复执行。"), unconfirmed: t("尚未发现记录；无法据此判断是否执行。请保留编号并核查服务端。") };
  return <details className="verification"><summary>{t("验证一次服务内调用")}</summary><div className="setup-form"><p className="muted small">{t("使用 MCP 调用令牌，经过相同的成员权限、能力授权和执行链路。此验证不会证明外部 AI 客户端已经连通。")}</p>
    <form onSubmit={e => { e.preventDefault(); void run(!!id); }}>
      <label>{t("MCP 调用令牌")}<input type="password" autoComplete="off" value={token} required maxLength={4096} onChange={e => setToken(e.target.value)} /></label>
      {!id && <><label>{t("已发布能力")}<select value={name} required onChange={e => setName(e.target.value)}><option value="">{t("选择能力")}</option>{tools.map(t => <option key={t.public_name}>{t.public_name}</option>)}</select></label><label>{t("调用参数")}<textarea className="mono" rows={4} maxLength={16000} value={args} onChange={e => setArgs(e.target.value)} /></label><label className="check-row"><input type="checkbox" checked={confirmed} onChange={e => setConfirmed(e.target.checked)} />{t("我确认此次验证会实际调用目标服务，已检查参数与操作影响")}</label></>}
      {id && <div className="notice"><p>{t("仅查询原调用状态，不会重新执行。重新打开此页后，需再次输入同一调用者的令牌。")}</p><p className="mono">{id}</p><label className="check-row"><input type="checkbox" checked={resolved} onChange={e => setResolved(e.target.checked)} />{t("我已在目标服务核查结果，结束本地跟踪")}</label><button type="button" disabled={!resolved || busy} onClick={() => { saveOperation(key, null); setId(null); setResult(null); setResolved(false); setConfirmed(false); }}>{t("结束核查")}</button><p className="small muted">{t("这不会取消已经接纳的调用。")}</p></div>}
      {error && <p className="error" role="alert">{t(error)}</p>}{result && <p role="status" className="notice">{status[result.status]}<br /><span className="mono">{result.invocation_id}</span></p>}
      <button className="primary" disabled={busy || !token || (!id && (!confirmed || !name || !tools.some(t => t.public_name === name)))}>{busy ? t("正在处理…") : id ? t("查询调用结果") : t("执行一次验证")}</button>
    </form>
  </div></details>;
}
