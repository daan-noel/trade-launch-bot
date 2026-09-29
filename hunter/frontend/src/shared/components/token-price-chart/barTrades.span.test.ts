import { describe, expect, it } from 'vitest';
import { rangeForSpan, rangeSpan } from './barTrades';

const t = (sec: number, slot: number) => ({ block_time: new Date(sec * 1000).toISOString(), slot });
// Slots 100..104, one trade each, at 1000 s .. 1004 s.
const trades = [t(1000, 100), t(1001, 101), t(1002, 102), t(1003, 103), t(1004, 104)];

describe('rangeForSpan', () => {
  it('snaps a span to bucket starts in time mode', () => {
    expect(rangeForSpan(trades, { from: 1000.5, to: 1003.2 }, 'time', 1)).toEqual({ lo: 1000, hi: 1003 });
    expect(rangeForSpan(trades, { from: 1001, to: 1009 }, 'time', 5)).toEqual({ lo: 1000, hi: 1005 });
  });

  it('takes the slots of the trades inside the span in slot mode', () => {
    expect(rangeForSpan(trades, { from: 1001, to: 1003 }, 'slot', 1)).toEqual({ lo: 101, hi: 103 });
    expect(rangeForSpan(trades, { from: 2000, to: 2001 }, 'slot', 1)).toBeNull();
  });
});

describe('rangeSpan', () => {
  it('covers whole buckets in time mode, the last slot at or before its end', () => {
    const s = rangeSpan(trades, { lo: 1001, hi: 1002, groupMode: 'time', intervalSec: 1 })!;
    expect(s.from).toBe(1001);
    expect(s.to).toBeCloseTo(1002.999);
    expect(s.endSlot).toBe(102);
  });

  it('spans the trades of the slots in slot mode', () => {
    expect(rangeSpan(trades, { lo: 101, hi: 103, groupMode: 'slot', intervalSec: 0 })).toEqual({
      from: 1001,
      to: 1003,
      endSlot: 103,
    });
  });

  it('round-trips with rangeForSpan', () => {
    const r = rangeForSpan(trades, { from: 1001, to: 1003 }, 'slot', 1)!;
    const s = rangeSpan(trades, { ...r, groupMode: 'slot', intervalSec: 0 })!;
    expect(rangeForSpan(trades, s, 'slot', 1)).toEqual(r);
  });

  it('is null when nothing sits at or before the range', () => {
    expect(rangeSpan(trades, { lo: 900, hi: 901, groupMode: 'time', intervalSec: 1 })).toBeNull();
  });
});
