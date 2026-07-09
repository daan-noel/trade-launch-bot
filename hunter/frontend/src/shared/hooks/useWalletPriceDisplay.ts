import { useMemo } from 'react';
import { usePriceUnit } from 'context/PriceUnitContext';
import { formatCompact, formatPrice, formatUsd } from 'utils/format';

/**
 * USD-native counterpart to {@link usePriceDisplay}. Wallet holdings already
 * carry USD values (price_usd / value_usd / liquidity come from Jupiter), so
 * SOL display *divides* by the SOL/USD rate rather than multiplying. Falls back
 * to USD when the rate hasn't loaded yet.
 *
 * Driven by the same global PriceUnit context as the header toggle, so flipping
 * SOL/USD anywhere updates the wallet table too.
 */
export function useWalletPriceDisplay() {
  const { unit, usdRate } = usePriceUnit();

  return useMemo(() => {
    const toSol = (usd: number) =>
      usdRate != null && usdRate > 0 ? usd / usdRate : null;

    return {
      unit,
      /** Per-token unit price (often sub-cent). */
      displayPrice: (usd: number) => {
        if (unit === 'SOL') {
          const sol = toSol(usd);
          if (sol != null) return `◎${formatPrice(sol)}`;
        }
        return formatUsd(usd);
      },
      /** Position value (dollars-scale). */
      displayValue: (usd: number) => {
        if (unit === 'SOL') {
          const sol = toSol(usd);
          if (sol != null) return `◎${formatCompact(sol, 4)}`;
        }
        return formatUsd(usd);
      },
      /** Pool liquidity (large-scale, compact). */
      displayLiquidity: (usd: number) => {
        if (unit === 'SOL') {
          const sol = toSol(usd);
          if (sol != null) return `◎${formatCompact(sol, 2)}`;
        }
        return `$${formatCompact(usd, 2)}`;
      },
    };
  }, [unit, usdRate]);
}
