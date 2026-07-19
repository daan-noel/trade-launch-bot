import { useMemo } from 'react';
import { DataTable } from 'components/table/DataTable';
import { StatTile } from 'components/ui/StatTile';
import { InlineAlert } from 'components/ui/Modal';
import { apiErrorMessage } from 'store/apiSlice';
import { formatCompact } from 'utils/format';
import { positionColumns } from '@live/components/live-trading/positionColumns';
import { useGetPortfolioPositionsQuery } from '@live/store/liveEndpoints';

const STRATEGY_LABEL: Record<string, string> = {
  tpsl_sniper_1: 'TPSL1',
  tpsl_sniper_2: 'TPSL2',
};

/**
 * Positions roll-up — cross-strategy REAL-money open positions.
 * Reads `GET /api/portfolio/positions` (real-only).
 */
export function LiveTradingPage() {
  const { data: positions = [], isLoading, isFetching, error } = useGetPortfolioPositionsQuery(true);
  const columns = useMemo(() => positionColumns(), []);
  const errMsg = apiErrorMessage(error);

  const perStrategy = useMemo(() => {
    const map = new Map<string, { open: number; deployedSol: number; rules: Set<string> }>();
    for (const p of positions) {
      const row = map.get(p.strategy_id) ?? { open: 0, deployedSol: 0, rules: new Set<string>() };
      row.open += 1;
      row.deployedSol += p.entry_sol ?? 0;
      if (p.rule_id) row.rules.add(p.rule_id);
      map.set(p.strategy_id, row);
    }
    return [...map.entries()]
      .map(([strategy_id, v]) => ({ strategy_id, ...v, rules: v.rules.size }))
      .sort((a, b) => b.open - a.open);
  }, [positions]);

  const totalDeployed = useMemo(
    () => positions.reduce((s, p) => s + (p.entry_sol ?? 0), 0),
    [positions],
  );

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-baseline gap-3">
        <h1 className="text-lg font-extrabold text-text">Positions</h1>
        <span className="text-sm text-text-mid">
          Bot inventory · open strategy positions (Trade = execute · Wallet = bag)
        </span>
      </div>

      {errMsg && <InlineAlert variant="error">{errMsg}</InlineAlert>}

      <div className="mb-3 grid grid-cols-2 gap-2.5 sm:grid-cols-4 lg:grid-cols-5">
        <StatTile label="Open Positions" value={positions.length} sub="strategy" />
        <StatTile label="SOL Deployed" value={`◎${formatCompact(totalDeployed, 2)}`} />
        {perStrategy.map((s) => (
          <StatTile
            key={s.strategy_id}
            label={STRATEGY_LABEL[s.strategy_id] ?? s.strategy_id}
            value={`${s.open} open`}
            sub={`◎${formatCompact(s.deployedSol, 2)} · ${s.rules} rule${s.rules === 1 ? '' : 's'}`}
            tone="primary"
          />
        ))}
      </div>

      {isLoading ? (
        <p className="py-10 text-center text-text-dim">Loading open positions…</p>
      ) : (
        <DataTable
          columns={columns}
          rows={positions}
          rowKey={(r) => `${r.strategy_id}:${r.mint_address}:${r.entry_time ?? ''}`}
          loading={isFetching}
          searchable
          colFilters
          hoverable
          tableId="live-trading"
          emptyMessage="No open real positions across strategies."
          selectable={false}
        />
      )}
    </div>
  );
}
