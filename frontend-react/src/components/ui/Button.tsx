import type { ButtonHTMLAttributes } from 'react';
import { cn } from '../../lib/cn';

/**
 * Action-based button variants:
 * - `primary`  → confirm / save / apply / create (the main affirmative action)
 * - `ghost`    → neutral / cancel / dismiss (secondary action)
 * - `danger`   → destructive action (delete / confirm-delete)
 * - `subtle`   → compact toolbar pill (refresh / toggle panels)
 * - `link`     → text-only action (clear / reset)
 */
type ButtonVariant = 'primary' | 'ghost' | 'danger' | 'subtle' | 'link';
type ButtonSize = 'xs' | 'sm' | 'md';

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** Highlight as the active/selected state (toggles & segmented controls). */
  active?: boolean;
}

const base =
  'inline-flex items-center justify-center gap-1 rounded-md font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50';

const sizes: Record<ButtonSize, string> = {
  xs: 'rounded px-2 py-0.5 text-[11px]',
  sm: 'min-h-7 px-2.5 py-1 text-[11px]',
  md: 'min-h-8 px-4 py-1.5 text-[13px]',
};

const variants: Record<ButtonVariant, string> = {
  primary:
    'border border-primary bg-primary/15 font-semibold text-primary hover:bg-primary/25 hover:shadow-[0_0_12px_rgba(19,206,175,0.25)]',
  ghost:
    'border border-white/10 bg-transparent text-text-dim hover:border-white/20 hover:bg-white/5 hover:text-text',
  danger:
    'border border-red/50 bg-red/10 text-red hover:border-red/70 hover:bg-red/20',
  subtle:
    'border border-white/8 bg-white/4 font-semibold uppercase tracking-wider text-text-dim hover:text-text',
  link: 'border border-transparent font-semibold text-text-dim hover:text-text',
};

/** Selected/active styling applied on top of the base variant. */
const activeVariants: Partial<Record<ButtonVariant, string>> = {
  ghost: 'border-primary bg-primary/10 font-bold text-primary hover:text-primary',
  subtle: 'border-primary/35 bg-primary/12 text-primary hover:text-primary',
};

export function Button({
  variant = 'ghost',
  size = 'md',
  active = false,
  type = 'button',
  className,
  children,
  ...props
}: ButtonProps) {
  return (
    <button
      type={type}
      className={cn(
        base,
        sizes[size],
        variants[variant],
        active && activeVariants[variant],
        className,
      )}
      {...props}
    >
      {children}
    </button>
  );
}
