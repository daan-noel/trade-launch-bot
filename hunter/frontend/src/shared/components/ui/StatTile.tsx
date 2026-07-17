import type { ReactNode } from 'react';
import { cn } from 'lib/cn';

export type StatTone = 'default' | 'green' | 'red' | 'primary' | 'muted' | 'info';

const toneClass: Record<StatTone, string> = {
  default: 'text-text',
  green: 'text-green',
  red: 'text-red',
  primary: 'text-primary',
  muted: 'text-text-dim',
  info: 'text-accent',
};

export type StatSize = 'sm' | 'md';

const sizeClass: Record<StatSize, { box: string; label: string; value: string }> = {
  sm: {
    box: 'min-h-10 gap-0.5 rounded-md px-1.5 py-1',
    label: 'text-[10px]',
    value: 'text-sm',
  },
  md: {
    box: 'min-h-[58px] gap-0.5 rounded-lg px-3 py-2',
    label: 'text-[10px]',
    value: 'text-lg font-semibold',
  },
};

/**
 * Single glanceable KPI atom — label over a mono value, optional sub-line.
 * Use `size="sm"` in dense detail grids; `md` for command-center strips.
 */
export function StatTile({
  label,
  value,
  sub,
  tone = 'default',
  size = 'md',
  href,
  bold,
}: {
  label: string;
  value: ReactNode;
  sub?: ReactNode;
  tone?: StatTone;
  size?: StatSize;
  href?: string;
  bold?: boolean;
}) {
  const s = sizeClass[size];
  const valueCls = cn('font-mono leading-tight', s.value, toneClass[tone], bold && 'font-semibold');

  return (
    <div
      className={cn(
        'grid content-center border border-white/5 bg-white/2 transition hover:border-white/10',
        s.box,
      )}
    >
      <span className={cn('truncate font-semibold uppercase tracking-wider text-text-dim', s.label)}>
        {label}
      </span>
      {href ? (
        <a
          href={href}
          target="_blank"
          rel="noopener noreferrer"
          className={cn(valueCls, 'text-accent hover:text-primary hover:underline')}
        >
          {value}
        </a>
      ) : (
        <span className={valueCls}>{value}</span>
      )}
      {sub != null && <span className="truncate text-[11px] text-text-dim">{sub}</span>}
    </div>
  );
}
