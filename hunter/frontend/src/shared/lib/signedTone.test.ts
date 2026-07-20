import { describe, expect, it } from 'vitest';
import {
  formatSigned,
  formatSignedPct,
  signedStatTone,
  signedToneClass,
} from './signedTone';

describe('signedToneClass', () => {
  it('colors strict sign; zero and missing stay neutral', () => {
    expect(signedToneClass(1)).toBe('text-green');
    expect(signedToneClass(-0.01)).toBe('text-red');
    expect(signedToneClass(0)).toBe('text-text-mid');
    expect(signedToneClass(null)).toBe('text-text-dim');
    expect(signedToneClass(undefined)).toBe('text-text-dim');
    expect(signedToneClass(Number.NaN)).toBe('text-text-dim');
  });
});

describe('signedStatTone', () => {
  it('maps onto StatTile tones', () => {
    expect(signedStatTone(2)).toBe('green');
    expect(signedStatTone(-2)).toBe('red');
    expect(signedStatTone(0)).toBe('default');
    expect(signedStatTone(null)).toBe('default');
  });
});

describe('formatSigned / formatSignedPct', () => {
  it('prefixes positives only', () => {
    expect(formatSigned(1.25, 2)).toBe('+1.25');
    expect(formatSigned(-1.25, 2)).toBe('-1.25');
    expect(formatSigned(0, 2)).toBe('0');
    expect(formatSignedPct(1.2, 1)).toBe('+1.2%');
    expect(formatSignedPct(-0.5, 1)).toBe('-0.5%');
    expect(formatSignedPct(0, 1)).toBe('0.0%');
  });
});
