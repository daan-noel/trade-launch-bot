import { useEffect, useRef, useState, type ReactNode } from 'react';
import { NO_FLOW_PATTERN_SOURCE, type FlowPatternSource } from 'hooks/useFlowPatternKeys';
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

/** Hook-based overlay for {@link TokenChartsGridProps.groupByMint} — called once
 *  per mint card (rules-of-hooks safe, same contract as {@link ChartOverlayHook}).
 *  Use when a mint's full episode set can't be derived from the rows the grid was
 *  handed (which are only the table's CURRENT PAGE) — e.g. a server-paged table
 *  where a rule's re-entries land on different pages — so the hook can fetch the
 *  mint's full set itself. `rows` is that mint's rows from the current page, handed
 *  over as an immediate seed/fallback while the hook's own fetch is in flight. */
export type MintGroupOverlayHook<R> = (mint: string, rows: R[]) => RowChartOverlay;

/**
 * Resolve ONE row's saved `volume_ix_patterns` keys, for a grid whose rows span
 * different fingerprints (Console History lists positions from many rules), where
 * a single grid-wide set would misclassify most cards. Called once per card as a
 * **hook** — same rules-of-hooks contract as {@link ChartOverlayHook}. Under
 * `groupByMint` it receives the group's representative row.
 *
 * What it returns IS what the card classifies with — the fingerprint's saved
 * patterns, the same set the engine decides on. Nothing layers over it.
 *
 * It returns the fingerprint id ALONGSIDE the keys because a card's trades table
 * can edit those patterns, and the write needs the row they came from — keys alone
 * cannot be traced back to one (see `hooks/useFlowPatternKeys`).
 */
export type FlowPatternSourceHook<R> = (row: R, mint: string) => FlowPatternSource;

const NO_OVERLAY: RowChartOverlay = {};
/** Stable default so cards always call a hook (rules-of-hooks) when none passed. */
const useNoRowOverlay: ChartOverlayHook<unknown> = () => NO_OVERLAY;
const useNoRowFlowSource: FlowPatternSourceHook<unknown> = () => NO_FLOW_PATTERN_SOURCE;

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
function LazyMount({ minHeight = 380, children }: { minHeight?: number; children: ReactNode }) {
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
  /** Precomputed markers to draw instead of `useOverlay`'s result — used by the
   *  group-by-mint path to overlay a token's whole re-entry episode set on one
   *  card. `undefined` ⇒ use the hook; `null`/array ⇒ override. */
  eventMarkersOverride?: ChartEventMarker[] | null;
  flowPatternKeys?: ReadonlySet<string> | null;
  /** Grid-wide fingerprint for {@link flowPatternKeys} — the Vol-badge write
   *  target when every card shares one (see `BarTradesPanel`). */
  flowFingerprintId?: string | null;
  /** A stored run's frozen patterns — display only (see `BarTradesPanel`). */
  flowReadOnly?: boolean;
  /** Resolves this card's own pattern source (called as a hook); wins over the
   *  grid-wide props whenever it resolves a fingerprint or a set. */
  useFlowSource: FlowPatternSourceHook<R>;
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
  eventMarkersOverride,
  flowPatternKeys,
  flowFingerprintId,
  flowReadOnly,
  useFlowSource,
  extra,
  selected,
  onSelect,
}: TokenChartCardProps<R>) {
  const { data: detail } = useGetTokenDetailQuery(mint, { skip: !mint });
  const heading = title ?? detail?.symbol ?? detail?.name ?? mint.slice(0, 6);
  // Always call the overlay hook (rules of hooks); the override wins when present.
  const { eventMarkers: hookMarkers } = useOverlay(row, mint);
  const eventMarkers = eventMarkersOverride !== undefined ? eventMarkersOverride : hookMarkers;
  // Per-row source wins; the grid-wide props are the fallback for a uniform
  // cohort. Keys and id fall back independently: a row hook that resolves the
  // fingerprint but finds no patterns on it still owns this card's write target.
  const rowFlowSource = useFlowSource(row, mint);
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
          flowPatternKeys={rowFlowSource.keys ?? flowPatternKeys}
          flowFingerprintId={rowFlowSource.fingerprintId ?? flowFingerprintId}
          flowReadOnly={flowReadOnly}
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
  /** Extra header content per card (e.g. position PnL / hold). When
   *  `groupByMint` is on, the second arg is every row in that mint group so the
   *  caller can fold re-entry episodes; otherwise omitted. */
  renderChartCardExtra?: (row: R, groupRows?: readonly R[]) => ReactNode;
  /** Highlight the card whose key matches (same key as the table's `selectedKey`). */
  selectedKey?: string | null;
  /** Header click on a card — same contract as the table row select (toggle). */
  onSelect?: (key: string | null) => void;
  /** Unique per-row identity for the chart cards — defaults to the row's
   *  `mint_address`. Pass when a row set can repeat the same mint (e.g. a rule
   *  or sim/sweep run that re-enters the same token across episodes) so each
   *  episode gets its own card instead of colliding under one React key. Also
   *  becomes the `selectedKey`/`onSelect` identity, matching the table's own
   *  `rowKey`. Ignored when `groupByMint` is on (cards are keyed by mint). */
  rowKey?: (row: R) => string;
  /** Collapse the grid to ONE card per mint (not per row) and draw the markers
   *  {@link mintGroupOverlay} (or {@link useMintGroupOverlay}) builds from all rows
   *  sharing that mint — so a token a rule re-entered N times shows all N episodes
   *  overlaid on a single chart, matching the inspect modal. The card's selection
   *  key is the mint. The rows handed to either overlay builder are only the rows
   *  on the current page (the grid itself is page-scoped) — a table whose re-entry
   *  episodes can land on different pages needs {@link useMintGroupOverlay}, which
   *  can fetch/derive the mint's full set beyond just this page. */
  groupByMint?: boolean;
  /** Builds one card's overlay from every row of its mint that's on the CURRENT
   *  PAGE (pure — no hooks). Fine when a mint's episodes are guaranteed to co-occur
   *  on one page (e.g. a client table already holding its full filtered dataset);
   *  otherwise use {@link useMintGroupOverlay}. Ignored when that hook is supplied. */
  mintGroupOverlay?: (rows: R[], mint: string) => RowChartOverlay;
  /** Hook-based alternative to {@link mintGroupOverlay} — see
   *  {@link MintGroupOverlayHook}. Takes precedence when both are supplied. */
  useMintGroupOverlay?: MintGroupOverlayHook<R>;
  /** Fingerprint volume_ix_patterns keys for the vol/non-vol overlay on every card.
   *  Use when one set is right for the whole grid (a rule-scoped cohort); when the
   *  rows span fingerprints, pass {@link useRowFlowPatternKeys} instead. */
  flowPatternKeys?: ReadonlySet<string> | null;
  /** Fingerprint {@link flowPatternKeys} came from — the Vol-badge write target
   *  for every card (see `BarTradesPanel`). */
  flowFingerprintId?: string | null;
  /** A stored run's frozen patterns — display only (see `BarTradesPanel`). */
  flowReadOnly?: boolean;
  /** Per-card pattern source for a grid whose rows span fingerprints — see
   *  {@link FlowPatternSourceHook}. Wins over the grid-wide props per card. */
  useRowFlowPatternSource?: FlowPatternSourceHook<R>;
}

const mintOfRow = <R,>(row: R): string =>
  (row as { mint_address?: string }).mint_address ?? '';

export function TokenChartsGrid<R>({
  rows,
  titleOf,
  chartTableId = 'token_charts_grid',
  highlightWallet,
  useRowOverlay,
  renderChartCardExtra,
  selectedKey,
  onSelect,
  rowKey,
  groupByMint,
  mintGroupOverlay,
  useMintGroupOverlay,
  flowPatternKeys,
  flowFingerprintId,
  flowReadOnly,
  useRowFlowPatternSource,
}: TokenChartsGridProps<R>) {
  const useOverlay = (useRowOverlay ?? useNoRowOverlay) as ChartOverlayHook<R>;
  const useFlowSource = (useRowFlowPatternSource ?? useNoRowFlowSource) as FlowPatternSourceHook<R>;
  if (rows.length === 0) return null;

  // Group-by-mint: one card per token, its markers built from ALL of that mint's
  // episodes (re-entries overlaid on one chart). Falls through to the per-row path
  // when no group overlay was supplied.
  if (groupByMint && (mintGroupOverlay || useMintGroupOverlay)) {
    const order: string[] = [];
    const byMint = new Map<string, R[]>();
    for (const row of rows) {
      const mint = mintOfRow(row);
      const bucket = byMint.get(mint);
      if (bucket) {
        bucket.push(row);
      } else {
        byMint.set(mint, [row]);
        order.push(mint);
      }
    }
    return (
      <div className="mt-4 flex flex-col gap-4">
        {order.map((mint) => {
          const groupRows = byMint.get(mint)!;
          const rep = groupRows[0];
          const selected = !!mint && selectedKey === mint;
          // Hook path (can fetch beyond this page) wins over the pure/page-scoped
          // builder; either way `useOverlay` below is still always called (rules
          // of hooks) — it just ignores its (row, mint) args in the hook path.
          const overlayHook = (
            useMintGroupOverlay
              ? () => useMintGroupOverlay(mint, groupRows)
              : useNoRowOverlay
          ) as ChartOverlayHook<R>;
          const eventMarkersOverride = useMintGroupOverlay
            ? undefined
            : (mintGroupOverlay!(groupRows, mint).eventMarkers ?? null);
          return (
            <LazyMount key={mint}>
              <TokenChartCard
                row={rep}
                mint_address={mint}
                title={titleOf?.(rep)}
                highlightWallet={highlightWallet}
                chartTableId={chartTableId}
                useOverlay={overlayHook}
                eventMarkersOverride={eventMarkersOverride}
                flowPatternKeys={flowPatternKeys}
                flowFingerprintId={flowFingerprintId}
                flowReadOnly={flowReadOnly}
                useFlowSource={useFlowSource}
                extra={renderChartCardExtra?.(rep, groupRows)}
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

  return (
    <div className="mt-4 flex flex-col gap-4">
      {rows.map((row) => {
        const mint = mintOfRow(row);
        const key = rowKey ? rowKey(row) : mint;
        const selected = !!key && selectedKey === key;
        return (
          <LazyMount key={key}>
            <TokenChartCard
              row={row}
              mint_address={mint}
              title={titleOf?.(row)}
              highlightWallet={highlightWallet}
              chartTableId={chartTableId}
              useOverlay={useOverlay}
              flowPatternKeys={flowPatternKeys}
              flowFingerprintId={flowFingerprintId}
              flowReadOnly={flowReadOnly}
              useFlowSource={useFlowSource}
              extra={renderChartCardExtra?.(row)}
              selected={selected}
              onSelect={
                onSelect && key
                  ? () => onSelect(selected ? null : key)
                  : undefined
              }
            />
          </LazyMount>
        );
      })}
    </div>
  );
}
