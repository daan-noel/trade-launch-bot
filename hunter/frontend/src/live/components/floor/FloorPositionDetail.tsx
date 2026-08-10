import { useMemo, type ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { buildEventMarkers, type InspectTarget } from 'components/strategy/inspectTarget';
import type { ChartEventMarker } from 'components/token-price-chart';
import { exitReasonBadge } from 'components/strategy/strategyColumns';
import { ElapsedCell } from 'components/table/ElapsedCell';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { cn } from 'lib/cn';
import { rulesHref } from 'lib/strategy/nav';
import { formatCompact } from 'utils/format';
import {
  formatSigned,
  formatSignedPct,
  pctGradeClass,
  signedToneClass,
} from 'lib/signedTone';
import { MintBarTradesPanel } from 'components/tokens/MintBarTradesPanel';
import { useBarTradesSelection } from 'components/tokens/useBarTradesSelection';
import { CrosshairTimeProvider, useCrosshairTimeStore } from './crosshairTime';
import { LazyFloorMintChart } from './LazyFloorMintChart';
import { OpenPositionStatusChips } from './openPositionStatus';

export interface FloorDetailFacts {
  mint: string;
  /** Token symbol when known (History / Evidence); falls back to mint. */
  symbol?: string | null;
  ruleId: string | null;
  ruleName: string | null;
  mode?: string | null;
  /** `bot` | `manual` — colors the origin chip. */
  origin?: string | null;
  /** Display label (e.g. "Holding"). */
  status: string;
  /** Raw engine status key for badge color; defaults from `status` when omitted. */
  statusKey?: string | null;
  entrySol?: number | null;
  entryPrice?: number | null;
  /** Raw entry token units — for reconstructed ledger rows when `position_fills` is empty. */
  entryTokenAmount?: number | null;
  exitPrice?: number | null;
  /** Total exit SOL (scale-out aggregate / legacy single exit). */
  exitSol?: number | null;
  exitTokenAmount?: number | null;
  exitReason?: string | null;
  /** ISO or display age already formatted (closed / static). */
  holdLabel?: string | null;
  /** When set, hold ticks live via ElapsedCell (open / waiting). */
  holdStartMs?: number | null;
  pnlSol?: number | null;
  pnlPct?: number | null;
  mtmSol?: number | null;
  soldBps?: number;
  scaleStage?: number;
  exitParked?: boolean;
  exitRedriveCount?: number;
  needsReview?: boolean;
  isDead?: boolean;
  /** Open vs closed chart markers. */
  inspect: InspectTarget;
  /**
   * Markers for **every** episode on the mint, when the host can resolve them
   * (`useMintEpisodeMarkers`). Overrides the single-episode set built from
   * `inspect`, so the chart shows the token's whole traded history rather than
   * just the row that was clicked. Omit where there is no position to widen from.
   */
  episodeMarkers?: ChartEventMarker[] | null;
  /** Fingerprint `volume_ix_patterns` keys for the chart vol/non-vol overlay. */
  flowPatternKeys?: ReadonlySet<string> | null;
  /** Optional action slot (Sell / Verify / …) — Console open only. */
  actions?: ReactNode;
  /**
   * Live rule-condition readout (`RuleConditionStrip`) — open + waiting only.
   * A slot rather than a `positionId`, so this component stays free of the live
   * endpoints and keeps rendering for closed positions, which have no engine state.
   */
  conditions?: ReactNode;
  /** Prefill Console manual-trade instead of navigating to `?mint=`. */
  onPrefillTrade?: () => void;
}

function heroWashClass(pct: number | null | undefined): string {
  if (pct == null || !Number.isFinite(pct) || pct === 0) {
    return 'border-white/8 bg-bg-panel';
  }
  if (pct > 0) {
    if (pct >= 100) return 'border-warning/35 bg-warning/8';
    if (pct >= 50) return 'border-green/35 bg-green/8';
    return 'border-info/30 bg-info/6';
  }
  return pct <= -50 ? 'border-red/40 bg-red/10' : 'border-red/30 bg-red/6';
}

/**
 * Position inspect body — hero + actions + compact fact strip + chart
 * (optionally beside a tall panel such as fills). Shared by Console,
 * Waiting, and Rules Evidence.
 */
export function FloorPositionDetail({
  facts,
  chartHeight = 220,
  chartAside,
}: {
  facts: FloorDetailFacts;
  /** Chart height — taller when the detail is shown in a modal vs. an inline row. */
  chartHeight?: number;
  /**
   * Content paired with the chart on wide screens (e.g. fills ledger).
   * Avoids a short fact-tile column that leaves empty space beside the chart.
   */
  chartAside?: ReactNode;
}) {
  const barTrades = useBarTradesSelection();
  // Published to `facts.conditions` (the rule-condition strip) so hovering the chart
  // answers "what did the rule see HERE". A store rather than state: the crosshair
  // moves per frame and must not re-render the chart emitting it.
  const crosshair = useCrosshairTimeStore();
  // Memoized: an open position re-renders on every mark tick, and a fresh marker
  // array would rebuild the chart's markers and the trades table's row tinting
  // each time.
  const singleMarkers = useMemo(() => buildEventMarkers(facts.inspect), [facts.inspect]);
  const markers = facts.episodeMarkers ?? singleMarkers;
  const mtmOrPnl = facts.mtmSol ?? facts.pnlSol;
  const pct = facts.pnlPct;
  const statusKey = facts.statusKey ?? facts.status;
  const isOpenMark = facts.mtmSol != null;
  const solLabel = isOpenMark ? 'MTM' : 'PnL';

  const holdNode =
    facts.holdStartMs != null && Number.isFinite(facts.holdStartMs) ? (
      <ElapsedCell startMs={facts.holdStartMs} className="tabular-nums text-text" />
    ) : facts.holdLabel ? (
      <span className="tabular-nums text-text">{facts.holdLabel}</span>
    ) : null;

  const chart = (
    <LazyFloorMintChart
      mint={facts.mint}
      markers={markers}
      tableId="floor-detail"
      height={chartHeight}
      flowPatternKeys={facts.flowPatternKeys ?? null}
      onCrosshairTimeChange={crosshair.set}
      {...barTrades.chartProps}
    />
  );

  const body = (
    <div className="flex flex-col gap-3">
      {/* Hero — money first, then identity / status chips */}
      <div className={cn('rounded-lg border px-3 py-2.5', heroWashClass(pct))}>
        <div className="flex flex-wrap items-start gap-3">
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <span
                className={facts.origin === 'manual' ? 'text-accent' : 'text-primary'}
                title={facts.origin === 'manual' ? 'manual position' : 'bot position'}
              >
                {facts.origin === 'manual' ? '○' : '●'}
              </span>
              <AddressDisplay
                address={facts.mint}
                kind="token"
                display={facts.symbol ?? undefined}
                reveal="always"
                actionsPlacement="right"
              />
            </div>
            <div className="mt-1.5">
              <OpenPositionStatusChips
                facts={{
                  status: statusKey,
                  origin: facts.origin,
                  mode: facts.mode,
                  showRealMode: true,
                  soldBps: facts.soldBps,
                  scaleStage: facts.scaleStage,
                  exitParked: facts.exitParked,
                  exitRedriveCount: facts.exitRedriveCount,
                  needsReview: facts.needsReview,
                  isDead: facts.isDead,
                  exitReason: facts.exitReason,
                  pnlSol: mtmOrPnl,
                }}
              />
            </div>
          </div>

          <div className="ml-auto text-right">
            <div
              className={cn(
                'text-3xl leading-none tabular-nums tracking-tight',
                pctGradeClass(pct),
              )}
            >
              {pct != null ? formatSignedPct(pct, 1) : '—'}
            </div>
            <div
              className={cn(
                'mt-1 text-base font-semibold tabular-nums',
                signedToneClass(mtmOrPnl),
              )}
            >
              {mtmOrPnl != null ? `${formatSigned(mtmOrPnl, 3)}◎` : '—'}
              <span className="ml-1 text-[10px] font-bold uppercase tracking-wider text-text-dim">
                {solLabel}
              </span>
            </div>
            {holdNode ? (
              <div className="mt-1 text-[11px] text-text-dim">Hold {holdNode}</div>
            ) : null}
          </div>
        </div>

        <div className="mt-2 flex flex-wrap items-center gap-3 text-[11px]">
          {facts.ruleId ? (
            <Link
              to={rulesHref(facts.ruleId)}
              className="font-semibold text-accent hover:underline"
              onClick={(e) => e.stopPropagation()}
            >
              Evidence · {facts.ruleName ?? facts.ruleId.slice(0, 8)}
            </Link>
          ) : facts.ruleName === 'manual' || facts.origin === 'manual' ? (
            <span className="text-text-dim">manual</span>
          ) : null}
          {facts.onPrefillTrade ? (
            <button
              type="button"
              className="font-semibold text-accent hover:underline"
              onClick={(e) => {
                e.stopPropagation();
                facts.onPrefillTrade?.();
              }}
            >
              Prefill trade
            </button>
          ) : (
            <Link
              to={`/console?mint=${encodeURIComponent(facts.mint)}`}
              className="font-semibold text-accent hover:underline"
              onClick={(e) => e.stopPropagation()}
            >
              Trade
            </Link>
          )}
        </div>
      </div>

      {facts.actions ? (
        <div
          className="flex flex-wrap items-center gap-2 rounded-lg border border-white/8 bg-bg-panel px-3 py-2"
          onClick={(e) => e.stopPropagation()}
        >
          <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
            Actions
          </span>
          {facts.actions}
        </div>
      ) : null}

      {/* Compact fact strip — one row, no tall empty column beside the chart */}
      <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1.5 rounded-lg border border-white/8 bg-bg-panel/80 px-3 py-2 text-[12px]">
        <InlineFact
          label="Entry"
          value={facts.entrySol != null ? `${formatCompact(facts.entrySol, 3)}◎` : '—'}
        />
        <InlineFact
          label="Entry px"
          value={facts.entryPrice != null ? formatCompact(facts.entryPrice, 6) : '—'}
          muted
        />
        <InlineFact
          label="Exit px"
          value={facts.exitPrice != null ? formatCompact(facts.exitPrice, 6) : '—'}
          muted
        />
        <InlineFact
          label={solLabel}
          value={
            mtmOrPnl != null ? (
              <span className={signedToneClass(mtmOrPnl)}>{formatSigned(mtmOrPnl, 3)}◎</span>
            ) : (
              '—'
            )
          }
        />
        <InlineFact
          label="PnL%"
          value={
            pct != null ? (
              <span className={pctGradeClass(pct)}>{formatSignedPct(pct, 1)}</span>
            ) : (
              '—'
            )
          }
        />
        <InlineFact
          label="Exit"
          value={
            facts.exitReason ? exitReasonBadge(facts.exitReason, mtmOrPnl, null, 'sm') : '—'
          }
        />
        {holdNode ? <InlineFact label="Hold" value={holdNode} /> : null}
      </div>

      {/* The rule's live conditions, directly under the money facts: the two
          together are the whole answer to "where is this and why". Above the
          chart because it is read at a glance, not scrubbed. */}
      {facts.conditions ?? null}

      {chartAside ? (
        <div className="grid grid-cols-1 items-start gap-3 lg:grid-cols-[minmax(0,1.15fr)_minmax(0,1fr)]">
          {chart}
          <div
            className="min-w-0 lg:overflow-y-auto"
            style={{ maxHeight: chartHeight > 0 ? chartHeight : undefined }}
          >
            {chartAside}
          </div>
        </div>
      ) : (
        chart
      )}

      {/* Bar/range trades sit BELOW the chart ∥ aside grid, not inside the chart
          column — a trades table in ~half a modal's width is unreadable. Mounted
          only while something is picked, so an unselected detail adds no flex
          gap; the click guard keeps a click inside the table from collapsing the
          host's expanded row. */}
      {barTrades.active ? (
        <div onClick={(e) => e.stopPropagation()}>
          <MintBarTradesPanel
            mint={facts.mint}
            selection={barTrades}
            tableId="floor-detail-bar-trades"
            eventMarkers={markers}
            flowPatternKeys={facts.flowPatternKeys ?? null}
            className="border-t border-white/7 pt-2"
          />
        </div>
      ) : null}
    </div>
  );

  // The provider wraps the chart AND the `conditions` slot — they are the two ends
  // of the crosshair, and nothing between them needs to know.
  return <CrosshairTimeProvider value={crosshair}>{body}</CrosshairTimeProvider>;
}

function InlineFact({
  label,
  value,
  muted,
}: {
  label: string;
  value: ReactNode;
  muted?: boolean;
}) {
  return (
    <span className="inline-flex items-baseline gap-1.5">
      <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/75">
        {label}
      </span>
      <span
        className={cn(
          'font-semibold tabular-nums',
          muted ? 'text-text-dim' : 'text-text',
        )}
      >
        {value}
      </span>
    </span>
  );
}
