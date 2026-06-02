import type { ButtonHTMLAttributes } from 'react';
import { cn } from '../../lib/cn';

type ButtonVariant = 'ghost' | 'primary';

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
}

const variants: Record<ButtonVariant, string> = {
  ghost:
    'border border-white/10 bg-transparent text-text-dim hover:border-white/20 hover:bg-white/5 hover:text-text',
  primary:
    'border border-primary bg-primary/15 text-primary hover:bg-primary/25 hover:shadow-[0_0_12px_rgba(19,206,175,0.25)]',
};

export function Button({
  variant = 'ghost',
  className,
  children,
  ...props
}: ButtonProps) {
  return (
    <button
      className={cn(
        'inline-flex min-h-8 items-center justify-center rounded-md px-4 text-[13px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-55',
        variants[variant],
        className,
      )}
      {...props}
    >
      {children}
    </button>
  );
}
