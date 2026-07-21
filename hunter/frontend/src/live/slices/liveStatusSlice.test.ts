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
  it('applySnapshot replaces armed + open and hydrates recentClosed from DB', () => {
    const withSession = reducer(
      empty,
      applyPositionDelta({
        rule_id: 'r1',
        mint_address: 'm1',
        position_id: 'p-session',
        status: 'End',
        exit_reason: 'TakeProfit',
      }),
    );
    const next = reducer(
      withSession,
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
        recentClosed: [
          {
            id: 'p-db',
            rule_id: 'r1',
            mint_address: 'm9',
            mode: 'real',
            status: 'End',
            exit_reason: 'StopLoss',
            exit_time: '2026-07-20T12:00:00.000Z',
          },
        ],
        ruleNames: { r1: 'Alpha' },
      }),
    );
    expect(Object.keys(next.armed)).toEqual([armedKey('r1', 'm2')]);
    expect(next.open.p1?.ruleName).toBe('Alpha');
    expect(next.open.p1?.entrySol).toBe(0.5);
    expect(next.recentClosed.map((r) => r.positionId)).toContain('p-db');
    expect(next.recentClosed.map((r) => r.positionId)).toContain('p-session');
    expect(next.hydrated).toBe(true);
  });

  it('applySnapshot drops armed that collide with open (rule, mint)', () => {
    const next = reducer(
      empty,
      applySnapshot({
        armed: [{ rule_id: 'r1', mint_address: 'm1', state: 'armed' }],
        positions: [
          {
            id: 'p1',
            strategy_id: 'generic',
            rule_id: 'r1',
            mint_address: 'm1',
            mode: 'real',
            status: 'Holding',
          },
        ],
        ruleNames: {},
      }),
    );
    expect(next.armed[armedKey('r1', 'm1')]).toBeUndefined();
    expect(next.open.p1?.status).toBe('Holding');
  });

  it('ignores position deltas with empty or nil position_id', () => {
    const withOpen = reducer(
      empty,
      applyPositionDelta({
        rule_id: 'r1',
        mint_address: 'm1',
        position_id: 'p1',
        status: 'Holding',
        trade_mode: 'real',
      }),
    );
    const ignored = reducer(
      withOpen,
      applyPositionDelta({
        rule_id: 'r1',
        mint_address: 'm1',
        position_id: '00000000-0000-0000-0000-000000000000',
        status: 'End',
        trade_mode: 'real',
      }),
    );
    expect(ignored.open.p1?.status).toBe('Holding');
    expect(ignored.recentClosed).toEqual([]);
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
