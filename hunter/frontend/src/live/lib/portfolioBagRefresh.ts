/**
 * Bag-level portfolio refresh fan-out — SSOT for "holdings may have changed".
 *
 * `usePortfolioRealtime` is the only SSE listener for bag-changing signals; it
 * invalidates `WalletHoldings` and notifies these subscribers. The Wallet page
 * registers here to reload its imperative table/summary (not RTK-tagged) so we
 * never open a second EventSource filter for the same facts.
 */

type Listener = () => void;

const listeners = new Set<Listener>();

/** Subscribe to coalesced bag-refresh ticks. Returns unsubscribe. */
export function onPortfolioBagRefresh(cb: Listener): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Notify all Wallet/imperative consumers (after RTK invalidate is scheduled). */
export function notifyPortfolioBagRefresh(): void {
  for (const cb of listeners) cb();
}
