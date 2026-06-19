import { memo, useEffect, useRef, useState, type MouseEvent } from 'react';
import { cn } from 'lib/cn';
import { truncate } from 'utils/format';
import { getAddressExplorerLinks, type AddressKind } from 'utils/addressLinks';

interface AddressDisplayProps {
  address: string;
  kind: AddressKind;
  /** Shown label; defaults to truncated address */
  display?: string;
  truncateLen?: number;
  className?: string;
  /** Prevent table row selection when interacting with controls */
  stopPropagation?: boolean;
  /** default: truncated + hover actions; full: full address + always-visible larger actions */
  mode?: 'default' | 'full';
}

function CopyIcon({ copied, size = 'sm' }: { copied: boolean; size?: 'sm' | 'lg' }) {
  const iconCls = size === 'lg' ? 'size-4' : 'size-3';
  if (copied) {
    return (
      <svg viewBox="0 0 20 20" fill="none" aria-hidden className={iconCls}>
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
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className={iconCls}>
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
  size = 'sm',
}: {
  href: string;
  title: string;
  label: string;
  className: string;
  onClick?: (e: MouseEvent<HTMLAnchorElement>) => void;
  size?: 'sm' | 'lg';
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
        'inline-flex shrink-0 items-center justify-center rounded bg-white/5 font-bold hover:bg-white/10',
        size === 'lg' ? 'size-[24px] text-[11px]' : 'size-[18px] text-[9px]',
        className,
      )}
    >
      {label}
    </a>
  );
}

const iconBtnSm =
  'inline-flex size-[18px] shrink-0 items-center justify-center rounded bg-white/5 text-text-dim transition hover:bg-white/10 hover:text-text';
const iconBtnLg =
  'inline-flex size-[24px] shrink-0 items-center justify-center rounded bg-white/5 text-text-dim transition hover:bg-white/10 hover:text-text';

const HOVER_DELAY_MS = 500;

function AddressDisplayBase({
  address,
  kind,
  display,
  truncateLen = 10,
  className,
  stopPropagation = true,
  mode = 'default',
}: AddressDisplayProps) {
  const isFull = mode === 'full';
  const [copied, setCopied] = useState(false);
  const [showActions, setShowActions] = useState(isFull);
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const links = getAddressExplorerLinks(kind, address);
  const label = isFull ? address : (display ?? truncate(address, truncateLen));
  const iconSize = isFull ? 'lg' : 'sm';
  const iconBtn = isFull ? iconBtnLg : iconBtnSm;

  const clearHoverTimer = () => {
    if (hoverTimerRef.current) {
      clearTimeout(hoverTimerRef.current);
      hoverTimerRef.current = null;
    }
  };

  const onMouseEnter = () => {
    if (isFull) return;
    clearHoverTimer();
    hoverTimerRef.current = setTimeout(() => setShowActions(true), HOVER_DELAY_MS);
  };

  const onMouseLeave = () => {
    if (isFull) return;
    clearHoverTimer();
    setShowActions(false);
  };

  const clearCopyTimer = () => {
    if (copyTimerRef.current) {
      clearTimeout(copyTimerRef.current);
      copyTimerRef.current = null;
    }
  };

  useEffect(
    () => () => {
      clearHoverTimer();
      clearCopyTimer();
    },
    [],
  );

  const stop = (e: MouseEvent) => {
    if (stopPropagation) e.stopPropagation();
  };

  const copy = async (e: MouseEvent<HTMLButtonElement>) => {
    stop(e);
    try {
      await navigator.clipboard.writeText(address);
      setCopied(true);
      clearCopyTimer();
      copyTimerRef.current = setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  };

  return (
    <div
      className={cn(
        'inline-flex min-w-0 flex-col gap-0.5',
        isFull ? 'items-start' : 'items-center',
        className,
      )}
      onClick={stopPropagation ? stop : undefined}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
    >
      <span
        className={cn(
          'max-w-full font-mono text-text-mid',
          isFull ? 'text-[10px] leading-snug break-all' : 'truncate text-[11px]',
        )}
        title={isFull ? undefined : address}
      >
        {label}
      </span>
      <div
        className={cn(
          'flex items-center gap-0.5',
          !isFull && 'justify-center overflow-hidden transition-[max-height,opacity] duration-150',
          isFull || showActions ? 'opacity-100' : 'max-h-0 opacity-0 pointer-events-none',
          !isFull && (showActions ? 'max-h-6' : 'max-h-0'),
        )}
      >
        <button
          type="button"
          onClick={copy}
          title={copied ? 'Copied!' : 'Copy address'}
          aria-label={copied ? 'Copied' : 'Copy address'}
          className={cn(iconBtn, copied && 'text-primary')}
          tabIndex={isFull || showActions ? 0 : -1}
        >
          <CopyIcon copied={copied} size={iconSize} />
        </button>
        {links.gmgn && (
          <ExplorerIconButton
            href={links.gmgn}
            title="Open on GMGN"
            label="G"
            size={iconSize}
            className="text-[#00c97a] hover:bg-[rgba(0,201,122,0.15)]"
            onClick={stop}
          />
        )}
        <ExplorerIconButton
          href={links.solscan}
          title="Open on Solscan"
          label="S"
          size={iconSize}
          className="text-[#9945ff] hover:bg-[rgba(153,69,255,0.15)]"
          onClick={stop}
        />
      </div>
    </div>
  );
}

export const AddressDisplay = memo(AddressDisplayBase);
