import type { ButtonHTMLAttributes } from 'react';
import { cn } from '../../lib/cn';

type StatusState = 'live' | 'dead';

interface StatusButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  state: StatusState;
  label?: string;
}

export function StatusButton({ state, label, className, ...props }: StatusButtonProps) {
  return (
    <button
      className={cn(
        'cursor-pointer inline-flex min-h-8 items-center justify-center rounded-full border px-3.5 text-[11px] font-bold uppercase tracking-wider transition-colors',
        state === 'live'
          ? 'border-green/35 bg-green/12 text-green shadow-[0_0_12px_rgba(8,153,129,0.15)]'
          : 'border-red/35 bg-red/12 text-red',
        className,
      )}
      {...props}
    >
      <span
        className={cn(
          'mr-1.5 inline-block h-1.5 w-1.5 rounded-full',
          state === 'live'
            ? 'bg-green shadow-[0_0_6px_rgba(8,153,129,0.8)] animate-pulse'
            : 'bg-red',
        )}
      />
      {label ?? (state === 'live' ? 'LIVE' : 'DEAD')}
    </button>
  );
}
