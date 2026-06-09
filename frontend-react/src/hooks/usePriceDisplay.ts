import { useMemo } from 'react';
import { usePriceUnit } from 'context/PriceUnitContext';
import { formatCompact, formatDecimalTrim, formatPrice, formatUsd } from 'utils/format';

export function usePriceDisplay() {
  const { unit, usdRate } = usePriceUnit();

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
    [unit, usdRate],
  );
}
