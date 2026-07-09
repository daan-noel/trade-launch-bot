import { useMemo } from 'react';
import { Link } from 'react-router-dom';
import { formatCompact } from 'utils/format';
import { useGetPortfolioPositionsQuery } from '@live/store/liveEndpoints';

/** Friendly labels for the canonical strategy ids. */
const STRATEGY_LABEL: Record<string, string> = {
  tpsl_sniper_1: 'TPSL1',
  tpsl_sniper_2: 'TPSL2',
  swing1: 'Swing1',
};

interface StrategyRow {
  strategy_id: string;
  open: number;
  deployedSol: number;
}

/** Compact per-strategy real-money strip: open positions + SOL deployed per
 *  strategy, linking to the cross-strategy Live-Trading monitor. */
export function StrategyStrip() {
  const { data: positions = [], isLoading } = useGetPortfolioPositionsQuery(true);

  const rows = useMemo<StrategyRow[]>(() => {
    const byStrategy = new Map<string, StrategyRow>();
    for (const p of positions) {
      const row =
        byStrategy.get(p.strategy_id) ??
        { strategy_id: p.strategy_id, open: 0, deployedSol: 0 };
      row.open += 1;
      row.deployedSol += p.entry_sol ?? 0;
      byStrategy.set(p.strategy_id, row);
    }
    return [...byStrategy.values()].sort((a, b) => b.open - a.open);
  }, [positions]);

  return (
    <div className="rounded-lg border border-white/5 bg-white/2 p-3">
      <div className="mb-2 flex items-center justify-between">
        <h2 className="text-sm font-bold text-text">Real Trading · by strategy</h2>
        <Link
          to="/live-trading"
          className="text-[11px] text-accent hover:text-primary hover:underline"
        >
          Monitor →
        </Link>
      </div>
      {isLoading ? (
        <p className="py-4 text-center text-xs text-text-dim">Loading positions…</p>
      ) : rows.length === 0 ? (
        <p className="py-4 text-center text-xs text-text-dim">No open real positions.</p>
      ) : (
        <div className="grid grid-cols-3 gap-2">
          {rows.map((r) => (
            <div key={r.strategy_id} className="rounded-md border border-white/5 bg-white/2 px-2.5 py-1.5">
              <div className="text-[11px] font-semibold text-text">
                {STRATEGY_LABEL[r.strategy_id] ?? r.strategy_id}
              </div>
              <div className="mt-0.5 flex items-baseline justify-between gap-1 text-xs tabular-nums">
                <span className="text-primary">{r.open} open</span>
                <span className="text-text-dim">◎{formatCompact(r.deployedSol, 2)}</span>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
