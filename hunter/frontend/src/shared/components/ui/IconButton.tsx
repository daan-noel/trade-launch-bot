import type { ButtonHTMLAttributes, ReactNode } from 'react';
import { cn } from 'lib/cn';

/**
 * Action button — icon-only by default, or icon + label for primary CTAs.
 * Always pass `title` (and `aria-label` when icon-only) so the action stays
 * discoverable; with `label`, aria-label defaults to the visible text.
 *
 * - `primary` → main affirmative CTA (run / save)
 * - `success` → create / add (green)
 * - `accent`  → edit (orange)
 * - `danger`  → delete / sell (red)
 * - `ghost`   → secondary toolbar / row action
 * - `subtle`  → compact toolbar pill tone
 */
type IconButtonVariant = 'ghost' | 'danger' | 'primary' | 'subtle' | 'success' | 'accent';
type IconButtonSize = 'sm' | 'md' | 'lg';

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: IconButtonVariant;
  /** `sm` 28px · `md` 36px · `lg` 44px (easy click target). */
  size?: IconButtonSize;
  /** Visible text beside the icon — use on main page CTAs, not dense row actions. */
  label?: ReactNode;
  /** Apply active/selected highlight (e.g. panel is open). */
  active?: boolean;
  activeClassName?: string;
}

const base =
  'inline-flex shrink-0 items-center justify-center rounded-md border leading-none transition-colors disabled:cursor-not-allowed disabled:opacity-50';

const iconOnlySizes: Record<IconButtonSize, string> = {
  sm: 'h-7 w-7 p-0 [&_svg]:size-3.5',
  md: 'h-9 w-9 p-0 [&_svg]:size-4',
  lg: 'h-11 w-11 p-0 [&_svg]:size-5',
};

/** Flexible height; gap lives on the inner icon+label row (not the button). */
const labeledSizes: Record<IconButtonSize, string> = {
  sm: 'min-h-7 px-2 py-1 text-[11px] font-medium',
  md: 'min-h-9 px-2.5 py-1.5 text-[12px] font-medium',
  lg: 'min-h-11 px-3.5 py-2 text-[13px] font-semibold',
};

const labeledGaps: Record<IconButtonSize, string> = {
  sm: 'gap-1',
  md: 'gap-1.5',
  lg: 'gap-2',
};

const labeledIconSizes: Record<IconButtonSize, string> = {
  sm: '[&_svg]:size-3.5',
  md: '[&_svg]:size-4',
  lg: '[&_svg]:size-5',
};

const variants: Record<IconButtonVariant, string> = {
  primary:
    'border-primary bg-primary/15 font-semibold text-primary hover:bg-primary/25 hover:shadow-[0_0_12px_color-mix(in_srgb,var(--color-primary)_25%,transparent)]',
  success:
    'border-green/50 bg-green/12 text-green hover:border-green/70 hover:bg-green/22',
  accent:
    'border-accent/50 bg-accent/12 text-accent hover:border-accent/70 hover:bg-accent/22',
  ghost:
    'border-white/10 bg-transparent text-text-dim hover:border-white/20 hover:bg-white/5 hover:text-text',
  danger:
    'border-red/50 bg-red/10 text-red hover:border-red/70 hover:bg-red/20',
  subtle:
    'border-white/8 bg-white/4 text-text-dim hover:text-text',
};

const activeVariants: Partial<Record<IconButtonVariant, string>> = {
  ghost: 'border-primary bg-primary/10 text-primary hover:text-primary',
  subtle: 'border-primary/35 bg-primary/12 text-primary hover:text-primary',
  primary: 'border-primary bg-primary/30 text-primary',
  success: 'border-green bg-green/25 text-green',
  accent: 'border-accent bg-accent/25 text-accent',
};

export function IconButton({
  variant = 'ghost',
  size = 'md',
  label,
  active = false,
  activeClassName,
  type = 'button',
  className,
  children,
  title,
  'aria-label': ariaLabel,
  onClick,
  ...props
}: IconButtonProps) {
  const labeled = label != null && label !== '';
  const labelText = typeof label === 'string' ? label : undefined;
  return (
    <button
      type={type}
      title={title ?? labelText}
      aria-label={ariaLabel ?? labelText}
      className={cn(
        base,
        labeled ? labeledSizes[size] : iconOnlySizes[size],
        variants[variant],
        active && (activeClassName ?? activeVariants[variant]),
        className,
      )}
      onClick={
        onClick
          ? (e) => {
              // IconButtons are almost always rendered inside a clickable row/card
              // (table rows, chart cards) — without this, its click bubbles up and
              // fires that ancestor's own onClick (e.g. row selection) alongside
              // whatever this button does.
              e.stopPropagation();
              onClick(e);
            }
          : undefined
      }
      {...props}
    >
      {labeled ? (
        // Own flex row so icon+label middle-align to each other (not text baseline).
        <span className={cn('flex items-center', labeledGaps[size], labeledIconSizes[size])}>
          {children != null && (
            <span className="flex shrink-0 items-center justify-center [&_svg]:block">
              {children}
            </span>
          )}
          <span className="leading-none whitespace-nowrap">{label}</span>
        </span>
      ) : (
        children != null && (
          <span className="flex items-center justify-center [&_svg]:block">{children}</span>
        )
      )}
    </button>
  );
}
