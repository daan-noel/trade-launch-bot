import { describe, expect, it } from 'vitest';

import { tagsFromJson, tagsToJson, withTagShape } from './tagsDoc';
import {
  addPatternRows,
  anyRowMatchesTrade,
  feeFromTrade,
  feeMaskActive,
  feeMatchesTrade,
  formatFeePins,
  parseIxPatternRow,
  parseIxPatternRows,
  patternKey,
  patternRowKey,
  removePatternRows,
  rowFamilyState,
  rowFromTrade,
  rowMatchesTrade,
  rowNarrows,
  rowPinsFee,
  rowsOverlap,
  serializeIxPatternRow,
  togglePatternRow,
  type IxPatternRow,
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

/** A save must not widen a pinned entry back to ix-only. */
describe('a tag keeps its ix shape rows whole', () => {
  it('keeps a pin through a read and a write', () => {
    const doc = withTagShape({}, 'dump', { labels: DUMP, cu_limit: 300_000, cu_price: 3_333_333 });
    expect(tagsFromJson(tagsToJson(tagsFromJson(doc)))[0].match.ix_shape).toEqual([
      { labels: DUMP, cu_limit: 300_000, cu_price: 3_333_333 },
    ]);
  });

  /** Removing a shape removes it; the pinned twin is a different entry. */
  it('removes exactly the row asked for', () => {
    let doc = withTagShape({}, 'dump', { labels: DUMP, cu_limit: 300_000 });
    doc = withTagShape(doc, 'dump', { labels: ['Pump.Fun: Buy'] });
    doc = withTagShape(doc, 'dump', { labels: DUMP }, true);
    expect(tagsFromJson(doc)[0].match.ix_shape).toEqual([{ labels: DUMP, cu_limit: 300_000 }, { labels: ['Pump.Fun: Buy'] }]);
  });

  /** A chart click and a draft Apply write through here, so both keep one broad row
   *  or narrow rows per family. */
  it('narrows and widens a family on add', () => {
    let doc = withTagShape({}, 'dump', { labels: DUMP });
    doc = withTagShape(doc, 'dump', { labels: DUMP, cu_limit: 300_000 });
    expect(tagsFromJson(doc)[0].match.ix_shape).toEqual([{ labels: DUMP, cu_limit: 300_000 }]);
    doc = withTagShape(doc, 'dump', { labels: DUMP });
    expect(tagsFromJson(doc)[0].match.ix_shape).toEqual([{ labels: DUMP }]);
  });

  /** An unpinned row stays the bare array it always was: `tags` is row identity. */
  it('writes an unpinned row as a bare array', () => {
    const doc = withTagShape({}, 'volume', { labels: DUMP });
    expect(doc).toEqual({ volume: { match: { ix_shape: [DUMP], creator: true }, sticky: true } });
  });
});

describe('feeFromTrade / rowFromTrade', () => {
  const tx = { cu_limit: 300_000, cu_price: 3_333_333, tip_lamports: 0 };

  it('copies nothing when the mask is off — a tx that has every field still stages ix-only', () => {
    expect(feeFromTrade(tx, {})).toEqual({});
    expect(feeFromTrade(tx, { cu_limit: false })).toEqual({});
    expect(rowFromTrade(DUMP, tx, {})).toEqual({ labels: DUMP });
  });

  it('copies only the fields the mask asks for, off THIS tx', () => {
    expect(feeFromTrade(tx, { cu_limit: true })).toEqual({ cu_limit: 300_000 });
    expect(rowFromTrade(DUMP, tx, { cu_limit: true, cu_price: true })).toEqual({
      labels: DUMP,
      cu_limit: 300_000,
      cu_price: 3_333_333,
    });
  });

  it('skips a checked field the tx does not carry', () => {
    expect(feeFromTrade({ cu_limit: null }, { cu_limit: true, cu_price: true })).toEqual({});
    expect(rowFromTrade(DUMP, { cu_limit: 300_000 }, { cu_limit: true, tip_lamports: true })).toEqual({
      labels: DUMP,
      cu_limit: 300_000,
    });
  });

  it('copies a pinned zero — the "no tip" reading', () => {
    expect(feeFromTrade(tx, { tip_lamports: true })).toEqual({ tip_lamports: 0 });
  });
});

describe('patternRowKey', () => {
  it('keys an unpinned row as its label array — the same string a structure click uses', () => {
    expect(patternRowKey({ labels: DUMP })).toBe(patternKey(DUMP));
    expect(patternRowKey({ labels: DUMP })).toBe(JSON.stringify(DUMP));
  });

  it('keys a pinned row as a different entry from the same shape unpinned', () => {
    expect(patternRowKey({ labels: DUMP, cu_limit: 300_000 })).not.toBe(
      patternRowKey({ labels: DUMP }),
    );
  });
});

describe('togglePatternRow', () => {
  const unpinned = { labels: DUMP };
  const pin300 = { labels: DUMP, cu_limit: 300_000 };
  const pin200 = { labels: DUMP, cu_limit: 200_000 };
  const other = { labels: ['Pump.Fun: Buy'] };

  it('adds and removes a pin', () => {
    const once = togglePatternRow([], pin300);
    expect(once).toEqual([pin300]);
    expect(togglePatternRow(once, pin300)).toEqual([]);
  });

  it('narrows a catch-all to the clicked pin instead of keeping both', () => {
    expect(togglePatternRow([unpinned], pin300)).toEqual([pin300]);
  });

  it('widens pins of a shape to a catch-all', () => {
    expect(togglePatternRow([pin300, pin200], unpinned)).toEqual([unpinned]);
  });

  it('removes a catch-all and leaves other shapes', () => {
    expect(togglePatternRow([other, unpinned], unpinned)).toEqual([other]);
  });

  it('ORs a second pin of the same shape', () => {
    expect(togglePatternRow([pin300], pin200)).toEqual([pin300, pin200]);
    expect(togglePatternRow([pin300, pin200], pin300)).toEqual([pin200]);
  });

  it('ignores an empty sequence', () => {
    expect(togglePatternRow([unpinned], { labels: [] })).toEqual([unpinned]);
  });
});

describe('addPatternRows / removePatternRows', () => {
  const pinned = { labels: DUMP, cu_limit: 300_000 };

  it('widens pins of a shape to a catch-all', () => {
    expect(addPatternRows([pinned], [{ labels: DUMP }])).toEqual([{ labels: DUMP }]);
  });

  it('does not duplicate an unpinned row already in the list', () => {
    expect(addPatternRows([{ labels: DUMP }], [{ labels: DUMP }])).toEqual([{ labels: DUMP }]);
  });

  it('skips a pin whose catch-all is already listed', () => {
    expect(addPatternRows([{ labels: DUMP }], [pinned])).toEqual([{ labels: DUMP }]);
  });

  it('unstages the catch-all and keeps the pin', () => {
    expect(removePatternRows([pinned, { labels: DUMP }], [{ labels: DUMP }])).toEqual([pinned]);
  });
});

describe('rowMatchesTrade / anyRowMatchesTrade', () => {
  const tx = { cu_limit: 300_000, cu_price: 3_333_333, tip_lamports: 0 };
  const other = { cu_limit: 200_000, cu_price: 1, tip_lamports: 1 };

  /** The bug this covers: an ix-only row is a fee wildcard, so a trade that
   *  carries every budget field still matches it — and so does checking the
   *  pin strip, which must not make the catch-all look unselected. */
  it('an unpinned row matches every budget of that shape', () => {
    const wild = { labels: DUMP };
    expect(feeMatchesTrade({}, tx)).toBe(true);
    expect(feeMatchesTrade({}, {})).toBe(true);
    expect(rowMatchesTrade(wild, DUMP, tx)).toBe(true);
    expect(rowMatchesTrade(wild, DUMP, other)).toBe(true);
    expect(rowMatchesTrade(wild, ['Pump.Fun: Buy'], tx)).toBe(false);
    expect(anyRowMatchesTrade([wild], DUMP, tx)).toBe(true);
    expect(anyRowMatchesTrade([wild], DUMP, other)).toBe(true);
  });

  it('a pin matches only that reading and never an absent one', () => {
    const pin = { labels: DUMP, cu_limit: 300_000 };
    expect(rowMatchesTrade(pin, DUMP, tx)).toBe(true);
    expect(rowMatchesTrade(pin, DUMP, other)).toBe(false);
    expect(rowMatchesTrade(pin, DUMP, {})).toBe(false);
    expect(anyRowMatchesTrade([pin], DUMP, tx)).toBe(true);
    expect(anyRowMatchesTrade([pin], DUMP, other)).toBe(false);
  });

  it('a wildcard in the list covers a trade a pin of the same shape would miss', () => {
    const rows = [
      { labels: DUMP, cu_limit: 300_000 },
      { labels: DUMP },
    ];
    expect(anyRowMatchesTrade(rows, DUMP, other)).toBe(true);
  });
});

describe('feeMaskActive / formatFeePins', () => {
  it('is off until a field is checked', () => {
    expect(feeMaskActive({})).toBe(false);
    expect(feeMaskActive({ cu_limit: true })).toBe(true);
  });

  it('reads a pin as the fields that are set', () => {
    expect(formatFeePins({})).toBe('');
    expect(formatFeePins({ cu_limit: 300_000, cu_price: 3_333_333 })).toBe(
      'cu 300,000 @ 3,333,333',
    );
  });
});

describe('core-level rows', () => {
  const core = { labels: ['Pump.Fun: Sell'], level: 'core' as const, side: 'sell' as const, marks: 'CL CP T0 A0', cu_price: 167000 };

  it('round-trips level, side, marks and pins', () => {
    const parsed = parseIxPatternRow(serializeIxPatternRow(core));
    expect(parsed).toEqual(core);
    expect(serializeIxPatternRow({ labels: ['Pump.Fun: Sell'], level: 'core' })).toEqual({ labels: ['Pump.Fun: Sell'], level: 'core' });
  });

  it('refuses what the engine refuses', () => {
    expect(parseIxPatternRow({ labels: ['A'], side: 'sell' })).toBeNull();
    expect(parseIxPatternRow({ labels: ['A'], level: 'wide' })).toBeNull();
    expect(parseIxPatternRow({ labels: ['A'], level: 'core', marks: 'CU' })).toBeNull();
  });

  it('matches every variant of the build, on side, extras and fee', () => {
    const t = { side: 'sell' as const, cu_price: 167000 };
    expect(rowMatchesTrade(core, ['Compute Budget: SetComputeUnitPrice', 'Compute Budget: SetComputeUnitLimit', 'Pump.Fun: SellV2'], t)).toBe(true);
    expect(rowMatchesTrade(core, ['Compute Budget: SetComputeUnitLimit', 'Compute Budget: SetComputeUnitPrice', 'Memo Program: Memo', 'Pump.Fun: Sell'], t)).toBe(false);
    expect(rowMatchesTrade(core, ['Compute Budget: SetComputeUnitLimit', 'Compute Budget: SetComputeUnitPrice', 'Pump.Fun: Sell'], { ...t, side: 'buy' })).toBe(false);
  });

  it('is its own family, apart from the exact row of the same labels', () => {
    expect(removePatternRows([core], [{ labels: ['Pump.Fun: Sell'] }])).toEqual([core]);
    expect(togglePatternRow([core], { labels: ['Pump.Fun: Sell'] })).toEqual([core, { labels: ['Pump.Fun: Sell'] }]);
  });

  it('keeps one broad core or narrow cores, as exact rows do', () => {
    const broad = { labels: ['Pump.Fun: Sell'], level: 'core' as const };
    const sideOnly = { labels: ['Pump.Fun: Sell'], level: 'core' as const, side: 'sell' as const };
    expect(rowNarrows(broad)).toBe(false);
    expect(rowNarrows(sideOnly)).toBe(true);
    expect(togglePatternRow([core, sideOnly], broad)).toEqual([broad]);
    expect(togglePatternRow([broad], core)).toEqual([core]);
    expect(togglePatternRow([core], sideOnly)).toEqual([core, sideOnly]);
    expect(rowFamilyState([core], broad)).toBe('narrow');
    expect(rowFamilyState([broad], core)).toBe('broad');
    expect(rowFamilyState([{ labels: ['Pump.Fun: Sell'] }], broad)).toBe('none');
  });
});

describe('rowsOverlap', () => {
  const FULL = ['Compute Budget: SetComputeUnitLimit', 'Pump.Fun: Sell', 'Token Program: CloseAccount'];
  const exact: IxPatternRow = { labels: FULL };
  const core: IxPatternRow = { labels: ['Pump.Fun: SELL'], level: 'core' };

  it('lights an arm on its broader and its narrower rows alike', () => {
    const pinned: IxPatternRow = { labels: FULL, cu_limit: 300_000 };
    expect(rowsOverlap(core, exact)).toBe(true);
    expect(rowsOverlap(exact, core)).toBe(true);
    expect(rowsOverlap(pinned, exact)).toBe(true);
    expect(rowsOverlap(exact, pinned)).toBe(true);
  });

  it('keeps apart rows no trade can match both of', () => {
    expect(rowsOverlap({ labels: FULL, cu_limit: 1 }, { labels: FULL, cu_limit: 2 })).toBe(false);
    expect(rowsOverlap(exact, { labels: ['Pump.Fun: Sell'] })).toBe(false);
    expect(rowsOverlap({ ...core, side: 'buy' }, { ...core, side: 'sell' })).toBe(false);
    expect(rowsOverlap({ labels: ['Pump.Fun: BUY'], level: 'core' }, exact)).toBe(false);
  });

  it("reads an exact row's marks off its labels against a core row's pin", () => {
    expect(rowsOverlap({ ...core, marks: 'CL C T0 A0' }, exact)).toBe(true);
    expect(rowsOverlap({ ...core, marks: 'CP T0 A0' }, exact)).toBe(false);
  });
});
