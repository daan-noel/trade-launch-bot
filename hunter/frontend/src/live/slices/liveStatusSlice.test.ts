import { describe, expect, it } from 'vitest';
import reducer, {
  applyArmedDelta,
  applyPositionDelta,
  applySnapshot,
  armedKey,
  selectRuleOpenCounts,
} from './liveStatusSlice';

const empty = reducer(undefined, { type: '@@init' });

describe('liveStatusSlice', () => {
  it('applySnapshot replaces armed + open and preserves recentClosed', () => {
    const withRecent = reducer(
      empty,
      applyPositionDelta({
        rule_id: 'r1',
        mint_address: 'm1',
        position_id: 'p-old',
        status: 'End',
        exit_reason: 'TakeProfit',
      }),
    );
    const next = reducer(
      withRecent,
      applySnapshot({
        armed: [{ rule_id: 'r1', mint_address: 'm2', state: 'armed' }],
        positions: [
          {
            id: 'p1',
            strategy_id: 'generic',
            rule_id: 'r1',
            mint_address: 'm1',
            mode: 'real',
            status: 'Holding',
            entry_sol: 0.5,
          },
        ],
        ruleNames: { r1: 'Alpha' },
      }),
    );
    expect(Object.keys(next.armed)).toEqual([armedKey('r1', 'm2')]);
    expect(next.open.p1?.ruleName).toBe('Alpha');
    expect(next.open.p1?.entrySol).toBe(0.5);
    expect(next.recentClosed).toHaveLength(1);
    expect(next.hydrated).toBe(true);
  });

  it('Holding → ExitPending → End moves open into recentClosed', () => {
    let s = reducer(
      empty,
      applyPositionDelta({
        rule_id: 'r1',
        mint_address: 'm1',
        position_id: 'p1',
        status: 'Holding',
        trade_mode: 'real',
        rule_name: 'Alpha',
        entry_price: 1,
      }),
    );
    expect(s.open.p1?.status).toBe('Holding');
    s = reducer(
      s,
      applyPositionDelta({
        rule_id: 'r1',
        mint_address: 'm1',
        position_id: 'p1',
        status: 'ExitPending',
        trade_mode: 'real',
      }),
    );
    expect(s.open.p1?.status).toBe('ExitPending');
    s = reducer(
      s,
      applyPositionDelta({
        rule_id: 'r1',
        mint_address: 'm1',
        position_id: 'p1',
        status: 'End',
        exit_reason: 'StopLoss',
        trade_mode: 'real',
      }),
    );
    expect(s.open.p1).toBeUndefined();
    expect(s.recentClosed[0]?.status).toBe('End');
    expect(s.recentClosed[0]?.exitReason).toBe('StopLoss');
  });

  it('armed delta self-heals and buy drops armed row', () => {
    let s = reducer(
      empty,
      applyArmedDelta({
        rule_id: 'r1',
        mint_address: 'm1',
        state: 'armed',
        rule_name: 'Alpha',
        trade_mode: 'real',
      }),
    );
    expect(s.armed[armedKey('r1', 'm1')]?.ruleName).toBe('Alpha');
    s = reducer(
      s,
      applyPositionDelta({
        rule_id: 'r1',
        mint_address: 'm1',
        position_id: 'p1',
        status: 'BuySubmitted',
        trade_mode: 'real',
      }),
    );
    expect(s.armed[armedKey('r1', 'm1')]).toBeUndefined();
    expect(s.open.p1?.status).toBe('BuySubmitted');
  });

  it('selectRuleOpenCounts splits holding vs pending', () => {
    let s = reducer(
      empty,
      applySnapshot({
        armed: [],
        positions: [
          {
            id: 'a',
            strategy_id: 'generic',
            rule_id: 'r1',
            mint_address: 'm1',
            mode: 'real',
            status: 'Holding',
          },
          {
            id: 'b',
            strategy_id: 'generic',
            rule_id: 'r1',
            mint_address: 'm2',
            mode: 'real',
            status: 'BuySubmitted',
          },
        ],
        ruleNames: {},
      }),
    );
    const counts = selectRuleOpenCounts({ liveStatus: s });
    expect(counts.r1).toEqual({ open: 1, pending: 1 });
  });
});
