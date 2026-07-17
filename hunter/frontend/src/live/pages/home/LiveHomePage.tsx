import { Link } from 'react-router-dom';
import { StatTile } from 'components/ui/StatTile';
import { usePriceUnit } from 'context/PriceUnitContext';
import { formatCompact, formatUsd } from 'utils/format';
import { TopHoldingsWidget } from '@live/components/home/TopHoldingsWidget';
import { LiveTradeFeed } from '@live/components/home/LiveTradeFeed';
import { StrategyStrip } from '@live/components/home/StrategyStrip';
import {
  useGetPortfolioSummaryQuery,
  useGetLiveModeQuery,
} from '@live/store/liveEndpoints';

/** `+`-prefixed compact number for signed PnL displays. */
function signed(v: number, digits: number): string {
  return `${v > 0 ? '+' : ''}${formatCompact(v, digits)}`;
}

function pnlTone(v: number): 'green' | 'red' | 'default' {
  if (v > 0) return 'green';
  if (v < 0) return 'red';
  return 'default';
}

/**
 * Home "Command Center" (live build) — the single pane of glass. KPI tiles deep-link
 * to the owning page so glance → act stays one click.
 */
export function LiveHomePage() {
  const { data: summary } = useGetPortfolioSummaryQuery();
  const { data: live } = useGetLiveModeQuery();
  const { usdRate } = usePriceUnit();

  const valueSol = summary?.total_value_sol ?? null;
  const valueUsd = summary?.total_value_usd ?? null;
  const pnlSol = summary?.total_unrealized_pnl_sol ?? null;
  const pnlPct =
    summary && summary.total_cost_basis_sol > 0
      ? (summary.total_unrealized_pnl_sol / summary.total_cost_basis_sol) * 100
      : null;
  const realizedToday = summary?.realized_pnl_today_sol ?? null;
  void usdRate;

  return (
    <div className="pt-2">
      <div className="mb-4 flex flex-wrap items-baseline gap-3">
        <h1 className="text-2xl font-extrabold text-text">Command Center</h1>
        <span className="text-sm text-text-mid">
          Glance → act · Wallet = bag · Positions = bot inventory · Trade = execute
        </span>
      </div>

      <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-3 lg:grid-cols-6">
        <Link to="/wallet" className="block rounded-lg focus:outline-none focus-visible:ring-1 focus-visible:ring-primary">
          <StatTile
            label="Wallet Value"
            value={valueSol != null ? `◎${formatCompact(valueSol, 2)}` : '—'}
            sub={valueUsd != null ? formatUsd(valueUsd) : undefined}
          />
        </Link>
        <Link to="/wallet" className="block rounded-lg focus:outline-none focus-visible:ring-1 focus-visible:ring-primary">
          <StatTile
            label="Unrealized PnL"
            value={pnlSol != null ? `◎${signed(pnlSol, 3)}` : '—'}
            sub={pnlPct != null ? `${signed(pnlPct, 1)}%` : undefined}
            tone={pnlSol != null ? pnlTone(pnlSol) : 'default'}
          />
        </Link>
        <StatTile
          label="Realized Today"
          value={realizedToday != null ? `◎${signed(realizedToday, 3)}` : '—'}
          tone={realizedToday != null ? pnlTone(realizedToday) : 'default'}
        />
        <Link to="/positions" className="block rounded-lg focus:outline-none focus-visible:ring-1 focus-visible:ring-primary">
          <StatTile
            label="Open Positions"
            value={summary?.open_position_count ?? '—'}
            sub="strategy"
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
