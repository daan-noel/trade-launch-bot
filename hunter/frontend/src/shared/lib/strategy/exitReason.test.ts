import { describe, expect, it } from 'vitest';
import {
  exitReasonLabel,
  exitReasonSearchText,
  normalizeExitReasonFilter,
} from './exitReason';

describe('exitReasonLabel', () => {
  it('maps engine vocabulary to badge labels', () => {
    expect(exitReasonLabel('TakeProfit')).toBe('TP');
    expect(exitReasonLabel('StopLoss')).toBe('SL');
    expect(exitReasonLabel('Metrics')).toBe('METRIC');
    expect(exitReasonLabel('Manual')).toBe('MANUAL');
    expect(exitReasonLabel('Migrated')).toBe('MIG');
    expect(exitReasonLabel('Dead')).toBe('DEAD');
    expect(exitReasonLabel('Open')).toBe('Open');
  });

  it('treats null/empty as Open, not unknown strings', () => {
    expect(exitReasonLabel(null)).toBe('Open');
    expect(exitReasonLabel(undefined)).toBe('Open');
    expect(exitReasonLabel('')).toBe('Open');
    expect(exitReasonLabel('WeirdReason')).toBe('WeirdReason');
  });
});

describe('exitReasonSearchText', () => {
  it('includes badge abbrev so TP matches TakeProfit rows', () => {
    expect(exitReasonSearchText('TakeProfit')).toContain('TP');
    expect(exitReasonSearchText('TakeProfit')).toContain('TakeProfit');
  });

  it('normalizes null open positions to Open', () => {
    expect(exitReasonSearchText(null)).toBe('Open');
  });
});

describe('normalizeExitReasonFilter', () => {
  it('expands badge aliases to persisted reasons', () => {
    expect(normalizeExitReasonFilter('TP')).toBe('TakeProfit');
    expect(normalizeExitReasonFilter('metric')).toBe('Metrics');
    expect(normalizeExitReasonFilter('Open')).toBe('Open');
    expect(normalizeExitReasonFilter('manual')).toBe('Manual');
  });

  it('passes through unknown needles', () => {
    expect(normalizeExitReasonFilter('TakeProf')).toBe('TakeProf');
  });
});
