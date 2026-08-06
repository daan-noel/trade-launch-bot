import { describe, expect, it } from 'vitest';
import {
  canonicalizeHistoryExitFilter,
  exitReasonMatchesFilter,
  filterClosesForCohort,
  HISTORY_EXIT_FILTER_OPTIONS,
  historyExitFilterToneClass,
  isHistoryMetricExitNeedle,
  seriesStatusAllowsCloses,
} from './historyExitFilter';

describe('HISTORY_EXIT_FILTER_OPTIONS', () => {
  it('offers system reasons and metric-name needles (not legacy ladder aliases)', () => {
    const values = HISTORY_EXIT_FILTER_OPTIONS.map((o) => o.value);
    expect(values).toContain('TakeProfit');
    expect(values).toContain('stall');
    expect(values).toContain('trail');
    expect(values).not.toContain('Trailing');
    expect(values).not.toContain('Stall');
    expect(values).not.toContain('TimeStop');
  });
});

describe('canonicalizeHistoryExitFilter', () => {
  it('rewrites retired ladder needles onto metric names', () => {
    expect(canonicalizeHistoryExitFilter('Trailing')).toBe('trail');
    expect(canonicalizeHistoryExitFilter('Stall')).toBe('stall');
    expect(canonicalizeHistoryExitFilter('TimeStop')).toBe('time');
    expect(canonicalizeHistoryExitFilter('LiquidityExit')).toBe('liquidity');
    expect(canonicalizeHistoryExitFilter('stall')).toBe('stall');
    expect(canonicalizeHistoryExitFilter(null)).toBeNull();
  });
});

describe('exitReasonMatchesFilter', () => {
  it('matches metric detail labels by name substring (case-insensitive)', () => {
    expect(exitReasonMatchesFilter('stall >= 300', 'stall')).toBe(true);
    expect(exitReasonMatchesFilter('trail >= 20', 'trail')).toBe(true);
    expect(exitReasonMatchesFilter('trail >= 20', 'Trailing')).toBe(false);
    expect(exitReasonMatchesFilter('TakeProfit', 'TakeProfit')).toBe(true);
    expect(exitReasonMatchesFilter('stall >= 300', 'TakeProfit')).toBe(false);
    expect(exitReasonMatchesFilter('TrailingStop', 'trail')).toBe(true);
  });

  it('handles synthetic Metric± cohort needles via pnl sign', () => {
    expect(isHistoryMetricExitNeedle('metric_win')).toBe(true);
    expect(exitReasonMatchesFilter('stall >= 300', 'metric_win', 0.1)).toBe(true);
    expect(exitReasonMatchesFilter('stall >= 300', 'metric_win', -0.1)).toBe(false);
    expect(exitReasonMatchesFilter('TakeProfit', 'metric_win', 0.1)).toBe(false);
    expect(exitReasonMatchesFilter('trail >= 12', 'metric_loss', -0.2)).toBe(true);
    expect(exitReasonMatchesFilter('stall >= 1', 'metric', 0)).toBe(true);
  });
});

describe('filterClosesForCohort', () => {
  const sample = [
    { exit_time: '2026-08-01T12:00:00.000Z', exit_reason: 'stall >= 300', pnl_sol: 0.2 },
    { exit_time: '2026-08-02T12:00:00.000Z', exit_reason: 'TakeProfit', pnl_sol: 0.1 },
    { exit_time: '2026-08-03T12:00:00.000Z', exit_reason: 'trail >= 12', pnl_sol: -0.05 },
  ];

  it('applies exit-reason contains and empties on non-End status', () => {
    expect(
      filterClosesForCohort(sample, {
        fromIso: null,
        toIso: null,
        status: null,
        exitReason: 'stall',
      }),
    ).toEqual([sample[0]]);

    expect(
      filterClosesForCohort(sample, {
        fromIso: null,
        toIso: null,
        status: 'Holding',
        exitReason: null,
      }),
    ).toEqual([]);

    expect(seriesStatusAllowsCloses('End')).toBe(true);
    expect(seriesStatusAllowsCloses('EntryFailed')).toBe(false);
  });

  it('filters Metric± synthetic needles using pnl_sol', () => {
    expect(
      filterClosesForCohort(sample, {
        fromIso: null,
        toIso: null,
        status: null,
        exitReason: 'metric_win',
      }),
    ).toEqual([sample[0]]);
    expect(
      filterClosesForCohort(sample, {
        fromIso: null,
        toIso: null,
        status: null,
        exitReason: 'metric_loss',
      }),
    ).toEqual([sample[2]]);
  });
});

describe('historyExitFilterToneClass', () => {
  it('tints metrics as info and system reasons by outcome', () => {
    expect(historyExitFilterToneClass('stall')).toBe('text-info');
    expect(historyExitFilterToneClass('TakeProfit')).toBe('text-green');
    expect(historyExitFilterToneClass('StopLoss')).toBe('text-red');
    expect(historyExitFilterToneClass('Dead')).toBe('text-accent');
  });
});
