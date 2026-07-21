import { createContext, useContext } from 'react';
import type { PriceUnit } from 'types';

// Context objects + hooks ONLY — no component lives here. Keeping this module
// component-free lets the sibling `PriceUnitProvider.tsx` be a clean React Fast
// Refresh boundary (a module that exports only components). A mixed module
// (provider + hooks together) is an INVALID refresh boundary, so editing any
// descendant would force this module to re-execute, minting fresh `UnitContext`
// / `RateContext` objects while the already-mounted provider still holds the old
// ones — consumers then read `null` and throw "must be used within
// PriceUnitProvider". Splitting the two removes that hazard.

export interface UnitContextValue {
  unit: PriceUnit;
  setUnit: (unit: PriceUnit) => void;
}

export interface RateContextValue {
  usdRate: number | null;
  setUsdRate: (rate: number | null) => void;
}

// Split providers so SOL-mode cells that only read `unit` do not re-render on
// every SOL/USD poll tick (see the memoized values in `PriceUnitProvider`).
export const UnitContext = createContext<UnitContextValue | null>(null);
export const RateContext = createContext<RateContextValue | null>(null);

/** Unit preference only — stable across SOL/USD rate ticks. */
export function usePriceUnitSetting() {
  const ctx = useContext(UnitContext);
  if (!ctx) throw new Error('usePriceUnitSetting must be used within PriceUnitProvider');
  return ctx;
}

/** Live SOL/USD rate only — subscribe when display actually depends on USD. */
export function useUsdRate() {
  const ctx = useContext(RateContext);
  if (!ctx) throw new Error('useUsdRate must be used within PriceUnitProvider');
  return ctx;
}

/** Combined unit + rate (toggle, charts that need both). Prefer the split hooks
 *  on hot paths so SOL mode skips rate-driven renders. */
export function usePriceUnit() {
  const unit = usePriceUnitSetting();
  const rate = useUsdRate();
  return { ...unit, ...rate };
}
