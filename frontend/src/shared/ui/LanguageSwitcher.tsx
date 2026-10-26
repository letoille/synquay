import { useSyncExternalStore } from 'react';
import { DropdownMenu } from 'radix-ui';
import { getLocale, languages, setLocale, subscribeLocale, t, type Locale } from '../i18n';
import { Icon } from './Icon';

export function LanguageSwitcher() {
  const locale = useSyncExternalStore(subscribeLocale, getLocale, getLocale);
  const label = `${t('语言')} · ${languages.find(item => item.code === locale)?.label}`;
  return <DropdownMenu.Root>
    <DropdownMenu.Trigger asChild><button type="button" className="icon-button language-trigger" aria-label={label} title={label}><Icon name="language" size={19} /></button></DropdownMenu.Trigger>
    <DropdownMenu.Portal><DropdownMenu.Content className="language-popover" align="end" sideOffset={8} collisionPadding={12} aria-label={t('语言')}>
      <DropdownMenu.RadioGroup value={locale} onValueChange={value => setLocale(value as Locale)}>
        {languages.map(item => <DropdownMenu.RadioItem key={item.code} value={item.code} className="language-option" lang={item.code}><span>{item.label}</span><DropdownMenu.ItemIndicator><Icon name="check" size={16} /></DropdownMenu.ItemIndicator></DropdownMenu.RadioItem>)}
      </DropdownMenu.RadioGroup>
    </DropdownMenu.Content></DropdownMenu.Portal>
  </DropdownMenu.Root>;
}
