import type { LiveTrade, TradeRecord } from 'types';

/**
 * Convert a `trade_executed` SSE frame into the REST chart-history row shape.
 * One adapter — charts and any other TradeRecord consumer must not re-map fields.
 *
 * Ordering keys (`tx_index`/`leg_index`) are required for correct live appends;
 * missing values (older frames) fall back to 0 so sort stays deterministic.
 */
export function liveTradeToTradeRecord(t: LiveTrade): TradeRecord {
  const side = t.trade_type === 'sell' ? 'sell' : 'buy';
  const venue =
    t.venue === 'amm' || t.venue === 'curve' ? t.venue : undefined;
  return {
    id: `${t.tx_signature}:${t.leg_index ?? 0}`,
    mint_address: t.mint_address,
    wallet_address: t.wallet,
    trade_type: side,
    amount_sol: t.amount_sol,
    token_amount: t.token_amount,
    price_per_token: t.price_per_token,
    tx_signature: t.tx_signature,
    tx_index: t.tx_index ?? 0,
    leg_index: t.leg_index ?? 0,
    slot: t.slot,
    block_time: t.timestamp,
    reserve_sol: t.reserve_sol ?? null,
    reserve_token: t.reserve_token ?? null,
    venue,
  };
}

/** Dedup key for appends into a TradeRecord[] cache (one leg per signature). */
export function tradeDedupeKey(t: Pick<TradeRecord, 'tx_signature' | 'leg_index'>): string {
  return `${t.tx_signature}:${t.leg_index ?? 0}`;
}
