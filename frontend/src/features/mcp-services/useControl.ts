import { useEffect, useRef, useState } from 'react';
import { ControlError, errorMessage } from './api';
import type { ControlApi } from './api';
import type { ControlView, PublishInput, Receipt } from './types';
import { draftNames, publishedNames, readPending, sameNames, storePending } from './model';

export function useControl(api: ControlApi, initial: ControlView, configurationChanged: () => void) {
  const [view, setView] = useState(initial);
  const [selected, setSelected] = useState(() => draftNames(initial));
  const [pending, setPending] = useState(() => readPending(initial));
  const [busy, setBusy] = useState('');
  const [problem, setProblem] = useState<unknown>(null);
  const [message, setMessage] = useState('');
  const [stale, setStale] = useState(false);
  const [review, setReview] = useState(false);
  const [receipt, setReceipt] = useState<Receipt | null>(null);
  const gate = useRef(false), abort = useRef(new AbortController());
  const dirty = !sameNames(selected, draftNames(view));
  const hasChanges = !sameNames(draftNames(view), publishedNames(view));
  const canPublish = !busy && !pending && !stale && !dirty && hasChanges && view.draft?.base_revision === view.revision;
  useEffect(() => { const c = new AbortController(); abort.current = c; return () => c.abort(); }, []);
  useEffect(() => {
    if (!dirty && !pending && !busy) return;
    const listener = (e: BeforeUnloadEvent) => { e.preventDefault(); };
    window.addEventListener('beforeunload', listener);
    return () => window.removeEventListener('beforeunload', listener);
  }, [dirty, pending, busy]);

  function fail(error: unknown) {
    if (abort.current.signal.aborted) return;
    setProblem(error);
  }
  async function run(label: string, fn: () => Promise<void>) {
    if (gate.current) return;
    gate.current = true; setBusy(label); setProblem(null); setMessage('');
    try { await fn(); } catch (error) { fail(error); }
    finally { gate.current = false; if (!abort.current.signal.aborted) setBusy(''); }
  }
  async function load(keepSelection: boolean) {
    const next = await api.view(abort.current.signal);
    if (next.profile_id !== view.profile_id || next.admin_id !== view.admin_id) {
      configurationChanged(); throw new ControlError(409, 'identity_changed');
    }
    setView(next); setStale(false); setReview(false);
    if (!keepSelection) setSelected(draftNames(next));
    else setSelected(old => old.filter(n => next.tools.some(t => t.public_name === n)));
  }
  function refresh(keepSelection = dirty) {
    void run('正在读取最新版本', async () => {
      await load(keepSelection);
      setMessage(keepSelection ? '已读取最新版本并保留你的选择，请重新检查后保存草稿。' : '已读取最新版本。');
    });
  }
  function save() {
    if (!dirty || stale || pending) return;
    void run('正在保存草稿', async () => {
      try {
        const draft = await api.save({ expected_generation: view.draft?.generation ?? 0, base_revision: view.revision, exposed_tools: selected }, abort.current.signal);
        setView(v => ({ ...v, draft })); setSelected(draft.exposed_tools); setReview(false);
        setMessage('草稿已保存，当前运行版本尚未改变。');
      } catch (e) { setStale(true); throw e; }
    });
  }
  async function completed(result: Receipt, operation: PublishInput) {
    if (result.operation_id !== operation.operation_id || result.revision !== operation.expected_revision + 1) throw new ControlError(0, 'invalid_response');
    setReceipt(result); setPending(null); storePending(view, null); setReview(false); setStale(true);
    // A committed receipt stays visible even if the follow-up read fails.
    setMessage(`发布已确认：v${result.revision}。`);
    await load(false);
  }
  function publish(retry = false) {
    if (!retry && (!canPublish || !view.draft)) return;
    const input: PublishInput = retry && pending ? pending : {
      operation_id: crypto.randomUUID(), expected_revision: view.revision, draft_generation: view.draft!.generation,
    };
    void run(retry ? '正在确认原发布操作' : '正在发布，等待当前调用完成', async () => {
      // Save only non-secret recovery metadata. Refuse to start if persistence fails.
      if (!storePending(view, input)) throw new ControlError(0, 'recovery_storage_unavailable');
      setPending(input);
      try { await completed(await api.publish(input, abort.current.signal), input); }
      catch (e) {
        // CAS/validation rejection is definitive; transient errors keep the same ID.
        if (e instanceof ControlError && [400, 409].includes(e.status)) {
          setPending(null); storePending(view, null); setStale(true); setReview(false);
        }
        throw e;
      }
    });
  }
  function reconcile() {
    if (!pending) return;
    void run('正在查询发布结果', async () => {
      const result = await api.result(pending, abort.current.signal);
      if (result.status === 'committed' && result.receipt) await completed(result.receipt, pending);
      else setMessage('尚未找到已提交的回执。操作可能仍在处理；稍后查询，或使用原操作编号重试。');
    });
  }
  return { view, selected, setSelected, pending, busy, problem, message, stale, review, setReview, receipt,
    dirty, hasChanges, canPublish, refresh, save, publish, reconcile, errorText: problem ? errorMessage(problem) : '' };
}
