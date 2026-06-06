import { useMemo } from 'react';
import { usePriceUnit } from 'context/PriceUnitContext';
import {
  formatCompact,
  formatDecimalTrim,
  formatPrice,
  formatWithCommas,
} from 'utils/format';

function formatUsd(value: number): string {
  if (value === 0) return '$0';
  const abs = Math.abs(value);
  const sign = value < 0 ? '-' : '';
  if (abs < 0.01) return `${sign}$${formatPrice(abs)}`;
  const rounded = Math.round(abs * 100) / 100;
  const whole = Math.trunc(rounded);
  const frac = Math.round((rounded - whole) * 100);
  const wholeStr = formatWithCommas(whole);
  if (frac === 0) return `${sign}$${wholeStr}`;
  return `${sign}$${wholeStr}.${String(frac).padStart(2, '0')}`;
}

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
