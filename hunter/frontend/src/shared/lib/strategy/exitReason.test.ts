import { describe, expect, it } from 'vitest';
import {
  exitReasonLabel,
  exitReasonSearchText,
  formatMetricExitLabel,
  isMetricExitReason,
  metricsExitLabel,
  normalizeExitReasonFilter,
  parseMetricExitParts,
  parseMetricExitTarget,
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

  it('shows spaced metric detail forms as stored', () => {
    expect(exitReasonLabel('stall > 3')).toBe('stall > 3');
    expect(exitReasonLabel('trail >= 20', 1)).toBe('trail >= 20');
    expect(exitReasonLabel('untagged_gross = 0')).toBe('untagged_gross = 0');
  });

  it('splits legacy Metrics by realized PnL sign', () => {
    expect(exitReasonLabel('Metrics', 0.12)).toBe('METRIC+');
    expect(exitReasonLabel('Metrics', -0.01)).toBe('METRIC-');
  });
});

describe('parseMetricExitParts', () => {
  it('splits spaced name op value', () => {
    expect(parseMetricExitParts('stall > 3')).toEqual({
      name: 'stall',
      op: '>',
      value: '3',
    });
    expect(parseMetricExitParts('trail >= 20.5')).toEqual({
      name: 'trail',
      op: '>=',
      value: '20.5',
    });
  });

  it('accepts legacy compact forms', () => {
    expect(parseMetricExitParts('stall>')).toEqual({
      name: 'stall',
      op: '>',
      value: '',
    });
  });

  // The engine writes the window qualifier into the persisted reason
  // (`event::format_metric_exit_name`). A parser that does not accept it returns
  // null, and the row stops reading as a metric exit at all.
  it('keeps the window qualifier on the name, in every unit', () => {
    expect(parseMetricExitParts('untagged_buy(2s) >= 0.9')).toEqual({
      name: 'untagged_buy(2s)',
      op: '>=',
      value: '0.9',
    });
    expect(parseMetricExitParts('buy_count(30sl) >= 3')).toEqual({
      name: 'buy_count(30sl)',
      op: '>=',
      value: '3',
    });
    expect(parseMetricExitParts('buy_count(30sl@1) >= 3')).toEqual({
      name: 'buy_count(30sl@1)',
      op: '>=',
      value: '3',
    });
  });
});

describe('parseMetricExitTarget', () => {
  it('reads the whole span, not just its size', () => {
    expect(parseMetricExitTarget('untagged_buy(2s) >= 0.9')).toEqual({
      metric: 'untagged_buy',
      window: { size: 2, lag: 0, unit: 'sec' },
    });
    expect(parseMetricExitTarget('buy_count(30sl@1) >= 3')).toEqual({
      metric: 'buy_count',
      window: { size: 30, lag: 1, unit: 'slot' },
    });
  });

  it('leaves a bare name unqualified, and a non-metric reason unparsed', () => {
    expect(parseMetricExitTarget('stall > 3')).toEqual({ metric: 'stall', window: null });
    expect(parseMetricExitTarget('TakeProfit')).toBeNull();
  });
});

describe('formatMetricExitLabel', () => {
  it('uses spaced name op value', () => {
    expect(formatMetricExitLabel('stall', '>', 3)).toBe('stall > 3');
    expect(formatMetricExitLabel('untagged_gross', '=', 0)).toBe('untagged_gross = 0');
  });
});

describe('isMetricExitReason', () => {
  it('accepts legacy Metrics and detail forms', () => {
    expect(isMetricExitReason('Metrics')).toBe(true);
    expect(isMetricExitReason('stall > 3')).toBe(true);
    expect(isMetricExitReason('stall>')).toBe(true);
    expect(isMetricExitReason('TakeProfit')).toBe(false);
    expect(isMetricExitReason('Stall')).toBe(false);
  });
});

describe('metricsExitLabel', () => {
  it('signs only finite non-zero pnl', () => {
    expect(metricsExitLabel(1)).toBe('METRIC+');
    expect(metricsExitLabel(-2)).toBe('METRIC-');
    expect(metricsExitLabel(0)).toBe('METRIC');
  });
});

describe('exitReasonSearchText', () => {
  it('indexes spaced metric parts', () => {
    const hay = exitReasonSearchText('stall > 3', -1);
    expect(hay).toContain('stall');
    expect(hay).toContain('>');
    expect(hay).toContain('3');
    expect(hay).toContain('METRIC');
  });
});

describe('normalizeExitReasonFilter', () => {
  it('expands badge aliases and passes detail needles through', () => {
    expect(normalizeExitReasonFilter('TP')).toBe('TakeProfit');
    expect(normalizeExitReasonFilter('metric')).toBe('Metrics');
    expect(normalizeExitReasonFilter('stall > 3')).toBe('stall > 3');
  });
});
