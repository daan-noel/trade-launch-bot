import { memo, useMemo } from 'react';
import type { WalletHolding } from 'types';
import { usePriceUnit } from 'context/PriceUnitContext';
import { cn } from 'lib/cn';
import { formatCompact, formatUsd } from 'utils/format';

/**
 * Portfolio header stat row for the Holdings page (Phase 2.2). Totals are derived
 * from the already-priced rows, so the row updates on the 20s Jupiter tick (value
 * moves) without touching the table. Own component so its re-render never forces
 * the DataTable to re-render.
 *
 * Value totals are live (value_usd ticks); cost basis / unrealized PnL are the
 * server-computed SSOT numbers (load-time, refreshed on manual refresh or trade).
 */
function pnlClass(v: number | null): string {
  if (v == null || v === 0) return 'text-text';
  return v > 0 ? 'text-green' : 'text-red';
}

function signed(v: number, digits: number): string {
  return `${v > 0 ? '+' : ''}${formatCompact(v, digits)}`;
}

const Tile = memo(function Tile({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="grid min-h-10 content-center gap-0.5 rounded-md border border-white/5 bg-white/2 px-2.5 py-1">
      <span className="truncate text-[9px] font-semibold uppercase tracking-wider text-text-dim">
        {label}
      </span>
      <span className="font-mono text-sm">{children}</span>
    </div>
  );
});

export const HoldingsSummaryBar = memo(function HoldingsSummaryBar({
  rows,
}: {
  rows: WalletHolding[];
}) {
  const { usdRate } = usePriceUnit();

  const totals = useMemo(() => {
    let totalValueUsd = 0;
    let hasValue = false;
    let weightedChange = 0;
    let changeWeight = 0;
    let totalCostBasisSol = 0;
    let totalUnrealizedSol = 0;
    let hasPnl = false;
    for (const r of rows) {
      if (r.value_usd != null) {
        totalValueUsd += r.value_usd;
        hasValue = true;
        if (r.price_change_24h != null) {
          weightedChange += r.value_usd * r.price_change_24h;
          changeWeight += r.value_usd;
        }
      }
      if (r.cost_basis_sol != null) totalCostBasisSol += r.cost_basis_sol;
      if (r.unrealized_pnl_sol != null) {
        totalUnrealizedSol += r.unrealized_pnl_sol;
        hasPnl = true;
      }
    }
    return {
      positions: rows.length,
      totalValueUsd: hasValue ? totalValueUsd : null,
      change24hPct: changeWeight > 0 ? weightedChange / changeWeight : null,
      totalCostBasisSol,
      totalUnrealizedSol: hasPnl ? totalUnrealizedSol : null,
    };
  }, [rows]);

  const valueSol =
    totals.totalValueUsd != null && usdRate != null && usdRate > 0
      ? totals.totalValueUsd / usdRate
      : null;
  const pnlPct =
    totals.totalUnrealizedSol != null && totals.totalCostBasisSol > 0
      ? (totals.totalUnrealizedSol / totals.totalCostBasisSol) * 100
      : null;

  return (
    <div className="mb-3.5 grid grid-cols-2 gap-2 sm:grid-cols-4">
      <Tile label="Total Value">
        <span className="text-text">
          {valueSol != null ? `◎${formatCompact(valueSol, 2)}` : '—'}
          {totals.totalValueUsd != null && (
            <span className="ml-1.5 text-xs text-text-dim">
              {formatUsd(totals.totalValueUsd)}
            </span>
          )}
        </span>
      </Tile>
      <Tile label="Unrealized PnL">
        {totals.totalUnrealizedSol != null ? (
          <span className={cn('font-semibold', pnlClass(totals.totalUnrealizedSol))}>
            ◎{signed(totals.totalUnrealizedSol, 3)}
            {pnlPct != null && (
              <span className="ml-1 text-xs">({signed(pnlPct, 1)}%)</span>
            )}
          </span>
        ) : (
          <span className="text-text-dim">—</span>
        )}
      </Tile>
      <Tile label="24h Change">
        {totals.change24hPct != null ? (
          <span className={cn('font-semibold', pnlClass(totals.change24hPct))}>
            {signed(totals.change24hPct, 2)}%
          </span>
        ) : (
          <span className="text-text-dim">—</span>
        )}
      </Tile>
      <Tile label="Positions">
        <span className="text-text">{totals.positions}</span>
      </Tile>
    </div>
  );
});
