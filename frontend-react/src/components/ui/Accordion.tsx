import { useState } from 'react';
import { cn } from 'lib/cn';

interface AccordionProps {
  /** Simple text/node title — the entire header row is the collapse toggle. Chevron on right. */
  title?: React.ReactNode;
  /** Extra content rendered inline with the title (badges, info icons). Only used with `title`. */
  badge?: React.ReactNode;
  /** Full custom header content. The chevron button is prepended at the left edge
   *  and is the *only* collapse trigger — interactive controls inside `header`
   *  (selects, buttons, inputs) are not affected. */
  header?: React.ReactNode;
  defaultOpen?: boolean;
  children: React.ReactNode;
  className?: string;
}

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

export function Accordion({
  title,
  badge,
  header,
  defaultOpen = true,
  children,
  className,
}: AccordionProps) {
  const [open, setOpen] = useState(defaultOpen);
  const toggle = () => setOpen((o) => !o);

  return (
    <div className={cn('overflow-hidden rounded-lg border border-white/10', className)}>
      {header !== undefined ? (
        // Interactive-header mode: only the chevron button toggles collapse.
        <div className="flex min-h-[48px] items-center gap-2 px-3 py-2">
          <button
            type="button"
            aria-expanded={open}
            onClick={toggle}
            className="flex shrink-0 items-center rounded-md p-1 text-text-dim hover:bg-white/8 hover:text-text"
          >
            <Chevron open={open} />
          </button>
          {header}
        </div>
      ) : (
        // Title mode: entire header row is the collapse button, chevron on right (MUI style).
        <button
          type="button"
          aria-expanded={open}
          onClick={toggle}
          className="flex min-h-[48px] w-full items-center justify-between gap-3 px-4 py-3 text-left hover:bg-white/5"
        >
          <span className="flex items-center gap-2">
            <span className="text-sm font-semibold text-text">{title}</span>
            {badge}
          </span>
          <Chevron open={open} />
        </button>
      )}
      {open && <div className="border-t border-white/10 px-4 py-4">{children}</div>}
    </div>
  );
}
