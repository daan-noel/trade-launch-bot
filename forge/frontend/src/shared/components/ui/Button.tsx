import { ButtonHTMLAttributes } from 'react';
import clsx from 'clsx';

type Variant = 'default' | 'primary' | 'danger' | 'ghost';

interface Props extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: 'sm' | 'md';
  loading?: boolean;
}

const VARIANT: Record<Variant, string> = {
  default: '',
  primary: 'btn-primary',
  danger: 'btn-danger',
  ghost: 'btn-ghost',
};

export function Button({
  variant = 'default',
  size = 'md',
  loading,
  disabled,
  className,
  children,
  ...rest
}: Props) {
  return (
    <button
      type="button"
      className={clsx('btn', VARIANT[variant], size === 'sm' && 'btn-sm', className)}
      disabled={disabled || loading}
      {...rest}
    >
      {loading && <span className="inline-block h-3 w-3 animate-spin rounded-full border-2 border-current border-t-transparent" />}
      {children}
    </button>
  );
}
