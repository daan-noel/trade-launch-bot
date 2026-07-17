import type { ReactNode } from 'react';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { cn } from 'lib/cn';
import type { HelpTip } from 'lib/strategy/strategyHelp';

/** Label row with an ⓘ tip — keeps rule/fingerprint forms consistent. */
export function LabelTip({
  tip,
  children,
  className,
}: {
  tip: HelpTip;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <span className={cn('inline-flex items-center gap-1', className)}>
      {children}
      <InfoTooltip title={tip.title} body={tip.body} />
    </span>
  );
}
