import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useGetTokenDetailQuery } from 'store/sharedEndpoints';
import { LazyTokenTradeChart } from 'components/tokens/LazyTokenTradeChart';
import type { ChartEventMarker } from 'components/token-price-chart';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { cn } from 'lib/cn';

/** Per-row chart overlay — the entry/exit markers a caller wants drawn on that
 *  row's inline chart, matching what its inspect modal shows. */
export interface RowChartOverlay {
  eventMarkers?: ChartEventMarker[] | null;
}

/**
 * Resolve a row's chart overlay. Called once per chart card as a **hook** (so an
 * implementation may run its own `useState`/`useEffect` — e.g. the swing1 charts
 * fetch their legs per token via `swing1-detect`), so obey the rules of hooks:
 * always call the same hooks in the same order regardless of the row. Entry/exit
 * overlays that just derive from row data call no hooks and are trivially safe.
 */
export type ChartOverlayHook<R> = (row: R, mint: string) => RowChartOverlay;

const NO_OVERLAY: RowChartOverlay = {};
/** Stable default so cards always call a hook (rules-of-hooks) when none passed. */
const useNoRowOverlay: ChartOverlayHook<unknown> = () => NO_OVERLAY;

/**
 * A grid of per-token trade-history charts, one card per row — the generalized
 * form of the Trader Analysis charts grid. Each card fetches its own
 * `getTokenDetail` on mount and lazily mounts on scroll, so a full page of rows
 * only fans out fetches for what's on screen (plus a pre-load margin). Fed by a
 * table's current on-screen rows (see {@link TokenTable}'s `charts` toggle, or a
 * page's own `onVisibleRowsChange`), so the grid mirrors the table's sort/filter/
 * page — **current page only** (never the full filtered set).
 *
 * Generic over the row shape via `mintOf`; a `renderChartCardExtra` slot lets a
 * caller add per-row context to a card header (e.g. Trader Analysis's per-wallet
 * buys/sells stats) without the grid knowing the row type.
 */

/**
 * Defer mounting `children` until the placeholder scrolls near the viewport. Each
 * chart fires its own detail + trades fetch on mount, so with many rows on the page
 * we must NOT mount them all at once — this keeps the fan-out to what's on screen
 * (plus a 400px pre-load margin). Once shown it stays mounted (a re-sort reorders
 * the DOM but keeps charts mounted via `key`).
 */
export function LazyMount({ minHeight = 380, children }: { minHeight?: number; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [shown, setShown] = useState(false);

  useEffect(() => {
    if (shown) return;
    const el = ref.current;
    if (!el) return;
    const obs = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setShown(true);
          obs.disconnect();
        }
      },
      { rootMargin: '400px' },
    );
    obs.observe(el);
    return () => obs.disconnect();
  }, [shown]);

  return (
    <div ref={ref} style={shown ? undefined : { minHeight }}>
      {shown ? children : null}
    </div>
  );
}

interface TokenChartCardProps<R> {
  row: R;
  mint_address: string;
  /** Header title; falls back to the fetched detail's symbol/name, then the mint. */
  title?: string;
  highlightWallet?: string | null;
  chartTableId: string;
  /** Resolves this row's entry/exit + swing overlay (called as a hook). */
  useOverlay: ChartOverlayHook<R>;
  /** Extra content rendered in the card header (per-row context). */
  extra?: ReactNode;
  selected?: boolean;
  /** Header click → select (chart canvas stays interactive — see stopPropagation). */
  onSelect?: () => void;
}

function TokenChartCard<R>({
  row,
  mint_address: mint,
  title,
  highlightWallet,
  chartTableId,
  useOverlay,
  extra,
  selected,
  onSelect,
}: TokenChartCardProps<R>) {
  const { data: detail } = useGetTokenDetailQuery(mint, { skip: !mint });
  const heading = title ?? detail?.symbol ?? detail?.name ?? mint.slice(0, 6);
  const { eventMarkers } = useOverlay(row, mint);
  const selectable = !!onSelect;

  return (
    <div
      className={cn(
        'rounded-lg border bg-bg-card/40 p-4 transition',
        selected
          ? 'border-primary/45 bg-primary/8 shadow-[0_14px_32px_rgba(2,192,118,0.08)]'
          : 'border-white/8',
      )}
    >
      <div
        className={cn(
          'mb-2 flex flex-wrap items-center gap-2 rounded-md -mx-1 px-1 py-0.5',
          selectable && 'cursor-pointer hover:bg-white/4',
        )}
        onClick={onSelect}
        role={selectable ? 'button' : undefined}
        tabIndex={selectable ? 0 : undefined}
        onKeyDown={
          selectable
            ? (e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  onSelect();
                }
              }
            : undefined
        }
        title={selectable ? 'Select token' : undefined}
      >
        <span className="text-sm font-bold text-text">{heading}</span>
        <AddressDisplay
          address={mint}
          kind="token"
          truncate={false}
          actionsPlacement="right"
          iconSize="sm"
        />
        {extra}
      </div>
      {/* Keep chart pan/zoom/bar-select from bubbling into card selection. */}
      <div onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
        <LazyTokenTradeChart
          key={mint}
          detail={detail ?? null}
          eventMarkers={eventMarkers ?? null}
          highlightWallet={highlightWallet ?? null}
          tableId={chartTableId}
        />
      </div>
    </div>
  );
}

export interface TokenChartsGridProps<R> {
  /** The rows to chart — the table's CURRENT on-screen page, never the full set. */
  rows: R[];
  /** Optional per-row header title (else derived from the fetched detail). */
  titleOf?: (row: R) => string;
  /** Chart-local prefs id (column visibility of the per-chart trades table). */
  chartTableId?: string;
  /** Wallet to spotlight on every chart (Trader Analysis). */
  highlightWallet?: string | null;
  /** Per-row entry/exit + swing overlay (matches the row's inspect modal). Called
   *  as a hook per card — see {@link ChartOverlayHook}. Omit for a plain chart. */
  useRowOverlay?: ChartOverlayHook<R>;
  /** Extra header content per card (e.g. per-wallet buys/sells). */
  renderChartCardExtra?: (row: R) => ReactNode;
  /** Highlight the card whose mint matches (same key as the table's `selectedKey`). */
  selectedKey?: string | null;
  /** Header click on a card — same contract as the table row select (toggle). */
  onSelect?: (key: string | null) => void;
}

export function TokenChartsGrid<R>({
  rows,
  titleOf,
  chartTableId = 'token_charts_grid',
  highlightWallet,
  useRowOverlay,
  renderChartCardExtra,
  selectedKey,
  onSelect,
}: TokenChartsGridProps<R>) {
  const useOverlay = (useRowOverlay ?? useNoRowOverlay) as ChartOverlayHook<R>;
  if (rows.length === 0) return null;
  return (
    <div className="mt-4 flex flex-col gap-4">
      {rows.map((row) => {
        const mint = (row as { mint_address?: string }).mint_address ?? '';
        const selected = !!mint && selectedKey === mint;
        return (
          <LazyMount key={mint}>
            <TokenChartCard
              row={row}
              mint_address={mint}
              title={titleOf?.(row)}
              highlightWallet={highlightWallet}
              chartTableId={chartTableId}
              useOverlay={useOverlay}
              extra={renderChartCardExtra?.(row)}
              selected={selected}
              onSelect={
                onSelect && mint
                  ? () => onSelect(selected ? null : mint)
                  : undefined
              }
            />
          </LazyMount>
        );
      })}
    </div>
  );
}
