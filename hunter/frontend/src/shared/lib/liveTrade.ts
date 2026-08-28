import { normalizeIxLabels } from 'lib/ixLabels';
import type { LiveTrade, TokenLiveStats, TradeRecord } from 'types';

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
    // `?? null` (not `?? 0`): a frame from a bin predating the field carries no
    // fee, and the Fee column must render that as "—", never as a free trade.
    fee_sol: t.fee_sol ?? null,
    // Same `?? null` reasoning as `fee_sol`, with one extra trap: for
    // `tip_lamports` a real 0 means "transfers, none to a recognised tip account"
    // and must survive. `??` keeps it (only null/undefined fall through); `||`
    // would collapse it to null and erase the one state that measures how far
    // behind the decoder's tip-account list has fallen.
    cu_limit: t.cu_limit ?? null,
    cu_price: t.cu_price ?? null,
    tip_lamports: t.tip_lamports ?? null,
    tx_signature: t.tx_signature,
    tx_index: t.tx_index ?? 0,
    leg_index: t.leg_index ?? 0,
    slot: t.slot,
    block_time: t.timestamp,
    reserve_sol: t.reserve_sol ?? null,
    reserve_token: t.reserve_token ?? null,
    venue,
    // The chart classifies vol / non-vol from these labels client-side, so an
    // appended row that drops them is counted as non-vol AND fails to tag its
    // wallet — the cumulative pair then diverges from that trade onward, which is
    // why the overlay was wrong on a still-open position and right once it closed.
    // Normalized here for the same reason the REST twin (`getTokenTrades`) does it:
    // both persisted `ix_labels` shapes have to reach readers as an ordered list.
    instruction_labels: normalizeIxLabels(t.instruction_labels),
  };
}

/**
 * Apply a `trade_executed` frame's `live` stats snapshot onto a cached token row.
 *
 * The ONE writer of pushed stats into a cache entry — the token grid's rows and
 * the chart's `getTokenDetail` share it so the two surfaces can't drift on which
 * fields tick. Field names are identical across `TokenLiveStats`, `TokenRecord`
 * and `TokenDetailRecord` (the backend `live_stats` mirrors them), so the target
 * is typed structurally: adding a stat is one edit here, not one per call site.
 *
 * The snapshot is cumulative and backend-authoritative, so last-frame-wins is
 * correct — no per-field max is needed to keep `ath_price` monotone.
 */
export function applyTokenLiveStats(target: TokenLiveStats, s: TokenLiveStats): void {
  target.current_price = s.current_price;
  target.volume_sol_total = s.volume_sol_total;
  target.market_cap = s.market_cap;
  target.trade_count = s.trade_count;
  target.ath_price = s.ath_price;
  target.ath_timestamp = s.ath_timestamp;
  target.last_trade_at = s.last_trade_at;
}

/** Dedup key for appends into a TradeRecord[] cache (one leg per signature). */
export function tradeDedupeKey(t: Pick<TradeRecord, 'tx_signature' | 'leg_index'>): string {
  return `${t.tx_signature}:${t.leg_index ?? 0}`;
}
