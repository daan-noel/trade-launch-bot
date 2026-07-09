import type { ReactNode } from 'react';
import { cn } from 'lib/cn';

export type StatTone = 'default' | 'green' | 'red' | 'primary' | 'muted';

const toneClass: Record<StatTone, string> = {
  default: 'text-text',
  green: 'text-green',
  red: 'text-red',
  primary: 'text-primary',
  muted: 'text-text-dim',
};

/**
 * A single glanceable KPI tile — label over a large mono value, with an optional
 * sub-line. Reused by the Home command center and (later) the Live-Trading page.
 */
export function StatTile({
  label,
  value,
  sub,
  tone = 'default',
}: {
  label: string;
  value: ReactNode;
  sub?: ReactNode;
  tone?: StatTone;
}) {
  return (
    <div className="grid min-h-[58px] content-center gap-0.5 rounded-lg border border-white/5 bg-white/2 px-3 py-2 transition hover:border-white/10">
      <span className="truncate text-[10px] font-semibold uppercase tracking-wider text-text-dim">
        {label}
      </span>
      <span className={cn('font-mono text-lg font-semibold leading-tight', toneClass[tone])}>
        {value}
      </span>
      {sub != null && <span className="truncate text-[11px] text-text-dim">{sub}</span>}
    </div>
  );
}
