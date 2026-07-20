import { useMemo, useRef, useState } from 'react';
import type { LiveTrade } from 'types';
import { cn } from 'lib/cn';
import { formatCompact } from 'utils/format';
import { useMintTradeStream } from 'hooks/useMintTradeStream';
import { useGetPortfolioHoldingsQuery } from '@live/store/liveEndpoints';

const MAX_ROWS = 15;

/**
 * Recent live trades on the mints you hold — the `trade_executed` SSE filtered to
 * the held-mint set (via {@link useMintTradeStream}). The feed owns its own state
 * so its high-frequency updates never re-render the KPI row or the holdings widget.
 */
export function LiveTradeFeed() {
  const { data: holdings = [] } = useGetPortfolioHoldingsQuery();
  // Held mint → symbol, read through a ref so the SSE handler always sees the
  // latest set without re-subscribing on every holdings refresh.
  const heldMap = useMemo(
    () => new Map(holdings.map((h) => [h.mint_address, h.symbol ?? undefined])),
    [holdings],
  );
  const heldRef = useRef(heldMap);
  heldRef.current = heldMap;

  const mintsRef = useRef<Set<string>>(new Set());
  mintsRef.current = new Set(heldMap.keys());

  const [trades, setTrades] = useState<LiveTrade[]>([]);

  useMintTradeStream(mintsRef, (batch) => {
    // Newest first in the feed.
    const reversed = [...batch].reverse();
    setTrades((prev) => [...reversed, ...prev].slice(0, MAX_ROWS));
  }, 400);

  return (
    <div className="rounded-lg border border-white/5 bg-white/2 p-3">
      <h2 className="mb-2 text-sm font-bold text-text">Live Trades · your holdings</h2>
      {trades.length === 0 ? (
        <p className="py-4 text-center text-xs text-text-dim">
          Waiting for trades on your held mints…
        </p>
      ) : (
        <ul className="flex flex-col gap-1">
          {trades.map((t) => {
            const buy = t.trade_type === 'buy';
            const symbol = heldRef.current.get(t.mint_address) ?? t.mint_address.slice(0, 6);
            return (
              <li
                key={`${t.tx_signature}:${t.leg_index ?? 0}`}
                className="flex items-center justify-between gap-2 text-xs tabular-nums"
              >
                <span className="flex items-center gap-2">
                  <span
                    className={cn(
                      'w-9 rounded px-1 text-center text-[10px] font-bold',
                      buy ? 'bg-buy/12 text-buy' : 'bg-sell/12 text-sell',
                    )}
                  >
                    {buy ? 'BUY' : 'SELL'}
                  </span>
                  <span className="truncate font-semibold text-text">{symbol}</span>
                </span>
                <span className="text-text-mid">◎{formatCompact(t.amount_sol, 3)}</span>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
