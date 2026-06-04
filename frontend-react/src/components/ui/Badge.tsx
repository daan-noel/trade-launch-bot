import type { HTMLAttributes } from 'react';
import { cn } from '../../lib/cn';

/**
 * Status pill for compact labels (migration state, trade mode, statuses…).
 * Variants map to the `@theme` color tokens.
 */
export type BadgeVariant =
  | 'neutral'
  | 'primary'
  | 'info'
  | 'success'
  | 'danger'
  | 'warning'
  | 'accent';
export type BadgeSize = 'sm' | 'md';

interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: BadgeVariant;
  size?: BadgeSize;
  /** Fully rounded pill shape instead of the default rounded-md. */
  pill?: boolean;
}

const sizes: Record<BadgeSize, string> = {
  sm: 'px-2 py-0.5 text-[10px]',
  md: 'px-2.5 py-0.5 text-[11px]',
};

const variants: Record<BadgeVariant, string> = {
  neutral: 'border border-white/10 bg-white/5 text-text-dim',
  primary: 'border border-primary/30 bg-primary/12 text-primary',
  info: 'border border-info/40 bg-info/12 text-info',
  success: 'border border-green/40 bg-green/12 text-green',
  danger: 'border border-red/40 bg-red/12 text-red',
  warning: 'border border-warning/40 bg-warning/12 text-warning',
  accent: 'border border-accent/40 bg-accent/12 text-accent',
};

export function Badge({
  variant = 'neutral',
  size = 'md',
  pill = false,
  className,
  children,
  ...props
}: BadgeProps) {
  return (
    <span
      className={cn(
        'inline-flex items-center font-bold tracking-wide cursor-default',
        pill ? 'rounded-full' : 'rounded-md',
        sizes[size],
        variants[variant],
        className,
      )}
      {...props}
    >
      {children}
    </span>
  );
}
