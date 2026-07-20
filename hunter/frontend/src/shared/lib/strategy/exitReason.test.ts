import { describe, expect, it } from 'vitest';
import {
  exitReasonLabel,
  exitReasonSearchText,
  metricsExitLabel,
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
    expect(exitReasonLabel('NoEntry')).toBe('No entry');
    expect(exitReasonLabel('Open')).toBe('Open');
  });

  it('splits Metrics by realized PnL sign', () => {
    expect(exitReasonLabel('Metrics', 0.12)).toBe('METRIC+');
    expect(exitReasonLabel('Metrics', -0.01)).toBe('METRIC-');
    expect(exitReasonLabel('Metrics', 0)).toBe('METRIC');
    expect(exitReasonLabel('Metrics', null)).toBe('METRIC');
  });

  it('ignores pnl for non-Metrics reasons', () => {
    expect(exitReasonLabel('TakeProfit', -1)).toBe('TP');
    expect(exitReasonLabel('StopLoss', 1)).toBe('SL');
  });

  it('treats null/empty as Open, not unknown strings', () => {
    expect(exitReasonLabel(null)).toBe('Open');
    expect(exitReasonLabel(undefined)).toBe('Open');
    expect(exitReasonLabel('')).toBe('Open');
    expect(exitReasonLabel('WeirdReason')).toBe('WeirdReason');
  });
});

describe('metricsExitLabel', () => {
  it('signs only finite non-zero pnl', () => {
    expect(metricsExitLabel(1)).toBe('METRIC+');
    expect(metricsExitLabel(-2)).toBe('METRIC-');
    expect(metricsExitLabel(0)).toBe('METRIC');
    expect(metricsExitLabel(Number.NaN)).toBe('METRIC');
  });
});

describe('exitReasonSearchText', () => {
  it('includes badge abbrev so TP matches TakeProfit rows', () => {
    expect(exitReasonSearchText('TakeProfit')).toContain('TP');
    expect(exitReasonSearchText('TakeProfit')).toContain('TakeProfit');
  });

  it('keeps bare METRIC plus the signed badge for Metrics wins/losses', () => {
    expect(exitReasonSearchText('Metrics', 1)).toBe('Metrics METRIC METRIC+');
    expect(exitReasonSearchText('Metrics', -1)).toBe('Metrics METRIC METRIC-');
    expect(exitReasonSearchText('Metrics', 0)).toBe('Metrics METRIC');
  });

  it('normalizes null open positions to Open', () => {
    expect(exitReasonSearchText(null)).toBe('Open');
  });
});

describe('normalizeExitReasonFilter', () => {
  it('expands badge aliases to persisted reasons', () => {
    expect(normalizeExitReasonFilter('TP')).toBe('TakeProfit');
    expect(normalizeExitReasonFilter('metric')).toBe('Metrics');
    expect(normalizeExitReasonFilter('METRIC+')).toBe('Metrics');
    expect(normalizeExitReasonFilter('metric-')).toBe('Metrics');
    expect(normalizeExitReasonFilter('Open')).toBe('Open');
    expect(normalizeExitReasonFilter('manual')).toBe('Manual');
    expect(normalizeExitReasonFilter('no entry')).toBe('NoEntry');
    expect(normalizeExitReasonFilter('not fired')).toBe('NoEntry');
  });

  it('passes through unknown needles', () => {
    expect(normalizeExitReasonFilter('TakeProf')).toBe('TakeProf');
  });
});
