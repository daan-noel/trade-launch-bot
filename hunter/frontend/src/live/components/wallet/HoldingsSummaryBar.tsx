import { memo } from 'react';
import type { HoldingsTableSummary } from 'services/api';
import { cn } from 'lib/cn';
import { formatSigned, formatSignedPct, pctGradeClass, signedToneClass } from 'lib/signedTone';
import { formatCompact, formatUsd } from 'utils/format';

/**
 * Portfolio header stat row for the Holdings page. Since the Holdings table moved
 * server-side (Phase 4), the totals are computed by the backend over the whole
 * **filtered** population (not just the current page) and handed in as `summary`, so
 * the bar always agrees with the table under any filter / dust toggle / mint set.
 *
 * Values are the composition's scan-time marks (refreshed on the ~8s scan / manual
 * refresh / trade), not the 20s display poll — the poll only overlays fresher
 * per-row display values on the current page.
 */
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
  summary,
}: {
  summary: HoldingsTableSummary;
}) {
  const valueSol = summary.total_value_sol;
  const valueUsd = summary.total_value_usd;
  const unrealizedSol = summary.total_unrealized_pnl_sol;
  const pnlPct =
    unrealizedSol != null && summary.total_cost_basis_sol > 0
      ? (unrealizedSol / summary.total_cost_basis_sol) * 100
      : null;

  return (
    <div className="mb-3.5 grid grid-cols-2 gap-2 sm:grid-cols-4">
      <Tile label="Total Value">
        <span className="text-text">
          {valueSol != null ? `◎${formatCompact(valueSol, 2)}` : '—'}
          {valueUsd != null && (
            <span className="ml-1.5 text-xs text-text-dim">{formatUsd(valueUsd)}</span>
          )}
        </span>
      </Tile>
      <Tile label="Unrealized PnL">
        {unrealizedSol != null ? (
          <span className={cn('font-semibold', signedToneClass(unrealizedSol))}>
            ◎{formatSigned(unrealizedSol, 3)}
            {pnlPct != null && (
              <span className={cn('ml-1 text-xs', pctGradeClass(pnlPct))}>
                ({formatSignedPct(pnlPct, 1)})
              </span>
            )}
          </span>
        ) : (
          <span className="text-text-dim">—</span>
        )}
      </Tile>
      <Tile label="24h Change">
        {summary.change_24h_pct != null ? (
          <span className={pctGradeClass(summary.change_24h_pct)}>
            {formatSignedPct(summary.change_24h_pct, 2)}
          </span>
        ) : (
          <span className="text-text-dim">—</span>
        )}
      </Tile>
      <Tile label="Positions">
        <span className="text-text">{summary.positions}</span>
      </Tile>
    </div>
  );
});
