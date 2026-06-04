import { useEffect, useRef, useState, type MouseEvent } from 'react';
import { cn } from '../../lib/cn';
import { truncate } from '../../utils/format';
import { getAddressExplorerLinks, type AddressKind } from '../../utils/addressLinks';

interface AddressDisplayProps {
  address: string;
  kind: AddressKind;
  /** Shown label; defaults to truncated address */
  display?: string;
  truncateLen?: number;
  className?: string;
  /** Prevent table row selection when interacting with controls */
  stopPropagation?: boolean;
}

function CopyIcon({ copied }: { copied: boolean }) {
  if (copied) {
    return (
      <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3">
        <path
          d="m5 10.5 3.5 3.5L15 7"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
    );
  }
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3">
      <rect
        x="6.5"
        y="6.5"
        width="9"
        height="9"
        rx="1.5"
        stroke="currentColor"
        strokeWidth="1.5"
      />
      <path
        d="M5.5 13.5h-1a1.5 1.5 0 0 1-1.5-1.5v-8a1.5 1.5 0 0 1 1.5-1.5h8a1.5 1.5 0 0 1 1.5 1.5v1"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
      />
    </svg>
  );
}

function ExplorerIconButton({
  href,
  title,
  label,
  className,
  onClick,
}: {
  href: string;
  title: string;
  label: string;
  className: string;
  onClick?: (e: MouseEvent<HTMLAnchorElement>) => void;
}) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      title={title}
      aria-label={title}
      onClick={onClick}
      className={cn(
        'inline-flex size-[18px] shrink-0 items-center justify-center rounded bg-white/5 text-[9px] font-bold hover:bg-white/10',
        className,
      )}
    >
      {label}
    </a>
  );
}

const iconBtn =
  'inline-flex size-[18px] shrink-0 items-center justify-center rounded bg-white/5 text-text-dim transition hover:bg-white/10 hover:text-text';

const HOVER_DELAY_MS = 500;

export function AddressDisplay({
  address,
  kind,
  display,
  truncateLen = 10,
  className,
  stopPropagation = false,
}: AddressDisplayProps) {
  const [copied, setCopied] = useState(false);
  const [showActions, setShowActions] = useState(false);
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const links = getAddressExplorerLinks(kind, address);
  const label = display ?? truncate(address, truncateLen);

  const clearHoverTimer = () => {
    if (hoverTimerRef.current) {
      clearTimeout(hoverTimerRef.current);
      hoverTimerRef.current = null;
    }
  };

  const onMouseEnter = () => {
    clearHoverTimer();
    hoverTimerRef.current = setTimeout(() => setShowActions(true), HOVER_DELAY_MS);
  };

  const onMouseLeave = () => {
    clearHoverTimer();
    setShowActions(false);
  };

  useEffect(() => () => clearHoverTimer(), []);

  const stop = (e: MouseEvent) => {
    if (stopPropagation) e.stopPropagation();
  };

  const copy = async (e: MouseEvent<HTMLButtonElement>) => {
    stop(e);
    try {
      await navigator.clipboard.writeText(address);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  };

  return (
    <div
      className={cn('inline-flex min-w-0 flex-col items-center gap-0.5', className)}
      onClick={stopPropagation ? stop : undefined}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
    >
      <span
        className="max-w-full truncate font-mono text-[11px] text-text-mid"
        title={address}
      >
        {label}
      </span>
      <div
        className={cn(
          'flex items-center justify-center gap-0.5 overflow-hidden transition-[max-height,opacity] duration-150',
          showActions ? 'max-h-6 opacity-100' : 'max-h-0 opacity-0 pointer-events-none',
        )}
      >
        <button
          type="button"
          onClick={copy}
          title={copied ? 'Copied!' : 'Copy address'}
          aria-label={copied ? 'Copied' : 'Copy address'}
          className={cn(iconBtn, copied && 'text-primary')}
          tabIndex={showActions ? 0 : -1}
        >
          <CopyIcon copied={copied} />
        </button>
        {links.gmgn && (
          <ExplorerIconButton
            href={links.gmgn}
            title="Open on GMGN"
            label="G"
            className="text-[#00c97a] hover:bg-[rgba(0,201,122,0.15)]"
            onClick={stop}
          />
        )}
        <ExplorerIconButton
          href={links.solscan}
          title="Open on Solscan"
          label="S"
          className="text-[#9945ff] hover:bg-[rgba(153,69,255,0.15)]"
          onClick={stop}
        />
      </div>
    </div>
  );
}
