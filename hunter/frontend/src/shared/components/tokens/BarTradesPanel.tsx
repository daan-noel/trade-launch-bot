import { useMemo, type CSSProperties } from 'react';
import { DataTable } from 'components/table/DataTable';
import { tokenTradeColumns } from 'components/tokens/tokenTradeColumns';
import { IxPatternBar, TagStageControls } from 'components/tokens/IxPatternBar';
import { Badge } from 'components/ui/Badge';
import { useTimezone } from 'context/TimezoneContext';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import {
  useIxPatternTarget,
  type IxPatternTarget,
  type TagStage,
  type TagTape,
} from 'hooks/useIxPatternTarget';
import { useFlowLensContext, type FlowLensTarget } from 'context/FlowLensContext';
import { formatTimestampMs } from 'utils/date';
import type { FlowReason } from 'lib/flow/classifyFlow';
import { tagLabel } from 'lib/flow/tapeClassify';
import { tagField, useStrategyRegistry } from 'lib/strategy/registry';
import type {
  ChartBarSelection,
  ChartEventMarker,
  ChartRangeSelectionDetail,
} from 'components/token-price-chart/types';
import type { TokenHighlight } from 'components/tokens/useTokenHighlight';
import { LensChips } from 'components/tokens/LensControls';
import type { TradeRecord } from 'types';

const EMPTY_TRADES: TradeRecord[] = [];
const tradeRowKey = (t: TradeRecord) => t.id;

/** A selection that lives outside the chart's own bar/range click (e.g. a swing
 *  leg picked in a separate results table) but drives this panel the same way.
 *  Takes over the heading/empty text; the host is responsible for handing the
 *  chart's own selection back (see `useBarTradesSelection`'s `onPick`). */
export interface BarTradesExternalSelection {
  /** Panel heading, e.g. "Swing trades". */
  label: string;
  /** Rendered next to the heading, e.g. a formatted time range. */
  timeLabel: string;
  emptyMessage: string;
}

export interface BarTradesPanelProps {
  /** Rows to list — already narrowed to the selection by the host. */
  trades: TradeRecord[];
  bar: ChartBarSelection | null;
  range: ChartRangeSelectionDetail | null;
  /** When set, overrides the labels and renders even with no chart selection. */
  external?: BarTradesExternalSelection | null;
  onClear: () => void;
  /** Passed to DataTable so column visibility persists per call-site. */
  tableId?: string;
  /** Entry/exit markers — tints the matching fill rows. */
  eventMarkers?: ChartEventMarker[] | null;
  /** Our own wallets — adds a left-border accent to their rows. */
  myWalletAddresses?: ReadonlySet<string> | null;
  /** The focused/input wallet (Trader Analysis). Its rows are painted gold — the
   *  same signal the chart's oversized gold marker carries — and counted in the
   *  heading, so the wallet under study is findable without reading addresses. */
  highlightWallet?: string | null;
  /** The host's key set (its default tag's exact ix shapes) - the fallback the
   *  panel classifies with and matches a write target by when it has no
   *  fingerprint id (see `hooks/useFlowPatternKeys`). */
  flowPatternKeys?: ReadonlySet<string> | null;
  /** The host's fingerprint - the row an "add to tag" click writes to. Pass it
   *  wherever the host knows one: without it the panel can only guess its target. */
  flowFingerprintId?: string | null;
  /** A stored run's frozen shapes - display only, never edited from here. */
  flowReadOnly?: boolean;
  /** Host-owned target (TokenTradeChart / Floor). When passed, this panel does not
   *  create its own `useIxPatternTarget`, so the lines above and the badges here
   *  cannot disagree about which tag is picked. */
  patternTarget?: IxPatternTarget | null;
  /** A staging tape (Flow Discovery): badges write the draft, not a fingerprint. */
  tape?: TagTape | null;
  /** The verdict per trade id over the host's FULL trade history
   *  (`tradeFlowReasons`) - sticky, cluster and the creation slot are forward-only,
   *  so a bar's rows alone cannot reconstruct them. */
  flowReasons?: ReadonlyMap<string, FlowReason> | null;
  /** The token's two ephemeral highlight lenses (`useTokenHighlight`). Wires the
   *  per-row target buttons, paints matched rows, and renders the armed chips.
   *  Omit on a host that doesn't offer lenses. */
  highlight?: TokenHighlight | null;
  /** Outer spacing — override in a host that already spaces its children (e.g.
   *  a `flex flex-col gap-*`, where the default top margin double-spaces). */
  className?: string;
}

export type RowEventKind = 'entry' | 'exit' | 'signal';

/** Maps tx signature → kind for entry/exit/signal row highlighting. Every inspect
 *  source carries the signature of the print each fill sits on: a real position's
 *  own transaction, the print a paper fill or sim result was priced against, or
 *  (grouped-sweep drill-in) one resolved from `trades` by (mint, slot, side).
 *  A signal marker is the trigger print, not a fill, so it keeps its own tint, and
 *  a fill on the same print outranks it. */
export function buildEntryExitMap(
  markers: ChartEventMarker[] | null | undefined,
): Map<string, RowEventKind> {
  const m = new Map<string, RowEventKind>();
  if (!markers) return m;
  for (const marker of markers) {
    if (!marker.txSignature) continue;
    if (marker.role === 'signal') {
      if (!m.has(marker.txSignature)) m.set(marker.txSignature, 'signal');
    } else {
      m.set(marker.txSignature, marker.kind);
    }
  }
  return m;
}

/**
 * The trades table under a price chart: what a clicked candle / dragged range is
 * actually made of. The ONE panel for that — Token detail, Console/Portfolio
 * position detail and the flow preview all render this, so a column or a row
 * highlight added here shows up on every chart instead of one of them.
 *
 * Renders nothing when nothing is selected, so a host can mount it
 * unconditionally.
 */
export function BarTradesPanel({
  trades,
  bar,
  range,
  external = null,
  onClear,
  tableId,
  eventMarkers = null,
  myWalletAddresses = null,
  highlightWallet = null,
  flowPatternKeys = null,
  flowFingerprintId = null,
  flowReadOnly = false,
  patternTarget: patternTargetProp = null,
  tape = null,
  flowReasons = null,
  highlight = null,
  className = 'mt-3 border-t border-white/7 pt-2',
}: BarTradesPanelProps) {
  const { timezone } = useTimezone();
  const price = usePriceDisplay();

  // A page-wide flow lens (Trader Analysis) OWNS the set on pages with no
  // fingerprint behind their tokens. When one is provided it is the write target:
  // clicks land in `ix_pattern_sets`, never on a fingerprint, so nothing a study
  // toggles can change what a live rule reads.
  const lens = useFlowLensContext();
  const lensTarget = flowReadOnly ? null : (lens?.target ?? null);

  // Without a lens or a tape, a click edits the fingerprint's tag directly, so the
  // badge, the chart lines and the engine always read the same row. A host that
  // already owns the target passes it in; this hook then no-ops.
  const createdTarget = useIxPatternTarget({
    fingerprintId: flowFingerprintId,
    savedKeys: flowPatternKeys,
    enabled: !flowReadOnly && !lensTarget && !tape && !patternTargetProp,
  });
  const patternTarget = patternTargetProp ?? createdTarget;
  const stage: TagStage | null = flowReadOnly ? null : (lensTarget ?? tape ?? patternTarget);
  // The tag the reasons were computed against - the lines above read the same one.
  // A stage with nothing classifying yet still names its tag, so the first click
  // that fills it has a column to land in.
  const flowTagName = lens
    ? (lens.tag?.name ?? lensTarget?.tagName ?? null)
    : tape
      ? tape.tagName
      : (patternTarget.tag?.name ?? (patternTarget.toggle ? patternTarget.tagName : null));
  const { data: reg } = useStrategyRegistry();

  // Pulled apart rather than passed as one object: `useTokenHighlight` returns a
  // fresh literal every render, and depending on it would rebuild every column —
  // and re-render the whole table — on each one. The pieces are `useCallback`s
  // that change only when the armed set does.
  const onLensWallet = highlight?.toggleWallet ?? null;
  const onLensStructure = highlight?.toggleStructure ?? null;
  const lensWalletColor = highlight?.walletColor ?? null;
  const lensStructureColor = highlight?.structureColor ?? null;
  const lensColorsOf = highlight?.colorsOf ?? null;
  const lensActive = highlight?.active ?? false;

  const tagFieldTitle = useMemo(
    () => (key: string) => tagField(reg, key)?.title ?? key,
    [reg],
  );
  const columns = useMemo(
    () =>
      tokenTradeColumns(price.unitLabel, {
        onLensWallet,
        lensWalletColor,
        onLensStructure,
        lensStructureColor,
        flowTagName,
        flowReasons,
        stage,
        tagFieldTitle,
      }),
    [
      price.unitLabel,
      flowTagName,
      flowReasons,
      stage,
      tagFieldTitle,
      onLensWallet,
      onLensStructure,
      lensWalletColor,
      lensStructureColor,
    ],
  );

  const entryExitMap = useMemo(() => buildEntryExitMap(eventMarkers), [eventMarkers]);

  const focusAddr = highlightWallet?.trim() || null;
  const focusCount = useMemo(
    () => (focusAddr ? trades.filter((t) => t.wallet_address === focusAddr).length : 0),
    [trades, focusAddr],
  );

  // A row an armed lens item matches takes that item's color (the first one, in
  // arming order, when it matches several) — the same color as its wash on the
  // chart. The color is runtime, so it rides a CSS variable the classes read.
  const rowStyle = useMemo(() => {
    if (!lensActive || !lensColorsOf) return undefined;
    return (t: TradeRecord): CSSProperties | undefined => {
      const colors = lensColorsOf(t);
      return colors.length > 0 ? ({ '--row-lens': colors[0] } as CSSProperties) : undefined;
    };
  }, [lensActive, lensColorsOf]);

  const rowClassName = useMemo(() => {
    const mineCount = myWalletAddresses?.size ?? 0;
    if (entryExitMap.size === 0 && mineCount === 0 && !focusAddr && !lensActive) {
      return undefined;
    }
    return (t: TradeRecord) => {
      const kind = entryExitMap.get(t.tx_signature);
      // The focused wallet outranks the entry/exit tint for the row background —
      // finding HIM is the whole reason the page is open, and the fill's own
      // direction is still on the row in its side column. A lens match is the same
      // kind of signal ("the thing I picked"), in its own color.
      const focused = !!focusAddr && t.wallet_address === focusAddr;
      const lensed = !focused && lensActive && !!lensColorsOf && lensColorsOf(t).length > 0;
      // Tints are the chart's marker colors (`CHART_COLORS.entry` / `.exit` /
      // `.signalEntry`), so a row and the marker above it read as one event.
      const base = focused
        ? 'bg-[#fde047]/16 hover:bg-[#fde047]/24 font-semibold'
        : lensed
          ? 'bg-(--row-lens)/16 hover:bg-(--row-lens)/24 font-semibold'
          : kind === 'entry'
            ? 'bg-[#02c076]/12 hover:bg-[#02c076]/20'
            : kind === 'exit'
              ? 'bg-[#f6465d]/12 hover:bg-[#f6465d]/20'
              : kind === 'signal'
                ? 'bg-[#5dade2]/12 hover:bg-[#5dade2]/20'
                : '';
      // Left accent: focus / lens (4px) beats "my trade" (amber, 2px) — a wallet
      // can be both, and only one border fits.
      const accent = focused
        ? 'border-l-4 border-l-[#fde047]'
        : lensed
          ? 'border-l-4 border-l-(--row-lens)'
          : t.wallet_address && myWalletAddresses?.has(t.wallet_address)
            ? 'border-l-2 border-l-[#fbbf24]'
            : '';
      return [base, accent].filter(Boolean).join(' ') || undefined;
    };
  }, [entryExitMap, myWalletAddresses, focusAddr, lensActive, lensColorsOf]);

  if (!external && !bar && !range) {
    return lensActive && highlight ? (
      <div className={className}>
        <LensChips highlight={highlight} />
      </div>
    ) : null;
  }

  const chartTimeLabel = range
    ? range.groupMode === 'slot'
      ? `Slot ${Math.min(range.lo, range.hi)} → ${Math.max(range.lo, range.hi)}`
      : `${formatTimestampMs(Math.min(range.lo, range.hi) * 1000, timezone)} → ${formatTimestampMs(Math.max(range.lo, range.hi) * 1000, timezone)}`
    : bar
      ? bar.groupMode === 'slot'
        ? `Slot ${bar.slot}`
        : formatTimestampMs(Number(bar.barTime) * 1000, timezone)
      : '';

  const label = external ? external.label : range ? 'Range Trades' : 'Bar Trades';
  const timeLabel = external ? external.timeLabel : chartTimeLabel;
  const emptyMessage = external
    ? external.emptyMessage
    : range
      ? 'No trades in this range.'
      : 'No trades in this bar.';
  const rows = trades.length > 0 ? trades : EMPTY_TRADES;

  return (
    <div className={className}>
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <span className="text-[9px] font-bold uppercase tracking-widest text-text-dim">
          {label}
        </span>
        <span className="font-mono text-[11px] text-text-dim">{timeLabel}</span>
        <Badge variant="primary" className="font-mono font-normal">
          {rows.length} trade{rows.length === 1 ? '' : 's'}
        </Badge>
        {focusAddr && (
          <span
            className="rounded border border-[#fde047]/60 bg-[#fde047]/16 px-1.5 py-px font-mono text-[10px] text-[#fde047]"
            title={`${focusAddr} — the wallet under analysis`}
          >
            {focusCount} of {rows.length} by {focusAddr.slice(0, 4)}…{focusAddr.slice(-4)}
          </span>
        )}
        <button
          type="button"
          onClick={onClear}
          className="text-[11px] text-text-dim hover:text-text"
        >
          Clear
        </button>
        {lensTarget ? (
          <FlowLensStrip target={lensTarget} tagName={flowTagName} />
        ) : tape ? (
          <span className="inline-flex flex-wrap items-center gap-2">
            <TagStageControls stage={tape} />
            <span className="text-[11px] text-text-dim">into the draft: Apply saves it</span>
          </span>
        ) : (
          <IxPatternBar target={patternTarget} readOnly={flowReadOnly} />
        )}
      </div>
      {highlight && lensActive && <LensChips highlight={highlight} />}
      <DataTable
        tableId={tableId}
        columns={columns}
        rows={rows}
        rowKey={tradeRowKey}
        searchable
        colFilters
        hoverable
        rowClassName={rowClassName}
        rowStyle={rowStyle}
        emptyMessage={emptyMessage}
        // `trades` is a fresh selection every time the host's chart click/drag
        // (or `external`) changes — outside this table's own state — so a new
        // selection must not inherit the previous one's page.
        resetKey={
          external
            ? `x:${external.label}|${external.timeLabel}`
            : range
              ? `r:${range.groupMode}:${range.lo}-${range.hi}`
              : bar
                ? `b:${bar.groupMode}:${bar.barTime}:${bar.slot ?? ''}`
                : 'none'
        }
      />
    </div>
  );
}


/**
 * The lens twin of {@link IxPatternBar}: what a badge click writes when the page owns
 * the set instead of a fingerprint. No picker - the set is chosen on the page, above
 * every chart - and no active-rule warning, because a lens is analysis only and no
 * rule can read it.
 */
function FlowLensStrip({
  target,
  tagName,
}: {
  target: FlowLensTarget;
  /** The lens tag's name, or `null` when the narrowing leaves nothing to classify. */
  tagName: string | null;
}) {
  const isTemplates = target.kind === 'templates';
  const total = isTemplates ? target.workingTemplates.length : target.patterns.length;
  return (
    <span className="inline-flex flex-wrap items-center gap-2">
      <Badge variant={isTemplates ? 'success' : 'info'} size="sm">
        Target IXs · {isTemplates ? 'Templates' : 'Exact'}
      </Badge>
      <span className="font-mono text-[11px] text-text-dim" title="entries in the whole stored set">
        {total} {isTemplates ? 'id' : 'shape'}
        {total === 1 ? '' : 's'}
      </span>
      {!tagName && <span className="text-[11px] text-text-dim">the narrowing leaves nothing to classify</span>}
      <TagStageControls stage={target} />
      <span className="text-[11px] text-text-dim">
        {target.activeGroup ? `filed under group "${target.activeGroup}"; ` : ''}analysis only, no rule reads{' '}
        {tagLabel(target.tagName)}.
      </span>
    </span>
  );
}
