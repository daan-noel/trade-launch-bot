import { useMemo } from 'react';
import { Link } from 'react-router-dom';
import { cn } from 'lib/cn';
import { formatSignedPct, signedToneClass } from 'lib/signedTone';
import { formatUsd } from 'utils/format';
import { useGetPortfolioHoldingsQuery } from '@live/store/liveEndpoints';

const TOP_N = 6;

/** Top holdings by value — a glance at where the money sits, linking to the full
 *  position manager. Reuses the same portfolio cache the Holdings page fills. */
export function TopHoldingsWidget() {
  const { data: holdings = [], isLoading } = useGetPortfolioHoldingsQuery();

  const top = useMemo(
    () =>
      [...holdings]
        .filter((h) => (h.value_usd ?? 0) > 0)
        .sort((a, b) => (b.value_usd ?? 0) - (a.value_usd ?? 0))
        .slice(0, TOP_N),
    [holdings],
  );

  return (
    <div className="rounded-lg border border-white/5 bg-white/2 p-3">
      <div className="mb-2 flex items-center justify-between">
        <h2 className="text-sm font-bold text-text">Top Holdings</h2>
        <Link to="/wallet" className="text-[11px] text-accent hover:text-primary hover:underline">
          View all →
        </Link>
      </div>
      {isLoading ? (
        <p className="py-4 text-center text-xs text-text-dim">Loading holdings…</p>
      ) : top.length === 0 ? (
        <p className="py-4 text-center text-xs text-text-dim">No holdings.</p>
      ) : (
        <ul className="flex flex-col gap-1">
          {top.map((h) => {
            const pct = h.unrealized_pnl_pct;
            return (
              <li key={h.mint_address} className="flex items-center justify-between gap-2 text-xs">
                <span className="truncate font-semibold text-text">{h.symbol ?? h.mint_address.slice(0, 6)}</span>
                <span className="flex items-center gap-2 tabular-nums">
                  <span className="text-text-mid">{h.value_usd != null ? formatUsd(h.value_usd) : '—'}</span>
                  {pct != null && (
                    <span className={cn('w-14 text-right font-semibold', signedToneClass(pct))}>
                      {formatSignedPct(pct, 1)}
                    </span>
                  )}
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
