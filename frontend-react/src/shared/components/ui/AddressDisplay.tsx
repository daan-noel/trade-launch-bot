import { memo, useEffect, useRef, useState, type MouseEvent } from 'react';
import { cn } from 'lib/cn';
import { truncate as truncateStr } from 'utils/format';
import { getAddressExplorerLinks, type AddressKind } from 'utils/addressLinks';

/** When the copy/explorer icon buttons become visible. */
export type AddressReveal = 'hover' | 'always';
/** Where the icon-button row sits relative to the label. `bottom` is always centered. */
export type AddressActionsPlacement = 'bottom' | 'right';

interface AddressDisplayProps {
  address: string;
  kind: AddressKind;
  /** Shown label; defaults to the (optionally shortened) address */
  display?: string;
  className?: string;
  /** Prevent table row selection when interacting with controls */
  stopPropagation?: boolean;
  /**
   * Convenience preset that seeds the defaults for the orthogonal props below.
   * Any explicit prop still wins over the preset.
   * - `default`: truncated + hover actions + small icons, actions below (centered)
   * - `full`: full address + always-visible larger icons, actions below (centered)
   */
  mode?: 'default' | 'full';
  /** When the icon buttons appear. */
  reveal?: AddressReveal;
  /** How the label is shortened: character budget, or `false` for the full address. */
  truncate?: number | false;
  /** Where the icon row sits relative to the label. */
  actionsPlacement?: AddressActionsPlacement;
  /** Icon-button size. */
  iconSize?: 'sm' | 'lg';
  /** @deprecated use `truncate` instead */
  truncateLen?: number;
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
  className,
  stopPropagation = true,
  mode = 'default',
  reveal: revealProp,
  truncate: truncateProp,
  actionsPlacement,
  iconSize: iconSizeProp,
  truncateLen,
}: AddressDisplayProps) {
  // Resolve orthogonal props: explicit prop > deprecated alias > preset default.
  const reveal: AddressReveal = revealProp ?? (mode === 'full' ? 'always' : 'hover');
  const shorten: number | false =
    truncateProp ?? truncateLen ?? (mode === 'full' ? false : 10);
  const placement: AddressActionsPlacement = actionsPlacement ?? 'bottom';
  const iconSize = iconSizeProp ?? (mode === 'full' ? 'lg' : 'sm');

  const showFull = shorten === false;
  const alwaysShow = reveal === 'always';
  const isRight = placement === 'right';
  // Hover-reveal on the inline `right` placement floats the actions as an overlay
  // so revealing them never reflows a dense table row; `bottom` animates max-height.
  const overlay = isRight && !alwaysShow;

  const [copied, setCopied] = useState(false);
  const [showActions, setShowActions] = useState(alwaysShow);
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const links = getAddressExplorerLinks(kind, address);
  const label = display ?? (showFull ? address : truncateStr(address, shorten));
  const iconBtn = iconSize === 'lg' ? iconBtnLg : iconBtnSm;
  const visible = alwaysShow || showActions;

  const clearHoverTimer = () => {
    if (hoverTimerRef.current) {
      clearTimeout(hoverTimerRef.current);
      hoverTimerRef.current = null;
    }
  };

  const onMouseEnter = () => {
    if (alwaysShow) return;
    clearHoverTimer();
    hoverTimerRef.current = setTimeout(() => setShowActions(true), HOVER_DELAY_MS);
  };

  const onMouseLeave = () => {
    if (alwaysShow) return;
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
        'inline-flex min-w-0',
        isRight
          ? 'relative flex-row items-center gap-1'
          : cn('flex-col gap-0.5', showFull ? 'items-start' : 'items-center'),
        className,
      )}
      onClick={stopPropagation ? stop : undefined}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
    >
      <span
        className={cn(
          'max-w-full font-mono text-text-mid',
          showFull ? 'text-[10px] leading-snug break-all' : 'truncate text-[11px]',
        )}
        title={showFull ? undefined : address}
      >
        {label}
      </span>
      <div
        className={cn(
          'flex items-center gap-0.5',
          overlay &&
            cn(
              'absolute left-full top-1/2 z-10 -translate-y-1/2 transition-opacity duration-150',
              visible ? 'opacity-100' : 'pointer-events-none opacity-0',
            ),
          !isRight &&
            cn(
              'justify-center overflow-hidden transition-[max-height,opacity] duration-150',
              visible ? 'max-h-6 opacity-100' : 'max-h-0 pointer-events-none opacity-0',
            ),
        )}
      >
        <button
          type="button"
          onClick={copy}
          title={copied ? 'Copied!' : 'Copy address'}
          aria-label={copied ? 'Copied' : 'Copy address'}
          className={cn(iconBtn, copied && 'text-primary')}
          tabIndex={visible ? 0 : -1}
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
