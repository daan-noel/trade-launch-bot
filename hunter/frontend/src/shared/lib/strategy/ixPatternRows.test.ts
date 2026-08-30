import { describe, expect, it } from 'vitest';

import {
  dumpPatternRowsFromConfig,
  dumpPatternsFromConfig,
  ixPatternRowsFromConfig,
  ixPatternsFromConfig,
  metricConfigWithDumpPatterns,
  metricConfigWithIxPatterns,
} from './registry';
import {
  parseIxPatternRow,
  parseIxPatternRows,
  rowPinsFee,
  serializeIxPatternRow,
  withPreservedFees,
} from './ixPatternRows';

const DUMP = ['Pump.Fun: Sell', 'Token Program: CloseAccount'];

describe('parseIxPatternRow', () => {
  it('reads both stored row shapes', () => {
    expect(parseIxPatternRow(DUMP)).toEqual({ labels: DUMP });
    expect(parseIxPatternRow({ labels: DUMP, cu_limit: 300_000 })).toEqual({
      labels: DUMP,
      cu_limit: 300_000,
    });
  });

  it('refuses a row the backend would refuse', () => {
    expect(parseIxPatternRow('Pump.Fun: Sell')).toBeNull();
    expect(parseIxPatternRow({ cu_limit: 1 })).toBeNull();
    expect(parseIxPatternRow(['a', 7])).toBeNull();
  });

  /** A fee field that is not a non-negative integer is dropped rather than kept as
   *  something the backend will reject on save. */
  it('drops a fee field that is not a whole non-negative number', () => {
    expect(parseIxPatternRow({ labels: DUMP, cu_limit: '300000' })).toEqual({ labels: DUMP });
    expect(parseIxPatternRow({ labels: DUMP, cu_price: -1 })).toEqual({ labels: DUMP });
    expect(parseIxPatternRow({ labels: DUMP, tip_lamports: 1.5 })).toEqual({ labels: DUMP });
  });

  /** A real `0` is a value — the tip pin that means "sent no tip". */
  it('keeps a pinned zero', () => {
    const row = parseIxPatternRow({ labels: DUMP, tip_lamports: 0 });
    expect(row).toEqual({ labels: DUMP, tip_lamports: 0 });
    expect(rowPinsFee(row!)).toBe(true);
  });

  it('skips unreadable rows rather than failing the whole list', () => {
    expect(parseIxPatternRows([DUMP, 'junk', { labels: ['A'], cu_limit: 1 }])).toEqual([
      { labels: DUMP },
      { labels: ['A'], cu_limit: 1 },
    ]);
  });
});

describe('serializeIxPatternRow', () => {
  /** `metric_config` is part of a fingerprint's ROW identity, so an unpinned row has
   *  to serialize back to the bare array it came in as — rewriting every list into
   *  the object form would rewrite identity app-wide for no behaviour change. */
  it('writes an unpinned row as a bare array', () => {
    expect(serializeIxPatternRow({ labels: DUMP })).toEqual(DUMP);
  });

  it('writes a pinned row as an object carrying only the pinned fields', () => {
    expect(serializeIxPatternRow({ labels: DUMP, cu_limit: 300_000 })).toEqual({
      labels: DUMP,
      cu_limit: 300_000,
    });
  });

  it('round-trips through the parser', () => {
    for (const row of [
      { labels: DUMP },
      { labels: DUMP, cu_limit: 300_000, cu_price: 3_333_333, tip_lamports: 0 },
    ]) {
      expect(parseIxPatternRow(serializeIxPatternRow(row))).toEqual(row);
    }
  });
});

describe('withPreservedFees', () => {
  const prev = [
    { labels: DUMP, cu_limit: 300_000 },
    ['Pump.Fun: Buy'],
  ];

  it('re-attaches a pin to the shape it belonged to', () => {
    expect(withPreservedFees([DUMP], prev)).toEqual([{ labels: DUMP, cu_limit: 300_000 }]);
  });

  it('leaves a newly added shape unpinned', () => {
    expect(withPreservedFees([['Pump.Fun: Create']], prev)).toEqual([
      { labels: ['Pump.Fun: Create'] },
    ]);
  });

  it('drops the pin with the shape when the shape is removed', () => {
    expect(withPreservedFees([['Pump.Fun: Buy']], prev)).toEqual([{ labels: ['Pump.Fun: Buy'] }]);
  });

  /** A shape with several pins is a preset MENU; collapsing it to one would quietly
   *  narrow the list. */
  it('keeps every pin on a shape that carries more than one', () => {
    const menu = [
      { labels: DUMP, cu_limit: 300_000 },
      { labels: DUMP, cu_limit: 200_000 },
    ];
    expect(withPreservedFees([DUMP], menu)).toEqual(menu);
  });
});

/** The regression this whole module exists to prevent: the fingerprint form, the
 *  flow lens, the sweep config and the discovery cart all edit LABELS, and a save
 *  from any of them must not widen a pinned entry back to ix-only. */
describe('a labels-only save preserves the pins it cannot edit', () => {
  it('keeps a flow pin across a labels-only rewrite', () => {
    const before = metricConfigWithIxPatterns([
      { labels: DUMP, cu_limit: 300_000 },
      { labels: ['Pump.Fun: Buy'] },
    ]);
    // What a labels-only surface reads, edits, and writes back.
    const labels = ixPatternsFromConfig(before);
    expect(labels).toEqual([DUMP, ['Pump.Fun: Buy']]);
    const after = metricConfigWithIxPatterns(labels, before);
    expect(ixPatternRowsFromConfig(after)).toEqual([
      { labels: DUMP, cu_limit: 300_000 },
      { labels: ['Pump.Fun: Buy'] },
    ]);
  });

  it('keeps a dump pin across a labels-only rewrite', () => {
    const before = metricConfigWithDumpPatterns({}, [
      { labels: DUMP, cu_limit: 300_000, cu_price: 3_333_333 },
    ]);
    const after = metricConfigWithDumpPatterns(before, dumpPatternsFromConfig(before));
    expect(dumpPatternRowsFromConfig(after)).toEqual([
      { labels: DUMP, cu_limit: 300_000, cu_price: 3_333_333 },
    ]);
  });

  /** Removing a shape still removes it — preservation is not resurrection. */
  it('does not resurrect a shape the surface deleted', () => {
    const before = metricConfigWithDumpPatterns({}, [
      { labels: DUMP, cu_limit: 300_000 },
      { labels: ['Pump.Fun: Buy'] },
    ]);
    const after = metricConfigWithDumpPatterns(before, [['Pump.Fun: Buy']]);
    expect(dumpPatternRowsFromConfig(after)).toEqual([{ labels: ['Pump.Fun: Buy'] }]);
  });

  /** An unpinned list must serialize byte-identically to what it always did. */
  it('leaves a list with no pins in the shape it has always had', () => {
    const cfg = metricConfigWithDumpPatterns({}, [DUMP, ['Pump.Fun: Buy']]);
    expect((cfg.m_dump_ix as Record<string, unknown>).ix_patterns).toEqual([
      DUMP,
      ['Pump.Fun: Buy'],
    ]);
  });
});
