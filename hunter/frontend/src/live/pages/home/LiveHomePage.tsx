import { useMemo } from 'react';
import { Link } from 'react-router-dom';
import { useSelector } from 'react-redux';
import { PageHeader } from 'components/ui/PageHeader';
import { StatTile } from 'components/ui/StatTile';
import { useUsdRate } from 'context/PriceUnitContext';
import { formatCompact, formatUsd } from 'utils/format';
import { formatSigned, formatSignedPct, signedStatTone } from 'lib/signedTone';
import { consoleHref, portfolioHref, rulesHref } from 'lib/strategy/nav';
import { TopHoldingsWidget } from '@live/components/home/TopHoldingsWidget';
import { LiveTradeFeed } from '@live/components/home/LiveTradeFeed';
import { ReviewDigest } from '@live/components/home/ReviewDigest';
import { StrategyStrip } from '@live/components/home/StrategyStrip';
import {
  useGetPortfolioSummaryQuery,
  useGetLiveModeQuery,
} from '@live/store/liveEndpoints';
import { selectLiveOpen } from '@live/slices/liveStatusSlice';

/**
 * Home "Command Center" (live build) — the single pane of glass, weighted for
 * *coming back to it*: the review digest (week of PnL, attention count, rule
 * decay alerts) sits directly under the KPI tiles, and the live trade feed —
 * an actively-watching artifact — is demoted to a collapsed side panel.
 * KPI tiles deep-link to the owning page so glance → act stays one click.
 */
export function LiveHomePage() {
  const { data: summary } = useGetPortfolioSummaryQuery();
  const { data: live } = useGetLiveModeQuery();
  const { usdRate } = useUsdRate();
  const openMap = useSelector(selectLiveOpen);
  const liveOpenReal = useMemo(
    () => Object.values(openMap).filter((p) => p.mode === 'real').length,
    [openMap],
  );

  const valueSol = summary?.total_value_sol ?? null;
  const valueUsd = summary?.total_value_usd ?? null;
  const cashUsd = summary?.cash_value_usd ?? 0;
  const posUsd = summary?.positions_value_usd ?? 0;
  const pnlSol = summary?.total_unrealized_pnl_sol ?? null;
  const pnlPct =
    summary && summary.total_cost_basis_sol > 0
      ? (summary.total_unrealized_pnl_sol / summary.total_cost_basis_sol) * 100
      : null;
  const realizedToday = summary?.realized_pnl_today_sol ?? null;
  void usdRate;

  const walletSub =
    valueUsd != null
      ? cashUsd > 0
        ? `${formatUsd(cashUsd)} cash · ${formatUsd(posUsd)} positions`
        : formatUsd(valueUsd)
      : undefined;

  return (
    <div className="pt-2">
      <PageHeader
        size="page"
        title="Command Center"
        description="Console = book · Portfolio = which rules earn their keep · Rules = keep/kill"
      />

      <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-3 lg:grid-cols-6">
        <Link to="/wallet" className="block rounded-lg focus:outline-none focus-visible:ring-1 focus-visible:ring-primary">
          <StatTile
            label="Wallet Value"
            value={valueSol != null ? `◎${formatCompact(valueSol, 2)}` : '—'}
            sub={walletSub}
          />
        </Link>
        <Link to="/wallet" className="block rounded-lg focus:outline-none focus-visible:ring-1 focus-visible:ring-primary">
          <StatTile
            label="Unrealized PnL"
            value={pnlSol != null ? `◎${formatSigned(pnlSol, 3)}` : '—'}
            sub={pnlPct != null ? formatSignedPct(pnlPct, 1) : undefined}
            tone={signedStatTone(pnlSol)}
          />
        </Link>
        <Link
          to={portfolioHref('today')}
          className="block rounded-lg focus:outline-none focus-visible:ring-1 focus-visible:ring-primary"
        >
          <StatTile
            label="Realized Today"
            value={realizedToday != null ? `◎${formatSigned(realizedToday, 3)}` : '—'}
            tone={signedStatTone(realizedToday)}
          />
        </Link>
        <Link
          to={consoleHref({ mode: 'real' })}
          className="block rounded-lg focus:outline-none focus-visible:ring-1 focus-visible:ring-primary"
        >
          <StatTile label="Open Positions" value={liveOpenReal} sub="console · live" />
        </Link>
        <Link
          to={rulesHref()}
          className="block rounded-lg focus:outline-none focus-visible:ring-1 focus-visible:ring-primary"
        >
          <StatTile label="Active Rules" value={summary?.active_rules ?? '—'} sub="armed on live" />
        </Link>
        <StatTile
          label="Trading"
          value={live == null ? '—' : live ? 'ON' : 'OFF'}
          tone={live ? 'green' : 'muted'}
          sub="header switch"
        />
      </div>

      <div className="mt-3">
        <ReviewDigest />
      </div>

      <div className="mt-3">
        <StrategyStrip />
      </div>

      <div className="mt-3 grid grid-cols-1 gap-3 lg:grid-cols-[minmax(0,1fr)_340px]">
        <TopHoldingsWidget />
        {/* The trade feed only means something while you're watching it — kept
            available, but collapsed by default so it doesn't outrank the review
            content above on a page you mostly come back to. */}
        <details className="rounded-lg border border-white/6 bg-bg-panel">
          <summary className="cursor-pointer px-3 py-2 text-[10px] font-bold uppercase tracking-wider text-text-dim hover:text-text">
            Live trade feed
          </summary>
          <div className="px-1 pb-1">
            <LiveTradeFeed />
          </div>
        </details>
      </div>
    </div>
  );
}
