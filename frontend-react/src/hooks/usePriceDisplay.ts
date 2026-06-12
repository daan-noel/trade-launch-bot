import { useMemo } from 'react';
import { usePriceUnit } from 'context/PriceUnitContext';
import { formatCompact, formatDecimalTrim, formatPrice, formatUsd } from 'utils/format';

export function usePriceDisplay() {
  const { unit, usdRate } = usePriceUnit();

  // In SOL mode the rate isn't read by any formatter below, so fold it out of
  // the memo key: otherwise every polled SOL/USD-rate tick hands back a fresh
  // object, which rebuilds the consuming column definitions and re-renders the
  // whole token table for nothing. In USD mode the rate genuinely drives the
  // output, so it stays in the key and a tick correctly refreshes prices.
  const rateKey = unit === 'USD' ? usdRate : null;

  return useMemo(
    () => ({
      unit,
      unitLabel: unit,
      displayPrice: (sol: number) =>
        unit === 'SOL'
          ? `◎${formatPrice(sol)}`
          : usdRate != null
            ? formatUsd(sol * usdRate)
            : `◎${formatPrice(sol)}`,
      displayAmount: (sol: number) =>
        unit === 'SOL'
          ? `◎${formatDecimalTrim(sol, 4)}`
          : usdRate != null
            ? formatUsd(sol * usdRate)
            : `◎${formatDecimalTrim(sol, 4)}`,
      displayCompact: (sol: number, digits: number) =>
        unit === 'SOL'
          ? `◎${formatCompact(sol, digits)}`
          : usdRate != null
            ? `$${formatCompact(sol * usdRate, digits)}`
            : `◎${formatCompact(sol, digits)}`,
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [unit, rateKey],
  );
}
