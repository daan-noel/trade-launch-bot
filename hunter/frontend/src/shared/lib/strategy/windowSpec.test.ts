import { describe, expect, it } from 'vitest';

// Imported straight from the Rust crates - ONE copy of each name, so the two sides
// cannot drift into a UI that spells a param the backend rejects as unknown.
import metricsSrc from '../../../../../engine/src/metrics/mod.rs?raw';
import {
  formatWindowSpec,
  parseWindowSpec,
  sameWindowSpec,
  unitSuffix,
  WINDOW_UNITS,
} from './windowSpec';

describe('window units match the engine', () => {
  // The engine enumerates its bases once, in `WindowUnit::ALL`.
  it('offers every basis the engine declares', () => {
    expect(metricsSrc).toContain(
      'pub const ALL: [WindowUnit; 3] = [Self::Sec, Self::Slot, Self::Print];',
    );
    expect(WINDOW_UNITS).toEqual(['sec', 'slot', 'print']);
  });
});

describe('label vocabulary matches the engine', () => {
  // `WindowUnit::suffix` writes these into the PERSISTED exit reason, via
  // `format_metric_exit_name`. If the two sides drift, a stored reason stops
  // resolving to the condition it names and the chart draws the wrong lane (or none).
  it('uses the same unit suffixes as WindowUnit::suffix', () => {
    for (const unit of WINDOW_UNITS) {
      const rustName = { sec: 'Sec', slot: 'Slot', print: 'Print' }[unit];
      expect(metricsSrc).toContain(`Self::${rustName} => "${unitSuffix(unit)}"`);
    }
    // The label is built once, by `WindowSpec::label`, and read back by
    // `WindowSpec::parse`; a condition's span and an exit label both use it.
    expect(metricsSrc).toContain('pub fn label(&self) -> String {');
    expect(metricsSrc).toContain('pub fn parse(s: &str) -> Option<Self> {');
  });

  // `formatWindowSpec` and `parseWindowSpec` are the frontend half of that pair. A
  // span that survives the round trip on this side is a span the backend reads back
  // as the same window.
  it('round-trips every basis through format and parse', () => {
    for (const spec of [
      { size: 30, lag: 0, unit: 'sec' as const },
      { size: 2.5, lag: 0, unit: 'sec' as const },
      { size: 1, lag: 0, unit: 'slot' as const },
      { size: 30, lag: 1, unit: 'slot' as const },
      { size: 1, lag: 0, unit: 'print' as const },
      { size: 20, lag: 1, unit: 'print' as const },
    ]) {
      expect(parseWindowSpec(formatWindowSpec(spec))).toEqual(spec);
    }
    // A bare number is seconds - the spelling every span had before the other bases
    // existed, and what `?windows=10,30,60` still means.
    expect(parseWindowSpec('60')).toEqual({ size: 60, lag: 0, unit: 'sec' });
    for (const bad of ['', 'abc', '0p', '-5s', '30x', '30sl@-1']) {
      expect(parseWindowSpec(bad)).toBeNull();
    }
  });
});

describe('formatWindowSpec', () => {
  // Same vocabulary as the Rust `event::format_metric_exit_name`, which is what a
  // persisted exit reason carries — a chip and a stored reason naming one req have
  // to read the same, or an operator sees two conditions where there is one.
  it('names the whole span, and only shows a lag when there is one', () => {
    expect(formatWindowSpec({ size: 30, lag: 0, unit: 'sec' })).toBe('30s');
    expect(formatWindowSpec({ size: 30, lag: 0, unit: 'slot' })).toBe('30sl');
    expect(formatWindowSpec({ size: 30, lag: 1, unit: 'slot' })).toBe('30sl@1');
    expect(formatWindowSpec({ size: 2.5, lag: 0, unit: 'sec' })).toBe('2.5s');
    // One size, three bases, three labels - `1p` is the one-transaction span and it
    // must not read as `1s` or `1sl`, which report entirely different tape.
    expect(formatWindowSpec({ size: 1, lag: 0, unit: 'print' })).toBe('1p');
    expect(formatWindowSpec({ size: 20, lag: 1, unit: 'print' })).toBe('20p@1');
    expect(
      new Set(
        WINDOW_UNITS.map((unit) => formatWindowSpec({ size: 1, lag: 0, unit })),
      ).size,
    ).toBe(WINDOW_UNITS.length);
    expect(formatWindowSpec(null)).toBe('');
  });
});

describe('sameWindowSpec', () => {
  it('separates unit, size and lag', () => {
    const base = { size: 30, lag: 0, unit: 'sec' } as const;
    expect(sameWindowSpec(base, { ...base })).toBe(true);
    expect(sameWindowSpec(base, { ...base, unit: 'slot' })).toBe(false);
    expect(sameWindowSpec(base, { ...base, unit: 'print' })).toBe(false);
    expect(sameWindowSpec({ ...base, unit: 'slot' }, { ...base, unit: 'print' })).toBe(false);
    expect(sameWindowSpec(base, { ...base, lag: 1 })).toBe(false);
    expect(sameWindowSpec(base, { ...base, size: 60 })).toBe(false);
    expect(sameWindowSpec(null, null)).toBe(true);
    expect(sameWindowSpec(base, null)).toBe(false);
  });
});
