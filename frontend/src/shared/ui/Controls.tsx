import { Checkbox as PrimitiveCheckbox, Switch as PrimitiveSwitch } from 'radix-ui';
import { useId, type ComponentProps, type ReactNode } from 'react';
import { Icon } from './Icon';

export function Button({ variant = 'secondary', className = '', type = 'button', ...props }: ComponentProps<'button'> & { variant?: 'primary' | 'secondary' | 'danger' | 'ghost' }) {
  return <button type={type} className={`ui-button ui-button--${variant} ${className}`} {...props} />;
}
export function Input({ className = '', type = 'text', ...props }: ComponentProps<'input'>) {
  return <input type={type} className={`ui-input ${className}`} {...props} />;
}
export function Checkbox({ children, description, className = '', ...props }: Omit<ComponentProps<typeof PrimitiveCheckbox.Root>, 'children'> & { children: ReactNode; description?: ReactNode }) {
  const generated = useId();
  const id = props.id ?? generated;
  return <div className={`ui-check-field ${className}`}>
    <PrimitiveCheckbox.Root {...props} id={id} className="ui-checkbox" aria-labelledby={`${id}-label`} aria-describedby={description ? `${id}-description` : undefined}>
      <PrimitiveCheckbox.Indicator><Icon name="check" size={14} /></PrimitiveCheckbox.Indicator>
    </PrimitiveCheckbox.Root>
    <label htmlFor={id}><span id={`${id}-label`}>{children}</span>{description && <small id={`${id}-description`}>{description}</small>}</label>
  </div>;
}
export function Switch(props: ComponentProps<typeof PrimitiveSwitch.Root>) {
  return <PrimitiveSwitch.Root {...props} className="ui-switch"><PrimitiveSwitch.Thumb className="ui-switch-thumb" /></PrimitiveSwitch.Root>;
}
