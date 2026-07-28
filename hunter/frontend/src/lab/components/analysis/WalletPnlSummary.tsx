import { memo } from 'react';
import { StatTile, type StatTone } from 'components/ui/StatTile';
import { AmountCell } from 'components/tokens/priceCells';
import { formatWithCommas } from 'utils/format';
import type { WalletPnlSummary } from './walletPnlStats';

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

/**
 * At-a-glance verdict row for the wallet currently under analysis — the
 * headline numbers the wallet-analysis reverse-engineering docs compute by hand
 * in SQL (gross vs fee-adjusted net, payoff ratio over win rate, mark-to-market
 * total). Pure display over {@link WalletPnlSummary}; no fetching here.
 */
export const WalletPnlSummaryRow = memo(function WalletPnlSummaryRow({
  summary,
}: {
  summary: WalletPnlSummary;
}) {
  return (
    <div className="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-8">
      <StatTile
        label="Realized PnL (gross)"
        value={<AmountCell sol={summary.totalRealizedPnlSol} />}
        tone={signTone(summary.totalRealizedPnlSol)}
      />
      <StatTile
        label="Realized PnL (net of fee)"
        value={<AmountCell sol={summary.totalRealizedPnlSolNetOfFee} />}
        tone={signTone(summary.totalRealizedPnlSolNetOfFee)}
        sub="~125bps/leg pump.fun fee"
      />
      <StatTile
        label="Unrealized (open bags)"
        value={<AmountCell sol={summary.totalUnrealizedPnlSol} />}
        tone={signTone(summary.totalUnrealizedPnlSol)}
        sub={`${summary.openCount} open`}
      />
      <StatTile
        label="Total (mark-to-market)"
        value={<AmountCell sol={summary.totalPnlSol} />}
        tone={signTone(summary.totalPnlSol)}
        bold
      />
      <StatTile
        label="Win rate"
        value={pct(summary.winRate)}
        sub={`${summary.winCount}W / ${summary.lossCount}L`}
      />
      <StatTile
        label="Avg win / loss"
        value={
          <>
            <AmountCell sol={summary.avgWinSol} /> / <AmountCell sol={summary.avgLossSol} />
          </>
        }
      />
      <StatTile label="Payoff ratio" value={ratio(summary.payoffRatio)} sub="avg win / |avg loss|" />
      <StatTile
        label="Volume traded"
        value={<AmountCell sol={summary.totalVolumeSol} />}
        sub={`${formatWithCommas(summary.tokenCount)} tokens${summary.partialDataCount > 0 ? ` · ${summary.partialDataCount} partial` : ''}`}
      />
    </div>
  );
});
