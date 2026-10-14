// Shared picture pieces for the workflow guides: numbered steps, yes/no arrows, chips.

import type { ReactNode } from 'react';

import { cn } from 'lib/cn';

export function Chip({ tone = 'plain', children }: { tone?: 'keep' | 'drop' | 'plain'; children: ReactNode }) {
  return (
    <span
      className={cn(
        'rounded border px-1.5 py-0.5 text-[11px] leading-snug',
        tone === 'keep' && 'border-green/40 bg-green/10 text-green',
        tone === 'drop' && 'border-red/40 bg-red/10 text-red',
        tone === 'plain' && 'border-white/10 bg-white/4 text-text-mid',
      )}
    >
      {children}
    </span>
  );
}

export function Chips({ names, tone = 'plain' }: { names: string[]; tone?: 'keep' | 'drop' | 'plain' }) {
  return (
    <div className="flex flex-wrap gap-1">
      {names.map((name) => (
        <Chip key={name} tone={tone}>
          {name}
        </Chip>
      ))}
    </div>
  );
}

export function Card({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div className={cn('flex min-w-0 flex-col items-start gap-1.5 rounded-md border border-white/10 bg-bg-card p-2', className)}>
      {children}
    </div>
  );
}

export function Pass({ n, title, hint, children }: { n: number; title: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3" aria-label={`${n}. ${title}`}>
      <div className="-ml-10 flex items-center gap-3">
        <span className="relative z-10 grid size-7 shrink-0 place-items-center rounded-full border border-white/40 bg-bg-panel text-[12px] font-semibold text-text">
          {n}
        </span>
        <div className="flex min-w-0 flex-col gap-1">
          <strong className="text-[13px] text-text">{title}</strong>
          {hint}
        </div>
      </div>
      {children}
    </section>
  );
}

function ArrowHead({ down }: { down?: boolean }) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        'border-solid border-current',
        down
          ? '-mt-px border-x-[8px] border-t-[12px] border-x-transparent border-t-current'
          : '-ml-px border-y-[8px] border-l-[12px] border-y-transparent border-l-current',
      )}
    />
  );
}

function Arrow({ down, children }: { down?: boolean; children: ReactNode }) {
  return (
    <span className="inline-flex shrink-0 items-center gap-2 text-[13px] text-white/15">
      {down ? (
        <span className="flex flex-col items-center text-white/8" aria-hidden="true">
          <span className="h-12 w-1 bg-current" />
          <ArrowHead down />
        </span>
      ) : (
        <span className="flex items-center text-white/8" aria-hidden="true">
          <span className="h-1 w-20 bg-current" />
          <ArrowHead />
        </span>
      )}
      {children}
    </span>
  );
}

export function DownJoin() {
  return (
    <div className="-ml-10 flex h-16 w-7 justify-center" aria-hidden="true">
      <span className="flex h-full flex-col items-center text-white/8">
        <span className="w-1 flex-1 bg-current" />
        <ArrowHead down />
      </span>
    </div>
  );
}

export function Fall({ label = 'No' }: { label?: string }) {
  return (
    <div className="py-1 pl-1">
      <Arrow down>{label}</Arrow>
    </div>
  );
}

export function Lane({ ask, via, children }: { ask: ReactNode; via: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <div className="min-w-56 flex-1">{ask}</div>
      <Arrow>{via}</Arrow>
      <div className="flex min-w-44 flex-1 flex-wrap gap-1">{children}</div>
    </div>
  );
}
