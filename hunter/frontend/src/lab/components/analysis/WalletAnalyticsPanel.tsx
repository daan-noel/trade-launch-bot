import { Suspense, lazy, useMemo, useState } from 'react';
import { Button } from 'components/ui/Button';
import { LoadingState } from 'components/ui/LoadingState';
import { WalletPnlSummaryRow } from './WalletPnlSummary';
import { WalletPnlHeatmap } from './WalletPnlHeatmap';
import { WalletRankedPnlBars } from './WalletRankedPnlBars';
import { WalletPnlDistribution } from './WalletPnlDistribution';
import { WalletHoldScatter } from './WalletHoldScatter';
import { buildEquityCurve, buildPnlHeatCells, computeWalletSummary } from './walletPnlStats';
import type { TraderTokenRow } from 'types';

/** Equity curve is the only view pulling `lightweight-charts` — keep it out of
 *  the panel's initial chunk (mirrors `LazyTokenChartsGrid`/`CreationTrendChart`'s
 *  lazy-import convention) so switching to the other four tabs never loads it. */
const WalletEquityCurveChart = lazy(() =>
  import('./WalletEquityCurveChart').then((m) => ({ default: m.WalletEquityCurveChart })),
);

type ChartTab = 'heatmap' | 'equity' | 'ranked' | 'distribution' | 'scatter';

const TABS: { id: ChartTab; label: string; hint: string }[] = [
  { id: 'heatmap', label: 'PnL heatmap', hint: 'Day × hour — when this wallet makes/loses money' },
  { id: 'equity', label: 'Equity curve', hint: 'Cumulative mark-to-market PnL over time' },
  { id: 'ranked', label: 'Ranked by PnL', hint: 'Best → worst token, not by win rate' },
  { id: 'distribution', label: 'Win/loss sizes', hint: 'Distribution of closed round-trip returns' },
  { id: 'scatter', label: 'Hold vs PnL', hint: 'Fast scalps or rides?' },
];

interface WalletAnalyticsPanelProps {
  /** The table's current FULL filtered cohort (not just the on-screen page) —
   *  see `DataTable.onFilteredRowsChange` — so filtering the token table also
   *  filters every stat/chart here. */
  rows: readonly TraderTokenRow[];
  timezone: string;
}

/**
 * Wallet-level PnL analytics — the summary stat row (always visible) plus a
 * tabbed chart panel (one chart mounted at a time, so switching views never
 * pays for building all five). Every number here is re-derived client-side
 * from the same enriched rows the token table renders (no extra fetch) — see
 * `walletPnlStats.ts` for the fold + the per-mint-grain caveats.
 */
export function WalletAnalyticsPanel({ rows, timezone }: WalletAnalyticsPanelProps) {
  const [tab, setTab] = useState<ChartTab>('heatmap');

  const summary = useMemo(() => computeWalletSummary(rows), [rows]);
  const heatCells = useMemo(() => buildPnlHeatCells(rows, timezone), [rows, timezone]);
  const equityPoints = useMemo(() => buildEquityCurve(rows), [rows]);
  const activeTab = TABS.find((t) => t.id === tab) ?? TABS[0]!;

  if (rows.length === 0) return null;

  return (
    <div className="mb-4 flex flex-col gap-3">
      <WalletPnlSummaryRow summary={summary} />

      <section className="rounded-lg border border-white/8 bg-white/2 p-3">
        <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
          <div className="flex flex-wrap items-center gap-1">
            {TABS.map((t) => (
              <Button key={t.id} size="sm" variant="subtle" active={tab === t.id} onClick={() => setTab(t.id)}>
                {t.label}
              </Button>
            ))}
          </div>
          <span className="text-[10px] text-text-dim">{activeTab.hint}</span>
        </div>

        {tab === 'heatmap' && <WalletPnlHeatmap cells={heatCells} />}
        {tab === 'equity' && (
          <Suspense fallback={<LoadingState variant="inline" label="Loading chart…" />}>
            <WalletEquityCurveChart points={equityPoints} timezone={timezone} />
          </Suspense>
        )}
        {tab === 'ranked' && <WalletRankedPnlBars rows={rows} />}
        {tab === 'distribution' && <WalletPnlDistribution rows={rows} />}
        {tab === 'scatter' && <WalletHoldScatter rows={rows} />}
      </section>
    </div>
  );
}
