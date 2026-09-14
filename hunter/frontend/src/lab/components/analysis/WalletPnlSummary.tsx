import { memo, type ComponentProps, type ReactNode } from 'react';
import { StatTile, type StatTone } from 'components/ui/StatTile';
import { AmountCell } from 'components/tokens/priceCells';
import { fmtSecs, pctText } from 'lib/strategy/runSummary';
import { formatDecimalTrim, formatWithCommas } from 'utils/format';
import { cn } from 'lib/cn';
import { WALLET_STATS, type WalletPnlSummary, type WalletStatKey } from './walletPnlStats';

/** A tile named and explained by its [`WALLET_STATS`] entry — the only way a
 *  wallet figure reaches the summary, so label and ⓘ can't drift from the fold. */
function Stat({
  k,
  ...tile
}: { k: WalletStatKey } & Omit<ComponentProps<typeof StatTile>, 'label' | 'info'>) {
  const d = WALLET_STATS[k];
  return <StatTile label={d.label} info={d.def} {...tile} />;
}

/** One titled group of small tiles — the same heading chrome as `MixBand`. */
function StatGroup({ title, hint, children }: { title: string; hint: string; children: ReactNode }) {
  return (
    <section className="min-w-0 rounded-lg border border-white/6 bg-bg-panel px-3 py-2.5">
      <div className="mb-1.5">
        <div className="text-[10px] font-bold uppercase tracking-wider text-text-mid">{title}</div>
        <div className="mt-0.5 text-[10px] leading-snug text-text-dim">{hint}</div>
      </div>
      <div className="grid grid-cols-2 gap-1.5 sm:grid-cols-3">{children}</div>
    </section>
  );
}

/** Green when > 0, red when < 0, default (dim) at exactly 0 or `null`. */
function signTone(v: number | null): StatTone {
  if (v == null || v === 0) return 'default';
  return v > 0 ? 'green' : 'red';
}

function pct(v: number | null, digits = 1): string {
  return v == null ? '—' : `${v.toFixed(digits)}%`;
}

function ratio(v: number | null): string {
  return v == null ? '—' : `${v.toFixed(2)}x`;
}

function pctOf(share: number): string {
  return `${(share * 100).toFixed(0)}%`;
}

type StatusToggle = 'open' | 'closed';
type OutcomeToggle = 'win' | 'loss';

interface WalletPnlSummaryRowProps {
  summary: WalletPnlSummary;
  /** Active status lens, if any. */
  status?: StatusToggle | null;
  /** Active outcome lens, if any. */
  outcome?: OutcomeToggle | null;
  onToggleStatus?: (status: StatusToggle) => void;
  onToggleOutcome?: (outcome: OutcomeToggle) => void;
}

interface MixSlice {
  key: string;
  n: number;
  label: string;
  full: string;
  bar: string;
  active: boolean;
  onSelect?: () => void;
}

/**
 * At-a-glance verdict for the wallet under analysis, net of fee throughout:
 * the Money row (net realized, open mark, their total, return %), then the
 * Per trade distribution, Risk and Behavior groups. Every tile is named and
 * explained by `WALLET_STATS`. Money / stat tiles stay display-only; Open /
 * Closed / Winners / Losers use the same proportion-bar + clickable count tiles
 * as Console / Position Summary (exit mix / Positions band).
 */
export const WalletPnlSummaryRow = memo(function WalletPnlSummaryRow({
  summary,
  status = null,
  outcome = null,
  onToggleStatus,
  onToggleOutcome,
}: WalletPnlSummaryRowProps) {
  const statusSlices: MixSlice[] = [
    {
      key: 'closed',
      n: summary.closedCount,
      label: 'Closed',
      full: 'Closed mints (no open bag)',
      bar: 'bg-info',
      active: status === 'closed',
      onSelect: onToggleStatus ? () => onToggleStatus('closed') : undefined,
    },
    {
      key: 'open',
      n: summary.openCount,
      label: 'Open',
      full: 'Still holding an open bag',
      bar: 'bg-warning',
      active: status === 'open',
      onSelect: onToggleStatus ? () => onToggleStatus('open') : undefined,
    },
  ];

  const outcomeSlices: MixSlice[] = [
    {
      key: 'win',
      n: summary.winCount,
      label: 'Winners',
      full: 'Trades with net realized above zero',
      bar: 'bg-green',
      active: outcome === 'win',
      onSelect: onToggleOutcome ? () => onToggleOutcome('win') : undefined,
    },
    {
      key: 'loss',
      n: summary.lossCount,
      label: 'Losers',
      full: 'Trades with net realized at or below zero',
      bar: 'bg-red',
      active: outcome === 'loss',
      onSelect: onToggleOutcome ? () => onToggleOutcome('loss') : undefined,
    },
  ];

  const s = summary;
  const decided = s.tradeCount;
  const openShare = s.tokenCount > 0 ? s.openCount / s.tokenCount : 0;

  return (
    <div className="flex flex-col gap-3">
      {/* Money: the four figures, one basis (net of fee). */}
      <div className="grid grid-cols-2 gap-2 lg:grid-cols-4">
        <Stat
          k="netRealizedSol"
          value={<AmountCell sol={s.netRealizedSol} />}
          tone={signTone(s.netRealizedSol)}
          sub={
            <>
              gross <AmountCell sol={s.grossRealizedSol} />
            </>
          }
        />
        <Stat
          k="openMarkSol"
          value={<AmountCell sol={s.openMarkSol} />}
          tone={signTone(s.openMarkSol)}
          sub={`${s.openCount} open`}
        />
        <Stat k="totalSol" value={<AmountCell sol={s.totalSol} />} tone={signTone(s.totalSol)} bold />
        <Stat
          k="returnPct"
          value={pctText(s.returnPct)}
          tone={signTone(s.returnPct)}
          sub={
            <>
              over <AmountCell sol={s.matchedCostSol} /> sold cost
            </>
          }
        />
      </div>

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        <StatGroup title="Per trade" hint={`${decided} trade${decided === 1 ? '' : 's'} with a sell`}>
          <Stat k="winRate" size="sm" value={pct(s.winRate)} sub={`${s.winCount}W / ${s.lossCount}L`} />
          <Stat k="medianPct" size="sm" value={pctText(s.medianPct)} tone={signTone(s.medianPct)} />
          <Stat k="meanPct" size="sm" value={pctText(s.meanPct)} tone={signTone(s.meanPct)} />
          <Stat
            k="p10p90Pct"
            size="sm"
            value={`${pctText(s.p10Pct)} / ${pctText(s.p90Pct)}`}
          />
          <Stat
            k="bestWorstPct"
            size="sm"
            value={`${pctText(s.bestPct)} / ${pctText(s.worstPct)}`}
          />
          <Stat
            k="expectancySol"
            size="sm"
            value={<AmountCell sol={s.expectancySol} />}
            tone={signTone(s.expectancySol)}
          />
        </StatGroup>

        <StatGroup title="Risk" hint="How deep and how long it bled">
          <Stat
            k="maxDrawdownSol"
            size="sm"
            value={<AmountCell sol={s.maxDrawdownSol > 0 ? -s.maxDrawdownSol : 0} />}
            tone={s.maxDrawdownSol > 0 ? 'red' : 'default'}
          />
          <Stat
            k="worstTradeSol"
            size="sm"
            value={<AmountCell sol={s.worstTradeSol} />}
            tone={signTone(s.worstTradeSol)}
            sub={s.worstTradePct == null ? undefined : pctText(s.worstTradePct)}
          />
          <Stat
            k="lossStreak"
            size="sm"
            value={decided > 0 ? String(s.longestLossStreak) : '—'}
            tone={s.longestLossStreak >= 5 ? 'red' : 'default'}
          />
          <Stat
            k="profitFactor"
            size="sm"
            value={ratio(s.profitFactor)}
            tone={s.profitFactor == null ? 'default' : s.profitFactor >= 1 ? 'green' : 'red'}
          />
          <Stat k="payoffRatio" size="sm" value={ratio(s.payoffRatio)} />
          <Stat
            k="avgWinLossSol"
            size="sm"
            value={
              <>
                <AmountCell sol={s.avgWinSol} /> / <AmountCell sol={s.avgLossSol} />
              </>
            }
          />
        </StatGroup>

        <StatGroup title="Behavior" hint="How it trades, before any PnL">
          <Stat
            k="capitalInSol"
            size="sm"
            value={<AmountCell sol={s.capitalInSol} />}
            sub={
              <>
                vol <AmountCell sol={s.volumeSol} />
              </>
            }
          />
          <Stat k="medianBuySol" size="sm" value={<AmountCell sol={s.medianBuySol} />} />
          <Stat k="medianHoldSecs" size="sm" value={fmtSecs(s.medianHoldSecs)} />
          <Stat
            k="medianEntryCurvePct"
            size="sm"
            value={s.medianEntryCurvePct == null ? '—' : `${formatDecimalTrim(s.medianEntryCurvePct, 1)}%`}
          />
          <Stat
            k="tokenCount"
            size="sm"
            value={formatWithCommas(s.tokenCount)}
            sub={s.partialDataCount > 0 ? `${s.partialDataCount} partial` : undefined}
          />
        </StatGroup>
      </div>

      {(onToggleStatus || onToggleOutcome) && (
        <div className="grid grid-cols-1 gap-4 rounded-lg border border-white/6 bg-bg-panel px-3 py-2.5 md:grid-cols-2 md:gap-6">
          {onToggleStatus && (
            <MixBand
              title="Positions"
              hint="Open bags vs fully closed mints — click a segment or tile to focus"
              slices={statusSlices}
              ariaLabel="Open vs closed mix"
              tiles={
                <>
                  <CountTile
                    label="Closed"
                    value={String(summary.closedCount)}
                    cls="text-info"
                    active={status === 'closed'}
                    onClick={onToggleStatus ? () => onToggleStatus('closed') : undefined}
                  />
                  <CountTile
                    label="Open"
                    value={String(summary.openCount)}
                    cls={summary.openCount > 0 ? 'text-warning' : 'text-text-dim'}
                    active={status === 'open'}
                    onClick={onToggleStatus ? () => onToggleStatus('open') : undefined}
                  />
                  <CountTile
                    label="Open share"
                    value={summary.tokenCount ? pctOf(openShare) : '—'}
                    cls={summary.openCount > 0 ? 'text-warning' : 'text-text-dim'}
                  />
                </>
              }
            />
          )}

          {onToggleOutcome && (
            <MixBand
              title="Outcomes"
              hint={
                decided > 0
                  ? `Trades only (${decided} of ${summary.tokenCount}), net of fee — never-sold bags excluded`
                  : 'No matched cost basis yet (every row is still an open bag)'
              }
              slices={outcomeSlices}
              ariaLabel="Win vs loss mix"
              className={onToggleStatus ? 'border-t border-white/6 pt-4 md:border-t-0 md:border-l md:pt-0 md:pl-6' : undefined}
              tiles={
                <>
                  <CountTile
                    label="Winners"
                    value={String(summary.winCount)}
                    cls="text-green"
                    active={outcome === 'win'}
                    onClick={onToggleOutcome ? () => onToggleOutcome('win') : undefined}
                  />
                  <CountTile
                    label="Losers"
                    value={String(summary.lossCount)}
                    cls="text-red"
                    active={outcome === 'loss'}
                    onClick={onToggleOutcome ? () => onToggleOutcome('loss') : undefined}
                  />
                  <CountTile
                    label="Win %"
                    value={pct(summary.winRate)}
                    cls={summary.winRate == null ? 'text-text-dim' : undefined}
                    active={outcome === 'win'}
                    onClick={onToggleOutcome ? () => onToggleOutcome('win') : undefined}
                  />
                </>
              }
            />
          )}
        </div>
      )}
    </div>
  );
});

/** One labelled band: proportion bar (exit-mix chrome) + count tiles underneath. */
function MixBand({
  title,
  hint,
  slices,
  ariaLabel,
  tiles,
  className,
}: {
  title: string;
  hint: string;
  slices: MixSlice[];
  ariaLabel: string;
  tiles: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn('min-w-0', className)}>
      <div className="mb-1.5">
        <div className="text-[10px] font-bold uppercase tracking-wider text-text-mid">{title}</div>
        <div className="mt-0.5 text-[10px] leading-snug text-text-dim">{hint}</div>
      </div>
      <MixBar slices={slices} ariaLabel={ariaLabel} />
      <div className="mt-2 flex flex-wrap gap-x-8 gap-y-3">{tiles}</div>
    </div>
  );
}

/**
 * Horizontal proportion bar — same chrome as `ExitMixBar` in `runSummary.tsx`
 * (flex-grown segments, 2px gaps, click-to-focus ring). Kept local so the wallet
 * surface doesn't import a private helper; keep the two visually in lockstep.
 */
function MixBar({ slices, ariaLabel }: { slices: MixSlice[]; ariaLabel: string }) {
  const shown = slices.filter((s) => s.n > 0);
  if (shown.length === 0) return null;
  const total = shown.reduce((a, s) => a + s.n, 0);
  return (
    <div className="mb-2 flex h-1.5 w-full gap-0.5" role="img" aria-label={ariaLabel}>
      {shown.map((s) => {
        const share = total > 0 ? s.n / total : 0;
        const title = `${s.full}: ${s.n} (${pctOf(share)})${s.onSelect ? ' — click to focus' : ''}`;
        if (!s.onSelect) {
          return (
            <div
              key={s.key}
              className={cn('h-full rounded-xs', s.bar)}
              style={{ flexGrow: s.n, flexBasis: 0 }}
              title={title}
            />
          );
        }
        return (
          <button
            key={s.key}
            type="button"
            onClick={s.onSelect}
            title={title}
            className={cn(
              'h-full rounded-xs border-0 p-0',
              s.bar,
              'cursor-pointer transition hover:opacity-90',
              s.active && 'ring-1 ring-white/70',
            )}
            style={{ flexGrow: s.n, flexBasis: 0 }}
          />
        );
      })}
    </div>
  );
}

function CountTile({
  label,
  value,
  cls,
  active,
  onClick,
}: {
  label: string;
  value: string;
  cls?: string;
  active?: boolean;
  onClick?: () => void;
}) {
  const body = (
    <>
      <span className="text-[9px] font-semibold uppercase tracking-wider text-text-dim">{label}</span>
      <span className={cn('font-mono text-sm font-bold text-text', cls)}>{value}</span>
    </>
  );
  if (!onClick) {
    return <div className="flex min-w-[84px] flex-col gap-0.5">{body}</div>;
  }
  return (
    <button
      type="button"
      onClick={onClick}
      title={`Click to focus ${label.toLowerCase()}`}
      className={cn(
        'flex min-w-[84px] flex-col gap-0.5 rounded-md px-1 py-0.5 text-left transition hover:bg-white/5',
        active && 'bg-primary/15 ring-1 ring-primary/50',
      )}
    >
      {body}
    </button>
  );
}
