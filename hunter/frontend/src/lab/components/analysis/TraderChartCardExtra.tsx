import { AmountCell, PriceCell } from 'components/tokens/priceCells';
import { Badge } from 'components/ui/Badge';
import { formatDurationShort } from 'utils/format';
import { formatTimestampMs } from 'utils/date';
import { walletHoldSeconds } from './walletPnlStats';
import type { TraderTokenRow } from 'types';

/**
 * Per-mint wallet stats for the Trader Analysis charts-grid card header.
 * Hold = first→last trade span in the window (per-mint grain, not one episode).
 */
export function TraderChartCardExtra({
  row,
  timezone,
}: {
  row: TraderTokenRow;
  timezone: string;
}) {
  const holdSecs = walletHoldSeconds(row);
  const pct = row.wallet_realized_pnl_pct;
  const unrealized = row.wallet_unrealized_pnl_sol;

  return (
    <span className="ml-auto flex flex-col items-end gap-1 rounded-md border border-white/8 bg-white/3 px-2 py-1 text-[11px]">
      <span className="flex flex-wrap items-center justify-end gap-x-2 gap-y-0.5">
        <span className="font-bold uppercase tracking-wide text-text-dim">This wallet</span>
        <span className="text-buy">{row.wallet_buy_count} buys</span>
        <span className="text-sell">{row.wallet_sell_count} sells</span>
        {holdSecs != null && holdSecs > 0 && (
          <span
            className="font-mono text-text"
            title="First→last trade span in the window (not a single episode)"
          >
            hold {formatDurationShort(holdSecs)}
          </span>
        )}
        <span className="text-text-dim">
          last {formatTimestampMs(Date.parse(row.wallet_last_trade_at), timezone)}
        </span>
      </span>
      <span className="flex flex-wrap items-center justify-end gap-x-2 gap-y-0.5">
        <span className={`font-bold ${row.wallet_total_pnl_sol >= 0 ? 'text-green' : 'text-red'}`}>
          <AmountCell sol={row.wallet_total_pnl_sol} /> PnL
          {pct != null && (
            <span className="ml-1 font-mono font-semibold">
              ({pct >= 0 ? '+' : ''}
              {pct.toFixed(1)}%)
            </span>
          )}
        </span>
        <span className="text-text-dim">
          vol <AmountCell sol={row.wallet_buy_sol} />
          {' / '}
          <AmountCell sol={row.wallet_sell_sol} />
        </span>
        {row.wallet_is_open && unrealized != null && (
          <span
            className={unrealized >= 0 ? 'text-green' : 'text-red'}
            title="Mark-to-market on the still-open bag"
          >
            uPnL <AmountCell sol={unrealized} />
          </span>
        )}
        {row.wallet_is_open && (
          <Badge variant="info" size="sm">
            open
          </Badge>
        )}
        {row.wallet_partial_data && (
          <Badge
            variant="warning"
            size="sm"
            title="Sold more than bought in this window — the position predates the look-back, so this PnL is a partial estimate"
          >
            partial
          </Badge>
        )}
      </span>
      <span className="flex flex-wrap items-center justify-end gap-x-2 gap-y-0.5 text-text-dim">
        <span>
          avg buy <PriceCell sol={row.wallet_avg_buy_price} />
          {row.wallet_avg_sell_price != null && (
            <>
              {' '}
              · sell <PriceCell sol={row.wallet_avg_sell_price} />
            </>
          )}
        </span>
      </span>
    </span>
  );
}
