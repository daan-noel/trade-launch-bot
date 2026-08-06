import { describe, expect, it } from 'vitest';
import type { ClosedTradePoint } from '@live/store/liveEndpoints';
import {
  activeExitTileLabel,
  closeToRunOutcome,
  exitTileClickAction,
  exitTileToHistoryNeedle,
  historyRunSummaryFromCloses,
} from './historyExitSummary';

function pt(partial: Partial<ClosedTradePoint> & Pick<ClosedTradePoint, 'id'>): ClosedTradePoint {
  return {
    exit_time: '2026-08-01T12:00:00.000Z',
    rule_id: null,
    mint_address: 'Mint111',
    pnl_sol: 0.1,
    entry_sol: 1,
    win: true,
    hold_secs: 30,
    exit_reason: 'TakeProfit',
    ...partial,
  };
}

describe('exitTileToHistoryNeedle', () => {
  it('maps system tiles and Metric± to hexit needles (composable with hfocus)', () => {
    expect(exitTileToHistoryNeedle('Take profit')).toBe('TakeProfit');
    expect(exitTileToHistoryNeedle('Trailing')).toBe('trail');
    expect(exitTileToHistoryNeedle('Metric+')).toBe('metric_win');
    expect(exitTileToHistoryNeedle('Metric-')).toBe('metric_loss');
    expect(exitTileToHistoryNeedle('Other')).toBeNull();
    expect(exitTileToHistoryNeedle('stall > 300')).toBe('stall > 300');
    expect(exitTileClickAction('Metric+')).toEqual({
      channel: 'filter',
      needle: 'metric_win',
    });
  });
});

describe('activeExitTileLabel', () => {
  it('reads synthetic needles and legacy exit focus', () => {
    expect(activeExitTileLabel('TakeProfit', null)).toBe('Take profit');
    expect(activeExitTileLabel('metric_win', null)).toBe('Metric+');
    expect(activeExitTileLabel('stall', null)).toBe('Stall');
    expect(activeExitTileLabel('pnl', null)).toBeNull();
    expect(activeExitTileLabel('stall > 300', null)).toBe('stall > 300');
    expect(activeExitTileLabel(null, { kind: 'exit', tile: 'Metric-' })).toBe('Metric-');
  });
});

describe('historyRunSummaryFromCloses', () => {
  it('splits metric exits by PnL and counts system reasons', () => {
    const summary = historyRunSummaryFromCloses([
      pt({ id: '1', exit_reason: 'TakeProfit', pnl_sol: 0.2 }),
      pt({ id: '2', exit_reason: 'stall >= 300', pnl_sol: 0.1, win: true }),
      pt({ id: '3', exit_reason: 'trail >= 12', pnl_sol: -0.05, win: false }),
      pt({ id: '4', exit_reason: 'Migrated', pnl_sol: 0.01 }),
    ]);
    const r = summary.realized;
    expect(r.n_closed).toBe(4);
    expect(r.n_exit_take_profit).toBe(1);
    expect(r.n_exit_metrics_win).toBe(1);
    expect(r.n_exit_metrics_loss).toBe(1);
    expect(r.n_exit_manual).toBe(0);
  });

  it('maps hold_secs and pnl% onto the outcome row', () => {
    expect(
      closeToRunOutcome(
        pt({ id: 'x', entry_sol: 2, pnl_sol: 0.5, hold_secs: 90, exit_reason: 'Dead' }),
      ),
    ).toEqual({
      fired: true,
      exit: 'Dead',
      pnl_sol: 0.5,
      pnl_pct: 25,
      holding_secs: 90,
    });
  });
});
