import { useMemo } from 'react';

import { cn } from 'lib/cn';
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
 * Paper / Real scope for a rule board — a segmented control that sits beside the
 * tag chips and narrows the table to one trade mode.
 *
 * Each chip carries the same hue as that mode's row rail and Mode badge
 * (`info` = paper, `warning` = real), plus a matching swatch, so the control
 * teaches the row paint rather than being a second unrelated code. Everything
 * downstream of the filtered set follows for free: on Rules the scoreboard
 * tiles stop blending paper PnL into real, and on Simulate the bulk-run buttons
 * target only the scoped cohort.
 */
const OPTIONS: {
  key: ModeFilter;
  label: string;
  /** Chip styling when selected. Literal strings — Tailwind scans source. */
  active: string;
  /** Rail-matching swatch; `all` gets both halves. */
  swatch: string | null;
  hint: (n: { paper: number; real: number }) => string;
}[] = [
  {
    key: 'all',
    label: 'All',
    active: 'bg-primary/20 text-primary',
    swatch: null,
    hint: (n) => `Show both modes (${n.paper} paper, ${n.real} real)`,
  },
  {
    key: 'paper',
    label: 'Paper',
    active: 'bg-info/20 text-info',
    swatch: 'bg-info/60',
    hint: (n) => `Show only the ${n.paper} paper rule(s)`,
  },
  {
    key: 'real',
    label: 'Real',
    active: 'bg-warning/20 text-warning',
    swatch: 'bg-warning',
    hint: (n) => `Show only the ${n.real} real-money rule(s)`,
  },
];

export function RuleModeFilter({ rules, value, onChange, className }: RuleModeFilterProps) {
  const counts = useMemo(() => modeCounts(rules), [rules]);

  return (
    <div
      role="group"
      aria-label="Trade mode"
      className={cn('inline-flex items-center gap-0.5 rounded-md bg-white/5 p-0.5', className)}
    >
      {OPTIONS.map((opt) => {
        const selected = value === opt.key;
        const count = opt.key === 'all' ? counts.paper + counts.real : counts[opt.key];
        return (
          <button
            key={opt.key}
            type="button"
            aria-pressed={selected}
            title={opt.hint(counts)}
            onClick={() => onChange(opt.key)}
            className={cn(
              'flex cursor-pointer items-center gap-1.5 rounded px-2 py-1 text-xs font-semibold',
              selected ? opt.active : 'text-text-dim hover:bg-white/8 hover:text-text',
            )}
          >
            {opt.swatch && <span className={cn('h-3 w-0.5 rounded-full', opt.swatch)} />}
            {opt.label}
            <span className="tabular-nums text-[10px] font-normal opacity-70">{count}</span>
          </button>
        );
      })}
    </div>
  );
}
