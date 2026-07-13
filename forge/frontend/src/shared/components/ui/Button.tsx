import { ButtonHTMLAttributes } from 'react';
import clsx from 'clsx';
import { Icon, IconName } from './Icon';

type Variant = 'default' | 'primary' | 'danger' | 'ghost';

interface Props extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: 'sm' | 'md';
  loading?: boolean;
  /** Optional leading icon. Swapped for the spinner while `loading`. */
  icon?: IconName;
}

const VARIANT: Record<Variant, string> = {
  default: '',
  primary: 'btn-primary',
  danger: 'btn-danger',
  ghost: 'btn-ghost',
};

/** Shared inline spinner — reused by Button + IconButton so they stay in lockstep. */
export function Spinner() {
  return (
    <span className="inline-block h-3 w-3 animate-spin rounded-full border-2 border-current border-t-transparent" />
  );
}

export function Button({
  variant = 'default',
  size = 'md',
  loading,
  icon,
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
      {loading ? <Spinner /> : icon && <Icon name={icon} />}
      {children}
    </button>
  );
}
