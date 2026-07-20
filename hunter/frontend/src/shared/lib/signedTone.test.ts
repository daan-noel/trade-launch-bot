import { describe, expect, it } from 'vitest';
import {
  formatSigned,
  formatSignedPct,
  pctGradeClass,
  signedStatTone,
  signedToneClass,
  winRateGradeClass,
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

describe('pctGradeClass', () => {
  it('grades by magnitude; null/NaN dim; zero mid', () => {
    expect(pctGradeClass(null)).toBe('text-text-dim');
    expect(pctGradeClass(Number.NaN)).toBe('text-text-dim');
    expect(pctGradeClass(0)).toBe('text-text-mid');
    expect(pctGradeClass(-10)).toBe('text-red');
    expect(pctGradeClass(-50)).toBe('text-red font-bold');
    expect(pctGradeClass(25)).toBe('text-info');
    expect(pctGradeClass(50)).toBe('text-green');
    expect(pctGradeClass(99.9)).toBe('text-green');
    expect(pctGradeClass(100)).toBe('text-warning font-semibold');
    expect(pctGradeClass(200)).toBe('text-accent font-bold');
    expect(pctGradeClass(500)).toBe('text-primary font-extrabold');
  });
});

describe('winRateGradeClass', () => {
  it('grades by 50/75/90 bands; null/NaN dim; 50% mid', () => {
    expect(winRateGradeClass(null)).toBe('text-text-dim');
    expect(winRateGradeClass(Number.NaN)).toBe('text-text-dim');
    expect(winRateGradeClass(0)).toBe('text-red font-bold');
    expect(winRateGradeClass(0.24)).toBe('text-red font-bold');
    expect(winRateGradeClass(0.25)).toBe('text-red');
    expect(winRateGradeClass(0.49)).toBe('text-red');
    expect(winRateGradeClass(0.5)).toBe('text-text-mid');
    expect(winRateGradeClass(0.51)).toBe('text-green');
    expect(winRateGradeClass(0.74)).toBe('text-green');
    expect(winRateGradeClass(0.75)).toBe('text-warning font-semibold');
    expect(winRateGradeClass(0.89)).toBe('text-warning font-semibold');
    expect(winRateGradeClass(0.9)).toBe('text-accent font-bold');
    expect(winRateGradeClass(1)).toBe('text-accent font-bold');
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
