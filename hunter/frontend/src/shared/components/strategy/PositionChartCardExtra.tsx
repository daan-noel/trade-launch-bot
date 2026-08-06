import { AmountCell, PriceCell } from 'components/tokens/priceCells';
import { Badge } from 'components/ui/Badge';
import { exitReasonBadge, solOf } from 'components/strategy/strategyColumns';
import { formatDurationShort } from 'utils/format';
import { formatSignedPct, pctGradeClass, signedToneClass } from 'lib/signedTone';
import { cn } from 'lib/cn';
import type { RulePositionRecord, SimulatedTokenResult } from 'types';

/**
 * Normalized facts for a position / sim / sweep chart-card header.
 * Callers map their row shape here once; the chrome stays shared.
 */
export interface PositionChartCardFacts {
  holdSecs: number | null;
  pnlSol: number | null;
  pnlPct: number | null;
  entrySol: number | null;
  entryPrice: number | null;
  exitPrice: number | null;
  exitReason: string | null;
  isOpen: boolean;
  /** When charts collapse re-entries to one mint card. */
  episodeCount?: number;
  entryError?: string | null;
}

/** One episode before {@link positionChartFactsFromEpisodes} folds a mint group. */
export interface PositionChartEpisode {
  holdSecs: number | null;
  pnlSol: number | null;
  pnlPct: number | null;
  entrySol: number | null;
  entryPrice: number | null;
  exitPrice: number | null;
  exitReason: string | null;
  isOpen: boolean;
  /** Include in multi-episode aggregates (skip NoEntry / not-fired). */
  include: boolean;
}

/** Chart-header chrome shared by Evidence / Simulate / Dry-run / Sweep. */
export function PositionChartCardExtra({ facts }: { facts: PositionChartCardFacts }) {
  const {
    holdSecs,
    pnlSol,
    pnlPct,
    entrySol,
    entryPrice,
    exitPrice,
    exitReason,
    isOpen,
    episodeCount,
    entryError,
  } = facts;
  const multi = (episodeCount ?? 1) > 1;

  return (
    <span className="ml-auto flex flex-col items-end gap-1 rounded-md border border-white/8 bg-white/3 px-2 py-1 text-[11px]">
      <span className="flex flex-wrap items-center justify-end gap-x-2 gap-y-0.5">
        {multi && (
          <Badge variant="info" size="sm" title="Re-entry episodes overlaid on this chart">
            {episodeCount} eps
          </Badge>
        )}
        {holdSecs != null && holdSecs > 0 && (
          <span
            className="font-mono text-text"
            title={multi ? 'Longest episode hold in this mint group' : 'Entry→exit hold'}
          >
            hold {formatDurationShort(holdSecs)}
          </span>
        )}
        {isOpen ? (
          <Badge variant="info" size="sm">
            open
          </Badge>
        ) : (
          exitReasonBadge(exitReason, pnlSol, entryError)
        )}
      </span>
      <span className="flex flex-wrap items-center justify-end gap-x-2 gap-y-0.5">
        {pnlSol != null && Number.isFinite(pnlSol) ? (
          <span className={cn('font-bold', signedToneClass(pnlSol))}>
            <AmountCell sol={pnlSol} /> PnL
            {pnlPct != null && Number.isFinite(pnlPct) && (
              <span className={cn('ml-1 font-mono font-semibold', pctGradeClass(pnlPct))}>
                ({formatSignedPct(pnlPct, 1)})
              </span>
            )}
          </span>
        ) : (
          <span className="text-text-dim">PnL —</span>
        )}
        {entrySol != null && Number.isFinite(entrySol) && (
          <span className="text-text-dim">
            size <AmountCell sol={entrySol} />
          </span>
        )}
      </span>
      <span className="flex flex-wrap items-center justify-end gap-x-2 gap-y-0.5 text-text-dim">
        {entryPrice != null && (
          <span>
            entry <PriceCell sol={entryPrice} />
            {exitPrice != null && (
              <>
                {' '}
                · exit <PriceCell sol={exitPrice} />
              </>
            )}
          </span>
        )}
      </span>
    </span>
  );
}

/** Fold one or more episodes (sim/sweep groupByMint) into header facts. */
export function positionChartFactsFromEpisodes(
  episodes: readonly PositionChartEpisode[],
): PositionChartCardFacts {
  const use = episodes.filter((e) => e.include);
  const list = use.length > 0 ? use : episodes;
  if (list.length === 0) {
    return {
      holdSecs: null,
      pnlSol: null,
      pnlPct: null,
      entrySol: null,
      entryPrice: null,
      exitPrice: null,
      exitReason: null,
      isOpen: false,
    };
  }
  if (list.length === 1) {
    const e = list[0]!;
    return {
      holdSecs: e.holdSecs,
      pnlSol: e.pnlSol,
      pnlPct: e.pnlPct,
      entrySol: e.entrySol,
      entryPrice: e.entryPrice,
      exitPrice: e.exitPrice,
      exitReason: e.isOpen ? null : e.exitReason,
      isOpen: e.isOpen,
      episodeCount: 1,
    };
  }

  let pnlSol = 0;
  let entrySolSum = 0;
  let hasEntrySol = false;
  let holdMax = 0;
  let open = false;
  let lastExit: string | null = null;
  let entryPrice: number | null = null;
  let exitPrice: number | null = null;
  for (const e of list) {
    if (e.pnlSol != null) pnlSol += e.pnlSol;
    if (e.entrySol != null) {
      entrySolSum += e.entrySol;
      hasEntrySol = true;
    }
    if (e.holdSecs != null && e.holdSecs > holdMax) holdMax = e.holdSecs;
    if (e.isOpen) open = true;
    else if (e.exitReason) lastExit = e.exitReason;
    if (entryPrice == null && e.entryPrice != null) entryPrice = e.entryPrice;
    if (e.exitPrice != null) exitPrice = e.exitPrice;
  }
  return {
    holdSecs: holdMax > 0 ? holdMax : null,
    pnlSol,
    pnlPct: null, // mixed episodes — SOL sum is the honest aggregate
    entrySol: hasEntrySol ? entrySolSum : null,
    entryPrice,
    exitPrice,
    exitReason: open ? null : lastExit,
    isOpen: open,
    episodeCount: list.length,
  };
}

function positionIsOpen(r: RulePositionRecord): boolean {
  return !r.exit_time && r.status !== 'End' && r.status !== 'EntryFailed';
}

function positionHoldSecs(r: RulePositionRecord): number | null {
  if (!r.entry_time) return null;
  const start = Date.parse(r.entry_time);
  if (!Number.isFinite(start)) return null;
  const end = r.exit_time ? Date.parse(r.exit_time) : Date.now();
  if (!Number.isFinite(end)) return null;
  return Math.max(0, (end - start) / 1000);
}

/** Evidence / live positions — one card per position id. */
export function positionChartFactsFromRule(r: RulePositionRecord): PositionChartCardFacts {
  const open = positionIsOpen(r);
  return {
    holdSecs: positionHoldSecs(r),
    pnlSol: r.pnl_sol,
    pnlPct: r.pnl_percent,
    entrySol: r.entry_sol ?? solOf(r.entry_price, r.entry_token_amount),
    entryPrice: r.entry_price,
    exitPrice: r.exit_price,
    exitReason: open ? null : r.exit_reason,
    isOpen: open,
    entryError: r.last_entry_error,
  };
}

function simFired(r: SimulatedTokenResult): boolean {
  return r.fired !== false && r.exit_reason !== 'NoEntry';
}

function simIsOpen(r: SimulatedTokenResult): boolean {
  return simFired(r) && (r.exit_reason === 'Open' || r.exit_time == null);
}

function simEpisode(r: SimulatedTokenResult): PositionChartEpisode {
  return {
    holdSecs: r.holding_secs,
    pnlSol: r.pnl_sol,
    pnlPct: r.pnl_percent,
    entrySol: solOf(r.entry_price, r.entry_token_amount),
    entryPrice: r.entry_price,
    exitPrice: r.exit_price,
    exitReason: r.exit_reason,
    isOpen: simIsOpen(r),
    include: simFired(r),
  };
}

/** Simulate / dry-run — folds re-entry episodes when the grid groups by mint. */
export function positionChartFactsFromSim(
  rows: readonly SimulatedTokenResult[],
): PositionChartCardFacts {
  return positionChartFactsFromEpisodes(rows.map(simEpisode));
}
