import { useMemo } from 'react';

import { ModeToggle } from './ModeToggle';
import { modeCounts, type ModeFilter } from 'lib/strategy/mode';
import type { StrategyRule } from 'lib/strategy/types';

export interface RuleModeFilterProps {
  /** The rules the counts describe — pass the set filtered by everything EXCEPT
   *  the mode filter, so a chip's count doesn't collapse the moment you click it
   *  (same contract as `RuleTagFilter`). */
  rules: Pick<StrategyRule, 'trade_mode'>[];
  value: ModeFilter;
  onChange: (next: ModeFilter) => void;
  className?: string;
}

/**
 * Paper / Real scope for a rule board — `ModeToggle` plus rule counts.
 * Each chip carries the same hue as that mode's row rail and Mode badge
 * (`info` = paper, `warning` = real).
 */
export function RuleModeFilter({ rules, value, onChange, className }: RuleModeFilterProps) {
  const counts = useMemo(() => modeCounts(rules), [rules]);

  return (
    <ModeToggle
      layout="filter"
      counts={counts}
      value={value}
      onChange={onChange}
      className={className}
    />
  );
}
