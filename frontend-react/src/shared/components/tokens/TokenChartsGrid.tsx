import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useGetTokenDetailQuery } from 'store/sharedEndpoints';
import { TokenTradeChart } from 'components/tokens/TokenTradeChart';
import { AddressDisplay } from 'components/ui/AddressDisplay';

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

interface TokenChartCardProps {
  mint: string;
  /** Header title; falls back to the fetched detail's symbol/name, then the mint. */
  title?: string;
  highlightWallet?: string | null;
  chartTableId: string;
  /** Extra content rendered in the card header (per-row context). */
  extra?: ReactNode;
}

function TokenChartCard({ mint, title, highlightWallet, chartTableId, extra }: TokenChartCardProps) {
  const { data: detail } = useGetTokenDetailQuery(mint, { skip: !mint });
  const heading = title ?? detail?.symbol ?? detail?.name ?? mint.slice(0, 6);

  return (
    <div className="rounded-lg border border-white/8 bg-bg-card/40 p-4">
      <div className="mb-2 flex flex-wrap items-center gap-2">
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
      <TokenTradeChart
        key={mint}
        detail={detail ?? null}
        highlightWallet={highlightWallet ?? null}
        tableId={chartTableId}
      />
    </div>
  );
}

export interface TokenChartsGridProps<R> {
  /** The rows to chart — the table's CURRENT on-screen page, never the full set. */
  rows: R[];
  /** Extract a row's mint (rows may key it `mint` or `mint_address`). */
  mintOf: (row: R) => string;
  /** Optional per-row header title (else derived from the fetched detail). */
  titleOf?: (row: R) => string;
  /** Chart-local prefs id (column visibility of the per-chart trades table). */
  chartTableId?: string;
  /** Wallet to spotlight on every chart (Trader Analysis). */
  highlightWallet?: string | null;
  /** Extra header content per card (e.g. per-wallet buys/sells). */
  renderChartCardExtra?: (row: R) => ReactNode;
}

export function TokenChartsGrid<R>({
  rows,
  mintOf,
  titleOf,
  chartTableId = 'token_charts_grid',
  highlightWallet,
  renderChartCardExtra,
}: TokenChartsGridProps<R>) {
  if (rows.length === 0) return null;
  return (
    <div className="mt-4 flex flex-col gap-4">
      {rows.map((row) => {
        const mint = mintOf(row);
        return (
          <LazyMount key={mint}>
            <TokenChartCard
              mint={mint}
              title={titleOf?.(row)}
              highlightWallet={highlightWallet}
              chartTableId={chartTableId}
              extra={renderChartCardExtra?.(row)}
            />
          </LazyMount>
        );
      })}
    </div>
  );
}
