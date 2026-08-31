import { useMemo } from 'react';
import { DataTable } from 'components/table/DataTable';
import { tokenTradeColumns } from 'components/tokens/tokenTradeColumns';
import { IxPatternBar } from 'components/tokens/IxPatternBar';
import { Badge } from 'components/ui/Badge';
import { useTimezone } from 'context/TimezoneContext';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { useIxPatternTarget } from 'hooks/useIxPatternTarget';
import { useFlowLensContext, type FlowLensTarget } from 'context/FlowLensContext';
import { formatTimestampMs } from 'utils/date';
import type { FlowReason } from 'lib/flow/classifyFlow';
import type {
  ChartBarSelection,
  ChartEventMarker,
  ChartRangeSelectionDetail,
} from 'components/token-price-chart/types';
import { ixLabelsActions } from 'lib/ixLabels';
import { CHART_COLORS } from 'components/token-price-chart/constants';
import type { LensMatch } from 'components/token-price-chart/lensTint';
import type { TokenHighlight } from 'components/tokens/useTokenHighlight';
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
  /** The host's saved `ix_patterns` keys — adds the Tagged/Untagged column.
   *  This is the set the chart lines, the metric panes and the engine all use, so
   *  the badge and the overlay can never report different classifications. */
  flowPatternKeys?: ReadonlySet<string> | null;
  /** The fingerprint {@link flowPatternKeys} came from — the row a Tagged-badge edit
   *  writes to. Pass it wherever the host knows one (`hooks/useFlowPatternKeys`
   *  resolves both together): without it the panel can only guess its write
   *  target from the pattern set, and an empty set matches every unconfigured
   *  fingerprint at once, which leaves the badge uneditable. */
  flowFingerprintId?: string | null;
  /** A stored run's frozen patterns — display only, never edited from here. */
  flowReadOnly?: boolean;
  /** Effective (contagion-aware) classification per trade id, from the host's
   *  FULL trade history — a bar's rows alone can't reconstruct contagion. Omit
   *  and the badge reports structure only, as it always has. */
  flowReasons?: ReadonlyMap<string, FlowReason> | null;
  /** The token's two ephemeral highlight lenses (`useTokenHighlight`). Wires the
   *  per-row target buttons, paints matched rows, and renders the armed chips.
   *  Omit on a host that doesn't offer lenses. */
  highlight?: TokenHighlight | null;
  /** Outer spacing — override in a host that already spaces its children (e.g.
   *  a `flex flex-col gap-*`, where the default top margin double-spaces). */
  className?: string;
}

/** Maps tx signature → kind for entry/exit row highlighting. Every inspect source
 *  carries the real fill signature: position/sim results read it off the position,
 *  and the grouped-sweep drill-in resolves it from `trades` by (mint, slot, side). */
function buildEntryExitMap(
  markers: ChartEventMarker[] | null | undefined,
): Map<string, 'entry' | 'exit'> {
  const m = new Map<string, 'entry' | 'exit'>();
  if (!markers) return m;
  for (const marker of markers) {
    if (marker.txSignature) m.set(marker.txSignature, marker.kind);
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
  flowReasons = null,
  highlight = null,
  className = 'mt-3 border-t border-white/7 pt-2',
}: BarTradesPanelProps) {
  const { timezone } = useTimezone();
  const price = usePriceDisplay();

  // A page-wide flow lens (Trader Analysis) OWNS the pattern set on pages with no
  // fingerprint behind their tokens. When one is provided it is the write target:
  // clicks land in `ix_pattern_sets`, never on a fingerprint, so nothing a study
  // toggles can change how a live rule classifies flow.
  const lens = useFlowLensContext();
  const lensTarget = flowReadOnly ? null : (lens?.target ?? null);

  // Without a lens, a toggle edits the fingerprint's saved patterns directly, so
  // the badge, the chart lines and the engine are always reading the same row.
  const patternTarget = useIxPatternTarget({
    fingerprintId: flowFingerprintId,
    savedKeys: flowPatternKeys,
    enabled: !flowReadOnly && !lensTarget,
  });
  const onTogglePattern = flowReadOnly
    ? null
    : (lensTarget?.toggle ?? patternTarget.toggle);
  // The badge reports what the CHART classified with. Under a lens that is the
  // narrowed key set the page handed down (group filters applied); a toggle still
  // edits the whole stored set, filing new patterns under the lens' active group.
  const badgeKeys = lensTarget ? (flowPatternKeys ?? null) : patternTarget.keys;
  const toggleTargetName = lensTarget?.name ?? patternTarget.target?.name ?? null;
  const feePinMask = lensTarget ? null : patternTarget.feePins;
  const patternRows = lensTarget ? null : patternTarget.rows;

  // Pulled apart rather than passed as one object: `useTokenHighlight` returns a
  // fresh literal every render, and depending on it would rebuild every column —
  // and re-render the whole table — on each one. The four pieces are stable
  // (two `useCallback`s and two strings).
  const onLensWallet = highlight?.toggleWallet ?? null;
  const onLensStructure = highlight?.toggleStructure ?? null;
  const armedLensWallet = highlight?.lens.wallet ?? null;
  const armedLensStructureKey = highlight?.lens.structureKey ?? null;

  const columns = useMemo(
    () =>
      tokenTradeColumns(price.unitLabel, {
        onLensWallet,
        lensWallet: armedLensWallet,
        onLensStructure,
        lensStructureKey: armedLensStructureKey,
        // The target's keys, not the prop: a badge must report the row its own
        // click writes to. They are the same set in the normal case, and differ
        // only when the reader deliberately picks another fingerprint — which
        // `IxPatternBar` flags rather than letting the two drift silently.
        flowPatternKeys: badgeKeys,
        onTogglePattern,
        toggleTargetName,
        flowReasons,
        // A lens only ever edits the tagged set, so the column follows the target's
        // list exclusively on the fingerprint path.
        patternList: lensTarget ? 'tagged' : patternTarget.list,
        otherListKeys: lensTarget ? null : patternTarget.otherKeys,
        feePinMask,
        patternRows,
      }),
    [
      price.unitLabel,
      badgeKeys,
      toggleTargetName,
      onTogglePattern,
      flowReasons,
      lensTarget,
      patternTarget.list,
      patternTarget.otherKeys,
      feePinMask,
      patternRows,
      onLensWallet,
      onLensStructure,
      armedLensWallet,
      armedLensStructureKey,
    ],
  );

  const entryExitMap = useMemo(() => buildEntryExitMap(eventMarkers), [eventMarkers]);

  const focusAddr = highlightWallet?.trim() || null;
  const focusCount = useMemo(
    () => (focusAddr ? trades.filter((t) => t.wallet_address === focusAddr).length : 0),
    [trades, focusAddr],
  );

  // The wallet LENS and the page's focused wallet are the same signal — "this is
  // the trader you are looking at" — so they share the gold, exactly as they share
  // it on the chart. The structure lens is a different question and takes its own
  // edge; the row background is already spoken for three ways over.
  const lensWalletAddr = armedLensWallet;
  const isStructureMatch = highlight?.isStructureMatch ?? null;
  const structureArmed = armedLensStructureKey != null;

  const rowClassName = useMemo(() => {
    const mineCount = myWalletAddresses?.size ?? 0;
    if (
      entryExitMap.size === 0 &&
      mineCount === 0 &&
      !focusAddr &&
      !lensWalletAddr &&
      !structureArmed
    ) {
      return undefined;
    }
    return (t: TradeRecord) => {
      const kind = entryExitMap.get(t.tx_signature);
      // The focused wallet outranks the entry/exit tint for the row background —
      // finding HIM is the whole reason the page is open, and the fill's own
      // direction is still on the row in its side column.
      const focused =
        (!!focusAddr && t.wallet_address === focusAddr) ||
        (!!lensWalletAddr && t.wallet_address === lensWalletAddr);
      const base = focused
        ? 'bg-[#fde047]/16 hover:bg-[#fde047]/24 font-semibold'
        : kind === 'entry'
          ? 'bg-[#02c076]/12 hover:bg-[#02c076]/20'
          : kind === 'exit'
            ? 'bg-[#f6465d]/12 hover:bg-[#f6465d]/20'
            : '';
      // Left accent: focus (gold, 4px) beats "my trade" (amber, 2px) — a wallet
      // can be both, and only one border fits.
      const accent = focused
        ? 'border-l-4 border-l-[#fde047]'
        : t.wallet_address && myWalletAddresses?.has(t.wallet_address)
          ? 'border-l-2 border-l-[#fbbf24]'
          : '';
      // Right edge, not the background: a row can be the focused wallet AND carry
      // the armed structure, and that overlap is the cell worth spotting.
      const structure =
        structureArmed && isStructureMatch?.(t) ? 'border-r-2 border-r-[#22d3ee]' : '';
      return [base, accent, structure].filter(Boolean).join(' ') || undefined;
    };
  }, [
    entryExitMap,
    myWalletAddresses,
    focusAddr,
    lensWalletAddr,
    structureArmed,
    isStructureMatch,
  ]);

  const lensActive = highlight?.active ?? false;
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
          <FlowLensStrip target={lensTarget} patternCount={badgeKeys?.size ?? 0} />
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
        emptyMessage={emptyMessage}
      />
    </div>
  );
}


/** Short address for a chip — the address itself is a column away. */
function shortAddr(addr: string): string {
  return addr.length > 12 ? `${addr.slice(0, 4)}…${addr.slice(-4)}` : addr;
}

/** `+4.21` / `-0.08` — sign always shown, because the sign is the point. */
function signedSol(match: LensMatch): string {
  const net = match.buySol - match.sellSol;
  return `${net >= 0 ? '+' : '−'}${Math.abs(net).toFixed(3)}`;
}

/**
 * One armed lens, stated in full: what is washed, how much of the token it is,
 * and the button that turns it off.
 *
 * The counts come from the CHART's own match (`onHighlightLensMatch`), not from a
 * second pass over the rows — a chip that quoted a different number from the wash
 * beside it would make the reader distrust both.
 */
function LensChip({
  color,
  label,
  title,
  match,
  note,
  onClear,
}: {
  color: string;
  label: string;
  title: string;
  match: LensMatch;
  note?: string | null;
  onClear: () => void;
}) {
  const total = match.buys + match.sells;
  return (
    <span
      className="inline-flex max-w-full items-center gap-1.5 rounded border px-1.5 py-px font-mono text-[10px]"
      style={{ borderColor: `${color}99`, backgroundColor: `${color}1f`, color }}
      title={title}
    >
      <span className="truncate">{label}</span>
      <span className="text-text-dim">
        {total} tx ({match.buys}b/{match.sells}s) · net {signedSol(match)} SOL
      </span>
      {note && <span className="text-text-dim">· {note}</span>}
      <button
        type="button"
        onClick={onClear}
        title="Stop highlighting"
        className="leading-none opacity-70 hover:opacity-100"
      >
        ×
      </button>
    </span>
  );
}

/**
 * The armed highlight lenses for this token, shown wherever the trades panel is —
 * including with no candle selected, so the control that disarms a lens never
 * hides behind the table it is washing.
 *
 * Counts are bar-aligned: they cover exactly the trades the chart could paint, so
 * dust legs the candles drop are absent here too.
 */
function LensChips({ highlight }: { highlight: TokenHighlight }) {
  const { lens, matches, structureLabels, unlabeled, toggleWallet, toggleStructure } =
    highlight;
  const structureText = structureLabels ? ixLabelsActions([...structureLabels]) : '';
  return (
    <div className="mb-2 flex flex-wrap items-center gap-2">
      {lens.wallet && (
        <LensChip
          color={CHART_COLORS.lensWallet}
          label={shortAddr(lens.wallet)}
          title={`${lens.wallet} — every candle this wallet traded in is washed gold`}
          match={matches.wallet}
          onClear={() => toggleWallet(null)}
        />
      )}
      {lens.structureKey && (
        <LensChip
          color={CHART_COLORS.lensStructure}
          label={structureText || 'ix structure'}
          title={
            `${structureText}

Every candle carrying this EXACT ordered structure is ` +
            `washed cyan. View-only — no fingerprint or rule reads it.`
          }
          match={matches.structure}
          note={unlabeled > 0 ? `${unlabeled} unlabeled` : null}
          onClear={() => toggleStructure(null)}
        />
      )}
    </div>
  );
}

/**
 * The lens twin of {@link IxPatternBar}: what a Tagged-badge click writes to
 * when the page owns the pattern set instead of a fingerprint. No picker — the
 * set is chosen on the page, above every chart — and no active-rule warning,
 * because a lens is analysis-only and no rule can be bound to it.
 */
function FlowLensStrip({
  target,
  patternCount,
}: {
  target: FlowLensTarget;
  /** Patterns actually classifying right now — the narrowed set, which is what
   *  the badges below report against. */
  patternCount: number;
}) {
  const total = target.patterns.length;
  return (
    <span className="inline-flex flex-wrap items-center gap-2">
      <Badge variant="info" size="sm">
        Flow lens
      </Badge>
      <span className="font-mono text-[11px] text-text">{target.name}</span>
      <span
        className="font-mono text-[11px] text-text-dim"
        title="Patterns classifying this chart / patterns in the whole set"
      >
        {patternCount}/{total} pattern{total === 1 ? '' : 's'}
      </span>
      <span className="text-[11px] text-text-dim">
        Tagged badges add/remove here
        {target.activeGroup ? ` (group "${target.activeGroup}")` : ''} — analysis only, no
        rule reads it.
      </span>
      {target.saving && <span className="text-[11px] text-text-dim">Saving…</span>}
      {target.error && <span className="text-[11px] text-red">{target.error}</span>}
    </span>
  );
}
