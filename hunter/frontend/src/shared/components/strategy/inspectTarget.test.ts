import { describe, expect, it } from 'vitest';
import {
  buildEventMarkers,
  buildEventMarkersForEpisodes,
  episodeRowKey,
  inspectFromPosition,
  parseEpisodeRowKey,
  type InspectTarget,
} from './inspectTarget';
import type { RulePositionRecord } from 'types';

function target(over: Partial<InspectTarget>): InspectTarget {
  return {
    mint_address: 'mint',
    entryTime: null,
    entryPrice: null,
    exitTime: null,
    exitPrice: null,
    ...over,
  };
}

describe('episodeRowKey', () => {
  it('keys a fired episode by mint + entry_time (unique per re-entry)', () => {
    expect(episodeRowKey({ mint_address: 'AAA', entry_time: '2026-07-20T22:39:35Z' })).toBe(
      'AAA::2026-07-20T22:39:35Z',
    );
  });

  it('separates two episodes of the same mint', () => {
    const a = episodeRowKey({ mint_address: 'AAA', entry_time: '2026-07-20T22:39:35Z' });
    const b = episodeRowKey({ mint_address: 'AAA', entry_time: '2026-07-20T22:42:59Z' });
    expect(a).not.toBe(b);
  });

  it('falls back to mint alone for a never-entered row', () => {
    expect(episodeRowKey({ mint_address: 'AAA', entry_time: null })).toBe('AAA');
    expect(episodeRowKey({ mint_address: 'AAA' })).toBe('AAA');
  });

  it('parseEpisodeRowKey round-trips', () => {
    expect(parseEpisodeRowKey('AAA::2026-07-20T22:39:35Z')).toEqual({
      mint_address: 'AAA',
      entry_time: '2026-07-20T22:39:35Z',
    });
    expect(parseEpisodeRowKey('AAA')).toEqual({ mint_address: 'AAA', entry_time: null });
  });
});

describe('buildEventMarkersForEpisodes', () => {
  it('leaves a single episode unnumbered', () => {
    const markers = buildEventMarkersForEpisodes([
      target({ entryTime: '2026-07-20T22:39:35Z', entryPrice: 1, exitTime: '2026-07-20T22:42:14Z', exitPrice: 2 }),
    ]);
    expect(markers.map((m) => m.label)).toEqual(['Entry', 'Exit']);
  });

  it('orders episodes by entry time and numbers every marker', () => {
    const later = target({
      entryTime: '2026-07-20T22:42:59Z',
      entryPrice: 3,
      exitTime: '2026-07-20T22:47:23Z',
      exitPrice: 4,
    });
    const earlier = target({
      entryTime: '2026-07-20T22:39:35Z',
      entryPrice: 1,
      exitTime: '2026-07-20T22:42:14Z',
      exitPrice: 2,
    });
    // Pass out of order to prove it sorts by entry time.
    const markers = buildEventMarkersForEpisodes([later, earlier]);
    expect(markers.map((m) => m.label)).toEqual(['Entry 1', 'Exit 1', 'Entry 2', 'Exit 2']);
    // Episode 1 is the earlier fill.
    const entry1 = markers.find((m) => m.label === 'Entry 1');
    expect(entry1?.priceInSol).toBe(1);
  });

  it('tags the focused episode so it is identifiable among its siblings', () => {
    const first = target({ entryTime: '2026-07-20T22:39:35Z', entryPrice: 1 });
    const second = target({ entryTime: '2026-07-20T22:42:59Z', entryPrice: 3 });
    const labels = buildEventMarkersForEpisodes([first, second], second).map((m) => m.label);
    expect(labels).toEqual(['Entry 1', 'Entry 2 ◂']);
  });

  it('does not tag anything when the mint has a single episode', () => {
    const only = target({ entryTime: '2026-07-20T22:39:35Z', entryPrice: 1 });
    expect(buildEventMarkersForEpisodes([only], only).map((m) => m.label)).toEqual(['Entry']);
  });

  it('carries every episode’s legs, numbered per episode', () => {
    const first = target({
      entryTime: '2026-07-20T22:39:35Z',
      entryPrice: 1,
      exitLegs: [
        { time: '2026-07-20T22:40:00Z', price: 1.5, sellBps: 6000, reason: 'TakeProfit' },
        { time: '2026-07-20T22:41:00Z', price: 1.2, sellBps: 4000, reason: 'TimeStop' },
      ],
    });
    const second = target({
      entryTime: '2026-07-20T22:42:59Z',
      entryPrice: 2,
      exitLegs: [{ time: '2026-07-20T22:44:00Z', price: 2.4, sellBps: 10000, reason: 'TakeProfit' }],
    });
    const markers = buildEventMarkersForEpisodes([second, first], first);
    // 1 entry + 2 legs, then 1 entry + 1 leg — the whole traded history, in order.
    expect(markers.map((m) => m.label)).toEqual([
      'Entry 1 ◂',
      'Exit 60% · TakeProfit 1 ◂',
      'Exit 40% · TimeStop 1 ◂',
      'Entry 2',
      'Exit · TakeProfit 2',
    ]);
    expect(markers.filter((m) => m.kind === 'exit').map((m) => m.priceInSol)).toEqual([
      1.5, 1.2, 2.4,
    ]);
  });

  it('is the union of each episode’s own markers', () => {
    const eps = [
      target({ entryTime: '2026-07-20T22:39:35Z', entryPrice: 1, exitTime: '2026-07-20T22:42:14Z', exitPrice: 2 }),
      target({ entryTime: '2026-07-20T22:42:59Z', entryPrice: 3, exitTime: '2026-07-20T22:47:23Z', exitPrice: 4 }),
    ];
    const total = buildEventMarkersForEpisodes(eps).length;
    const summed = eps.reduce((n, t) => n + buildEventMarkers(t).length, 0);
    expect(total).toBe(summed);
  });
});

describe('buildEventMarkers scale-out legs', () => {
  it('renders one exit marker per leg with banked %', () => {
    const markers = buildEventMarkers(
      target({
        entryTime: '2026-07-20T22:39:35Z',
        entryPrice: 1,
        exitTime: '2026-07-20T22:47:23Z',
        exitPrice: 1.1,
        exitLegs: [
          { time: '2026-07-20T22:42:14Z', price: 1.5, sellBps: 7000, reason: 'TakeProfit' },
          { time: '2026-07-20T22:47:23Z', price: 1.1, sellBps: 3000, reason: 'TimeStop' },
        ],
      }),
    );
    expect(markers.map((m) => m.label)).toEqual([
      'Entry',
      'Exit 70% · TakeProfit',
      'Exit 30% · TimeStop',
    ]);
    expect(markers.filter((m) => m.kind === 'exit').map((m) => m.priceInSol)).toEqual([1.5, 1.1]);
  });

  it('falls back to the single exit_* fields when exitLegs is absent', () => {
    const markers = buildEventMarkers(
      target({
        entryTime: '2026-07-20T22:39:35Z',
        entryPrice: 1,
        exitTime: '2026-07-20T22:42:14Z',
        exitPrice: 2,
        exitLabel: 'TakeProfit',
      }),
    );
    expect(markers.map((m) => m.label)).toEqual(['Entry', 'Exit · TakeProfit']);
  });

  it('titles each leg’s price line by its share, not all of them "Exit"', () => {
    const lines = buildEventMarkers(
      target({
        entryTime: '2026-07-20T22:39:35Z',
        entryPrice: 1,
        exitLegs: [
          { time: '2026-07-20T22:42:14Z', price: 1.5, sellBps: 7000, reason: 'TakeProfit' },
          { time: '2026-07-20T22:47:23Z', price: 1.1, sellBps: 3000, reason: 'TimeStop' },
        ],
      }),
    ).filter((m) => m.kind === 'exit');
    expect(lines.map((m) => m.lineLabel)).toEqual(['Exit 70%', 'Exit 30%']);
  });
});

describe('inspectFromPosition', () => {
  function position(over: Partial<RulePositionRecord>): RulePositionRecord {
    return {
      entry_time: '2026-07-20T22:39:35Z',
      entry_price: 1,
      exit_time: '2026-07-20T22:47:23Z',
      exit_price: 1.38,
      exit_reason: 'TimeStop',
      ...over,
    } as RulePositionRecord;
  }

  it('draws one arrow per leg instead of one at the weighted-average price', () => {
    // `exit_price` 1.38 is the SOL-weighted average of the two legs — a price the
    // position never filled at, so no marker may sit on it.
    const markers = buildEventMarkers(
      inspectFromPosition(
        position({
          exit_legs: [
            { time: '2026-07-20T22:42:14Z', price: 1.5, sell_bps: 7000, tx: 'a', reason: 'TakeProfit' },
            { time: '2026-07-20T22:47:23Z', price: 1.1, sell_bps: 3000, tx: 'b', reason: 'TimeStop' },
          ],
        }),
      ),
    );
    const exits = markers.filter((m) => m.kind === 'exit');
    expect(exits.map((m) => m.priceInSol)).toEqual([1.5, 1.1]);
    expect(exits.map((m) => m.txSignature)).toEqual(['a', 'b']);
    expect(exits.some((m) => m.priceInSol === 1.38)).toBe(false);
  });

  it('keeps the single exit_* arrow when the backend ships no legs', () => {
    const markers = buildEventMarkers(inspectFromPosition(position({ exit_legs: null })));
    expect(markers.map((m) => m.label)).toEqual(['Entry', 'Exit · TimeStop']);
  });
});
