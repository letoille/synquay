import type { ControlView } from './types';
export function operationKey(view: ControlView, kind: 'connection' | 'verification') { return `synquay:${kind}:${view.profile_id}:${view.admin_id}`; }
export function readOperation(key: string): string | null {
  try { const id = sessionStorage.getItem(key); return id && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(id) ? id : null; } catch { return null; }
}
export function saveOperation(key: string, id: string | null) {
  try { if (id) sessionStorage.setItem(key, id); else sessionStorage.removeItem(key); return true; } catch { return false; }
}
export function jsonObject(value: string): Record<string, unknown> {
  const v: unknown = JSON.parse(value);
  if (v === null || typeof v !== 'object' || Array.isArray(v)) throw Error('请填写 JSON 对象。');
  return v as Record<string, unknown>;
}
