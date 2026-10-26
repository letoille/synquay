import type { ControlView, PublishInput } from './types';
export const sameNames = (a: string[], b: string[]) => a.length === b.length && a.every(n => b.includes(n));
export const publishedNames = (v: ControlView) => v.tools.filter(t => t.exposed).map(t => t.public_name);
export function draftNames(v: ControlView): string[] {
  // A completed publication leaves its draft as an immutable generation anchor.
  return v.draft?.base_revision === v.revision ? v.draft.exposed_tools : publishedNames(v);
}
export function changes(before: string[], after: string[]) {
  return { added: after.filter(n => !before.includes(n)), removed: before.filter(n => !after.includes(n)) };
}
export function pendingKey(v: ControlView) { return `synquay.publication.${v.profile_id}.${v.admin_id}`; }
export function readPending(v: ControlView): PublishInput | null {
  try {
    const value: unknown = JSON.parse(sessionStorage.getItem(pendingKey(v)) ?? 'null');
    if (!value || typeof value !== 'object') return null;
    const p = value as Partial<PublishInput>;
    if (typeof p.operation_id !== 'string' || !/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(p.operation_id)
      || !Number.isSafeInteger(p.expected_revision) || (p.expected_revision ?? 0) < 1
      || !Number.isSafeInteger(p.draft_generation) || (p.draft_generation ?? 0) < 1) return null;
    return { operation_id: p.operation_id, expected_revision: p.expected_revision!, draft_generation: p.draft_generation! };
  } catch { return null; }
}
export function storePending(v: ControlView, p: PublishInput | null): boolean {
  try { if (p) sessionStorage.setItem(pendingKey(v), JSON.stringify(p)); else sessionStorage.removeItem(pendingKey(v)); return true; } catch { return false; }
}
