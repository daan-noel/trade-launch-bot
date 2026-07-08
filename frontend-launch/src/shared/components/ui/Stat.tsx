import { ReactNode } from 'react';
import clsx from 'clsx';

/** Compact KPI tile for the dashboard header row. */
export function StatCard({
  label,
  value,
  sub,
  tone,
  className,
}: {
  label: ReactNode;
  value: ReactNode;
  sub?: ReactNode;
  tone?: 'good' | 'warn' | 'bad';
  className?: string;
}) {
  const toneClass =
    tone === 'good'
      ? 'text-[var(--color-good)]'
      : tone === 'warn'
        ? 'text-[var(--color-warn)]'
        : tone === 'bad'
          ? 'text-[var(--color-bad)]'
          : '';
  return (
    <div className={clsx('panel px-4 py-3', className)}>
      <div className="field-label mb-1">{label}</div>
      <div className={clsx('text-2xl font-semibold leading-none', toneClass)}>{value}</div>
      {sub && <div className="mt-1 text-xs muted">{sub}</div>}
    </div>
  );
}
