import { catalog } from './catalog.ts';

export const languages = [
  { code: 'zh-CN', label: '简体中文' }, { code: 'en', label: 'English' },
  { code: 'ja', label: '日本語' }, { code: 'es', label: 'Español' },
] as const;
export type Locale = typeof languages[number]['code'];
const offsets = { en: 0, ja: 1, es: 2 } as const;
export function resolveLocale(value: string): Locale {
  const base = value.toLowerCase().split('-')[0];
  return base === 'zh' ? 'zh-CN' : base === 'ja' ? 'ja' : base === 'es' ? 'es' : 'en';
}
let locale: Locale = 'zh-CN';
if (typeof window !== 'undefined') {
  try { locale = resolveLocale(localStorage.getItem('synquay-language') ?? navigator.language); }
  catch { locale = resolveLocale(navigator.language); }
}
const listeners = new Set<() => void>();
export const getLocale = () => locale;
export function subscribeLocale(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; }
export function setLocale(next: Locale) {
  if (!languages.some(item => item.code === next)) return;
  locale = next;
  try { localStorage.setItem('synquay-language', next); } catch { /* Language switching also works without persistent storage. */ }
  syncDocumentLocale();
  listeners.forEach(listener => listener());
}
export function syncDocumentLocale() {
  if (typeof document === 'undefined') return;
  document.documentElement.lang = locale;
  document.title = t('Synquay 管理台');
}
function translated(key: string, target: Locale) { return target === 'zh-CN' ? key : catalog[key]?.[offsets[target]] ?? key; }
const escapePattern = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const patterns = Object.keys(catalog).filter(key => key.includes('{0}')).flatMap(key => [key].map(text => ({ key, pattern: new RegExp(`^${text.split('{0}').map(escapePattern).join('(.*?)')}$`) })));
export function translate(text: string, target: Locale, values?: readonly (string | number)[]): string {
  let key = text;
  let params = values;
  if (!Object.hasOwn(catalog, key)) {
    for (const item of patterns) { const match = item.pattern.exec(text); if (match) { key = item.key; params = match.slice(1); break; } }
  }
  const template = translated(key, target);
  return params ? template.replace(/\{(\d+)\}/g, (match, index: string) => String(params?.[Number(index)] ?? match)) : template;
}
export function t(text: string, values?: readonly (string | number)[]) { return translate(text, locale, values); }
