import { describe, expect, it } from 'vitest';

import {
  amountInDisplayUnit,
  amountToStorageUnit,
  setPriceUnitSnapshot,
} from './priceUnitSnapshot';

describe('priceUnitSnapshot amount conversion', () => {
  it('is identity when display unit matches storage', () => {
    setPriceUnitSnapshot({ unit: 'SOL', usdRate: 150 });
    expect(amountInDisplayUnit(2, 'sol')).toBe(2);
    expect(amountToStorageUnit(2, 'sol')).toBe(2);

    setPriceUnitSnapshot({ unit: 'USD', usdRate: 150 });
    expect(amountInDisplayUnit(300, 'usd')).toBe(300);
    expect(amountToStorageUnit(300, 'usd')).toBe(300);
  });

  it('converts SOL storage ↔ USD display', () => {
    setPriceUnitSnapshot({ unit: 'USD', usdRate: 150 });
    expect(amountInDisplayUnit(2, 'sol')).toBe(300);
    expect(amountToStorageUnit(300, 'sol')).toBe(2);
  });

  it('converts USD storage ↔ SOL display', () => {
    setPriceUnitSnapshot({ unit: 'SOL', usdRate: 150 });
    expect(amountInDisplayUnit(300, 'usd')).toBe(2);
    expect(amountToStorageUnit(2, 'usd')).toBe(300);
  });

  it('falls back to storage when rate is missing', () => {
    setPriceUnitSnapshot({ unit: 'USD', usdRate: null });
    expect(amountInDisplayUnit(2, 'sol')).toBe(2);
    expect(amountToStorageUnit(300, 'sol')).toBe(300);
  });
});
