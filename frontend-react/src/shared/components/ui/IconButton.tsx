import type { ButtonHTMLAttributes } from 'react';
import { cn } from 'lib/cn';

type IconButtonVariant = 'ghost' | 'danger';

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: IconButtonVariant;
  /** Apply active/selected highlight (e.g. result panel is open). */
  active?: boolean;
  activeClassName?: string;
}

const base =
  'inline-flex h-7 w-7 items-center justify-center rounded border text-[13px] transition-colors disabled:cursor-not-allowed disabled:opacity-50';

const variants: Record<IconButtonVariant, string> = {
  ghost: 'border-white/10 bg-transparent text-text-dim hover:border-white/20 hover:bg-white/5 hover:text-text',
  danger: 'border-red/40 bg-transparent text-red hover:border-red/60 hover:bg-red/10',
};

export function IconButton({
  variant = 'ghost',
  active = false,
  activeClassName,
  type = 'button',
  className,
  children,
  ...props
}: IconButtonProps) {
  return (
    <button
      type={type}
      className={cn(base, variants[variant], active && activeClassName, className)}
      {...props}
    >
      {children}
    </button>
  );
}
