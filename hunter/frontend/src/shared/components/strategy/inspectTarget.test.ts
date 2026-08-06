import { describe, expect, it } from 'vitest';
import {
  buildEventMarkers,
  buildEventMarkersForEpisodes,
  episodeRowKey,
  parseEpisodeRowKey,
  type InspectTarget,
} from './inspectTarget';

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
});
