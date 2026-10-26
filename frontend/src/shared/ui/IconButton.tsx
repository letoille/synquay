import { Tooltip } from 'radix-ui';
import type { ComponentProps } from 'react';
import { Button } from './Controls';
export function IconButton({label,...props}: ComponentProps<typeof Button> & {label:string}) {
  return <Tooltip.Provider delayDuration={250}><Tooltip.Root><Tooltip.Trigger asChild><Button {...props} aria-label={label} className={`ui-icon-action ${props.className ?? ''}`} /></Tooltip.Trigger><Tooltip.Portal><Tooltip.Content className="ui-tooltip" sideOffset={6}>{label}<Tooltip.Arrow className="ui-tooltip-arrow" /></Tooltip.Content></Tooltip.Portal></Tooltip.Root></Tooltip.Provider>;
}
