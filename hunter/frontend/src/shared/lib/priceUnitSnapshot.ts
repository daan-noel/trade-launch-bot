// Module-level mirror of PriceUnitContext so column `filterNumber` accessors
// (plain functions, not hooks) and `toTableRequest` can convert between the
// storage currency and the unit the user currently sees in cells.
//
// PriceUnitProvider is the only writer; column defs / serializers are readers.

import type { PriceUnit } from 'types';

export type AmountStorageUnit = 'sol' | 'usd';

let unit: PriceUnit = 'SOL';
let usdRate: number | null = null;

/** Called by `PriceUnitProvider` whenever unit or rate changes. */
export function setPriceUnitSnapshot(next: { unit: PriceUnit; usdRate: number | null }): void {
  unit = next.unit;
  usdRate = next.usdRate;
}

/**
 * Storage amount → currently displayed unit (for `filterNumber`).
 * When the rate is missing, cells fall back to the storage unit — so do we.
 */
export function amountInDisplayUnit(storage: number, storageUnit: AmountStorageUnit): number {
  const display: AmountStorageUnit = unit === 'USD' ? 'usd' : 'sol';
  if (display === storageUnit || usdRate == null || !(usdRate > 0)) return storage;
  return storageUnit === 'sol' ? storage * usdRate : storage / usdRate;
}

/**
 * User-typed displayed amount → storage unit (for server-side compare).
 * Same no-rate fallback as {@link amountInDisplayUnit}.
 */
export function amountToStorageUnit(displayed: number, storageUnit: AmountStorageUnit): number {
  const display: AmountStorageUnit = unit === 'USD' ? 'usd' : 'sol';
  if (display === storageUnit || usdRate == null || !(usdRate > 0)) return displayed;
  return storageUnit === 'sol' ? displayed / usdRate : displayed * usdRate;
}
