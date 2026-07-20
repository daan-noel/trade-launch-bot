import { memo } from 'react';
import type { HoldingsTableSummary } from 'services/api';
import { StatTile } from 'components/ui/StatTile';
import { cn } from 'lib/cn';
import { formatSigned, formatSignedPct, pctGradeClass, signedToneClass } from 'lib/signedTone';
import { formatCompact, formatUsd } from 'utils/format';

/**
 * Wallet header optimized for glanceability:
 *   1. Hero funding tiles — SOL · USDC · Available (large)
 *   2. Trading strip — positions / unrealized / 24h (secondary)
 *
 * Cash is a balance-sheet asset, never a token-table row. Funding is unfiltered;
 * position metrics match the filtered meme table. Values are scan-time marks.
 */

export const HoldingsSummaryBar = memo(function HoldingsSummaryBar({
  summary,
}: {
  summary: HoldingsTableSummary;
}) {
  const solBal = summary.sol_balance_sol;
  const solBalUsd = summary.sol_balance_usd;
  const cashUsd = summary.cash_value_usd;
  const cashSol = summary.cash_value_sol;
  const cashLines = summary.cash_holdings ?? [];
  const usdc = cashLines.find((h) => h.symbol === 'USDC') ?? cashLines[0];
  const posSol = summary.positions_value_sol;
  const posUsd = summary.positions_value_usd;
  const unrealizedSol = summary.total_unrealized_pnl_sol;
  const pnlPct =
    unrealizedSol != null && summary.total_cost_basis_sol > 0
      ? (unrealizedSol / summary.total_cost_basis_sol) * 100
      : null;

  const hasCash = (cashUsd != null && cashUsd > 0) || (usdc != null && usdc.ui_amount > 0);
  const usdcUsd = hasCash ? (usdc?.value_usd ?? cashUsd) : null;
  const usdcSol = hasCash ? (usdc?.value_sol ?? cashSol) : null;

  const fundingUsd =
    solBalUsd != null || usdcUsd != null
      ? (solBalUsd ?? 0) + (usdcUsd ?? 0)
      : null;

  return (
    <div className="mb-4 grid gap-2.5">
      {/* Hero balances — the first thing the eye hits. */}
      <div className="grid grid-cols-2 gap-2 lg:grid-cols-3">
        <StatTile
          label="SOL"
          size="md"
          bold
          value={solBal != null ? `◎${formatCompact(solBal, 3)}` : '—'}
          sub={solBalUsd != null ? formatUsd(solBalUsd) : undefined}
        />
        <StatTile
          label="USDC"
          size="md"
          bold
          tone={hasCash ? 'default' : 'muted'}
          value={usdcUsd != null ? formatUsd(usdcUsd) : '—'}
          sub={
            hasCash
              ? [
                  usdc != null ? `${formatCompact(usdc.ui_amount, 2)} USDC` : null,
                  usdcSol != null ? `≈ ◎${formatCompact(usdcSol, 2)}` : null,
                ]
                  .filter(Boolean)
                  .join(' · ') || undefined
              : 'No cash'
          }
        />
        <div className="col-span-2 lg:col-span-1">
          <StatTile
            label="Available"
            size="md"
            bold
            tone="primary"
            value={fundingUsd != null ? formatUsd(fundingUsd) : '—'}
            sub="SOL + USDC dry powder"
          />
        </div>
      </div>

      {/* Secondary trading book — dense, not competing with balances. */}
      <div className="flex flex-wrap items-baseline gap-x-5 gap-y-1 border-t border-white/5 pt-2">
        <span className="text-[9px] font-semibold uppercase tracking-wider text-text-dim">
          Positions
        </span>
        <span className="font-mono text-sm tabular-nums text-text">
          {posSol != null ? `◎${formatCompact(posSol, 2)}` : '—'}
          {posUsd != null && (
            <span className="ml-1.5 text-xs text-text-dim">{formatUsd(posUsd)}</span>
          )}
          <span className="ml-1.5 text-xs text-text-dim">×{summary.positions}</span>
        </span>
        <span className="text-text-dim" aria-hidden>
          ·
        </span>
        <span className="text-[9px] font-semibold uppercase tracking-wider text-text-dim">
          Unrealized
        </span>
        {unrealizedSol != null ? (
          <span
            className={cn(
              'font-mono text-sm font-semibold tabular-nums',
              signedToneClass(unrealizedSol),
            )}
          >
            ◎{formatSigned(unrealizedSol, 3)}
            {pnlPct != null && (
              <span className={cn('ml-1 text-xs', pctGradeClass(pnlPct))}>
                ({formatSignedPct(pnlPct, 1)})
              </span>
            )}
          </span>
        ) : (
          <span className="font-mono text-sm text-text-dim">—</span>
        )}
        <span className="text-text-dim" aria-hidden>
          ·
        </span>
        <span className="text-[9px] font-semibold uppercase tracking-wider text-text-dim">
          24h
        </span>
        {summary.change_24h_pct != null ? (
          <span
            className={cn(
              'font-mono text-sm tabular-nums',
              pctGradeClass(summary.change_24h_pct),
            )}
          >
            {formatSignedPct(summary.change_24h_pct, 2)}
          </span>
        ) : (
          <span className="font-mono text-sm text-text-dim">—</span>
        )}
      </div>
    </div>
  );
});
