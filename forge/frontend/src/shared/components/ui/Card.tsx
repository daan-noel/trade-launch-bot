import { ReactNode } from 'react';
import clsx from 'clsx';
import { Tone } from './Badge';

export function Card({
  title,
  actions,
  children,
  className,
  bodyClassName,
}: {
  title?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <section className={clsx('panel', className)}>
      {(title || actions) && (
        <header className="flex items-center justify-between gap-3 px-4 py-2.5 border-b border-[var(--color-line)]">
          <h2 className="text-xs font-semibold uppercase tracking-wider muted">{title}</h2>
          {actions && <div className="flex items-center gap-2">{actions}</div>}
        </header>
      )}
      <div className={clsx('p-4', bodyClassName)}>{children}</div>
    </section>
  );
}

const BANNER_TONE: Record<Tone, string> = {
  good: 'border-[var(--color-good)] text-[var(--color-good)] bg-[var(--color-good-bg)]',
  warn: 'border-[var(--color-warn)] text-[var(--color-warn)] bg-[var(--color-warn-bg)]',
  bad: 'border-[var(--color-bad)] text-[var(--color-bad)] bg-[var(--color-bad-bg)]',
  info: 'border-[var(--color-info)] text-[var(--color-info)] bg-[var(--color-info-bg)]',
  neutral: 'border-[var(--color-line-2)] muted',
};

export function Banner({
  tone = 'info',
  children,
  actions,
  className,
}: {
  tone?: Tone;
  children: ReactNode;
  actions?: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={clsx(
        'flex items-center justify-between gap-3 rounded-lg border px-3.5 py-2.5 text-sm',
        BANNER_TONE[tone],
        className,
      )}
    >
      <div className="min-w-0">{children}</div>
      {actions && <div className="flex items-center gap-2 shrink-0">{actions}</div>}
    </div>
  );
}
