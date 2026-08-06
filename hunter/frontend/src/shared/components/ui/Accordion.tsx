import { useState } from 'react';
import { cn } from 'lib/cn';

interface AccordionProps {
  /** Simple text/node title — the entire header row is the collapse toggle. Chevron on right. */
  title?: React.ReactNode;
  /** Extra content rendered inline with the title (badges, info icons). Only used with `title`. */
  badge?: React.ReactNode;
  /** Full custom header content. A labelled chevron button is prepended at the left
   *  edge, and the header row's own empty/static area toggles too — but interactive
   *  controls inside `header` (selects, buttons, inputs, their labels) never do. */
  header?: React.ReactNode;
  /** Text shown next to the chevron in `header` mode. Without it the only visible
   *  affordance is a ~20px icon, which is a poor click target for a panel toggle. */
  toggleLabel?: React.ReactNode;
  defaultOpen?: boolean;
  children: React.ReactNode;
  className?: string;
  /** Draw the outer border and the header/content divider. Default true. */
  bordered?: boolean;
  /** Header + content spacing. Default 'default' (matches legacy look). */
  padding?: 'default' | 'sm' | 'none';
  /** Persist the open/closed state under this localStorage key (survives reloads).
   *  Falls back to `defaultOpen` when unset or unreadable. */
  storageKey?: string;
}

function readStoredOpen(key: string | undefined, fallback: boolean): boolean {
  if (!key) return fallback;
  try {
    const v = localStorage.getItem(key);
    if (v === '0') return false;
    if (v === '1') return true;
  } catch {
    /* ignore */
  }
  return fallback;
}

const PAD = {
  default: { row: 'min-h-[48px]', header: 'px-3 py-2', title: 'px-4 py-3', body: 'px-4 py-4' },
  sm: { row: 'min-h-0', header: 'px-2 py-1', title: 'px-2 py-1.5', body: 'px-2 py-2' },
  none: { row: 'min-h-0', header: 'p-0', title: 'p-0', body: 'pt-2' },
} as const;

function Chevron({ open }: { open: boolean }) {
  return (
    <svg
      className={cn(
        'h-5 w-5 shrink-0 text-text-dim transition-transform duration-200',
        open && 'rotate-90',
      )}
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <polyline points="7,4 13,10 7,16" />
    </svg>
  );
}

/** Elements inside a custom `header` that own their own click semantics — a click
 *  landing in one of these must never also collapse/expand the panel. `label` is in
 *  the list because clicking it focuses/activates its control, not the accordion. */
const HEADER_INTERACTIVE = 'button, select, input, textarea, a, label, [role="button"]';

export function Accordion({
  title,
  badge,
  header,
  toggleLabel,
  defaultOpen = true,
  children,
  className,
  bordered = true,
  padding = 'default',
  storageKey,
}: AccordionProps) {
  const [open, setOpen] = useState(() => readStoredOpen(storageKey, defaultOpen));
  const toggle = () =>
    setOpen((o) => {
      const next = !o;
      if (storageKey) {
        try {
          localStorage.setItem(storageKey, next ? '1' : '0');
        } catch {
          /* ignore */
        }
      }
      return next;
    });
  const pad = PAD[padding];

  return (
    <div
      className={cn(
        'overflow-hidden rounded-lg',
        bordered && 'border border-white/10',
        className,
      )}
    >
      {header !== undefined ? (
        // Interactive-header mode: the row's static area toggles collapse, but a click
        // that lands on one of the header's own controls is left entirely alone.
        <div
          className={cn('flex cursor-pointer items-center gap-2 hover:bg-white/5', pad.row, pad.header)}
          onClick={(e) => {
            if ((e.target as HTMLElement).closest(HEADER_INTERACTIVE)) return;
            toggle();
          }}
        >
          <button
            type="button"
            aria-expanded={open}
            onClick={toggle}
            className="flex shrink-0 items-center gap-1 rounded-md p-1 text-text-dim hover:bg-white/8 hover:text-text"
          >
            <Chevron open={open} />
            {toggleLabel !== undefined && (
              <span className="text-sm font-medium">{toggleLabel}</span>
            )}
          </button>
          {header}
        </div>
      ) : (
        // Title mode: entire header row is the collapse button, chevron on right (MUI style).
        <button
          type="button"
          aria-expanded={open}
          onClick={toggle}
          className={cn(
            'flex w-full items-center justify-between gap-3 text-left hover:bg-white/5',
            pad.row,
            pad.title,
          )}
        >
          <span className="flex items-center gap-2">
            <span className="text-sm font-semibold text-text">{title}</span>
            {badge}
          </span>
          <Chevron open={open} />
        </button>
      )}
      {open && (
        <div className={cn(bordered && 'border-t border-white/10', pad.body)}>{children}</div>
      )}
    </div>
  );
}
