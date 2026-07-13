import { ButtonHTMLAttributes } from 'react';
import clsx from 'clsx';
import { Icon, IconName } from './Icon';
import { Spinner } from './Button';

type Variant = 'default' | 'primary' | 'danger' | 'ghost';

interface Props extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> {
  icon: IconName;
  /** Required — becomes the tooltip AND the accessible name, since there's no visible text. */
  label: string;
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

/**
 * Square icon-only button for compact, at-a-glance actions (row actions,
 * pagination, toolbar utilities). `label` is mandatory so every icon-only control
 * still has a tooltip + screen-reader name.
 */
export function IconButton({
  icon,
  label,
  variant = 'default',
  size = 'sm',
  loading,
  disabled,
  className,
  ...rest
}: Props) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      className={clsx('btn btn-icon', VARIANT[variant], size === 'sm' && 'btn-sm', className)}
      disabled={disabled || loading}
      {...rest}
    >
      {loading ? <Spinner /> : <Icon name={icon} />}
    </button>
  );
}
