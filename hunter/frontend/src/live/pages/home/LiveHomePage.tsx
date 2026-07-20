import { useMemo } from 'react';
import { Link } from 'react-router-dom';
import { useSelector } from 'react-redux';
import { StatTile } from 'components/ui/StatTile';
import { usePriceUnit } from 'context/PriceUnitContext';
import { formatCompact, formatUsd } from 'utils/format';
import { formatSigned, formatSignedPct, signedStatTone } from 'lib/signedTone';
import { TopHoldingsWidget } from '@live/components/home/TopHoldingsWidget';
import { LiveTradeFeed } from '@live/components/home/LiveTradeFeed';
import { StrategyStrip } from '@live/components/home/StrategyStrip';
import {
  useGetPortfolioSummaryQuery,
  useGetLiveModeQuery,
} from '@live/store/liveEndpoints';
import { selectLiveOpen } from '@live/slices/liveStatusSlice';

/**
 * Home "Command Center" (live build) — the single pane of glass. KPI tiles deep-link
 * to the owning page so glance → act stays one click.
 */
export function LiveHomePage() {
  const { data: summary } = useGetPortfolioSummaryQuery();
  const { data: live } = useGetLiveModeQuery();
  const { usdRate } = usePriceUnit();
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
      <div className="mb-4 flex flex-wrap items-baseline gap-3">
        <h1 className="text-2xl font-extrabold text-text">Command Center</h1>
        <span className="text-sm text-text-mid">
          Glance → act · Wallet = bag · Ops = live inventory · Trade = execute
        </span>
      </div>

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
        <StatTile
          label="Realized Today"
          value={realizedToday != null ? `◎${formatSigned(realizedToday, 3)}` : '—'}
          tone={signedStatTone(realizedToday)}
        />
        <Link to="/ops" className="block rounded-lg focus:outline-none focus-visible:ring-1 focus-visible:ring-primary">
          <StatTile
            label="Open Positions"
            value={liveOpenReal}
            sub="ops · live"
          />
        </Link>
        <Link to="/strategies/rules" className="block rounded-lg focus:outline-none focus-visible:ring-1 focus-visible:ring-primary">
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
        <StrategyStrip />
      </div>

      <div className="mt-3 grid grid-cols-1 gap-3 lg:grid-cols-2">
        <TopHoldingsWidget />
        <LiveTradeFeed />
      </div>
    </div>
  );
}
