// A clause or exit label as a search job prints it
// (`m_flow.buy_sol @!volume [2s] >= 0.9`, `... >= 2 [peak]`), explained on hover by
// its metric's registry definition. A label opens with the metric path, which is all
// the lookup needs.

import { cn } from 'lib/cn';
import { findMetric, metricHelp, useStrategyRegistry } from 'lib/strategy/registry';

export function Clause({ text, className }: { text: string; className?: string }) {
  const { data: reg } = useStrategyRegistry();
  const spec = findMetric(reg, text.split(' ')[0]);
  return (
    <span className={cn('font-mono text-text-mid', className)} title={spec ? metricHelp(spec) : undefined}>
      {text}
    </span>
  );
}
