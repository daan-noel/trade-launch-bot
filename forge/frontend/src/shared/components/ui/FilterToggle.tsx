import { ButtonHTMLAttributes, ReactNode } from 'react';
import clsx from 'clsx';
import { Badge, type Tone } from './Badge';

interface Props extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> {
  /**
   * Tone driving the badge color. Pass the SAME tone the matching status pill
   * uses (e.g. `statusTone('retired')`) so the toggle reads as "this group" at a
   * glance — the badge color == the table pill color, straight off the SSOT.
   */
  tone?: Tone;
  /** Whether the group is currently shown. Flips the eye glyph + badge dim. */
  active: boolean;
  /** How many rows are hidden — folded into the tooltip, not rendered, to stay narrow. */
  count?: number;
  /** Short group label rendered inside the colored badge, e.g. "Retired". */
  children: ReactNode;
}

/** Eye (shown) / eye-off (hidden) glyph — inline so we carry no icon dependency. */
function EyeIcon({ off }: { off: boolean }) {
  return (
    <svg className="ft-icon" width="14" height="14" viewBox="0 0 24 24" fill="none"
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
      aria-hidden="true">
      <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7Z" />
      <circle cx="12" cy="12" r="3" />
      {off && <line x1="3" y1="3" x2="21" y2="21" />}
    </svg>
  );
}

/**
 * Compact show/hide toggle: an eye glyph + a status-colored badge, nothing else.
 * Reuses the Badge tone SSOT so the color never drifts from the pills, and the
 * hidden count lives in the tooltip so the control stays narrow.
 */
export function FilterToggle({
  tone = 'neutral',
  active,
  count,
  disabled,
  className,
  title,
  children,
  ...rest
}: Props) {
  const hint =
    count != null && count > 0 ? `${title ? `${title} ` : ''}(${count} hidden)` : title;
  return (
    <button
      type="button"
      data-active={active}
      disabled={disabled}
      title={hint}
      aria-pressed={active}
      className={clsx('filter-toggle', className)}
      {...rest}
    >
      <EyeIcon off={!active} />
      <Badge tone={tone}>{children}</Badge>
    </button>
  );
}
