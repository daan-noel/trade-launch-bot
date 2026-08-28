import { describe, expect, it } from 'vitest';

// Imported straight from the Rust crates - ONE copy of each name, so the two sides
// cannot drift into a UI that spells a param the backend rejects as unknown.
import metricsSrc from '../../../../../engine/src/metrics/mod.rs?raw';
import flowSliceSrc from '../../../../../engine/src/metrics/flow_slice.rs?raw';
import eventSrc from '../../../../../engine/src/event.rs?raw';
import {
  SLICE_PARAM,
  SLICE_PRINT_PARAM,
  SLICE_SLOT_PARAM,
  sliceSpecFromStrict,
  formatWindowSpec,
  parseWindowSpec,
  readWindow,
  sameWindowSpec,
  unitSuffix,
  WINDOW_LAG_PARAM,
  WINDOW_PRINT_PARAM,
  WINDOW_SEC_PARAM,
  WINDOW_SLOT_PARAM,
  WINDOW_UNITS,
  windowSpecFromStrict,
} from './windowSpec';

/** The param names are a CONTRACT with the engine registry: a rule spelling one the
 *  backend does not declare is rejected as an unknown param at save, and a control
 *  the backend never reads is a field that silently does nothing. Read them out of
 *  the Rust source rather than trusting two hand-kept copies. */
describe('param names match the engine', () => {
  it('mirrors hunter_engine::metrics window params', () => {
    const src = metricsSrc;
    for (const [constant, value] of [
      ['WINDOW_SEC_PARAM', WINDOW_SEC_PARAM],
      ['WINDOW_SLOT_PARAM', WINDOW_SLOT_PARAM],
      ['WINDOW_PRINT_PARAM', WINDOW_PRINT_PARAM],
      ['WINDOW_LAG_PARAM', WINDOW_LAG_PARAM],
    ] as const) {
      expect(src).toContain(`pub const ${constant}: &str = "${value}";`);
    }
  });

  it('mirrors hunter_engine::metrics::flow_slice burst params', () => {
    const src = flowSliceSrc;
    expect(src).toContain(`pub const SLICE_PARAM: &str = "${SLICE_PARAM}";`);
    expect(src).toContain(`pub const SLICE_SLOT_PARAM: &str = "${SLICE_SLOT_PARAM}";`);
    expect(src).toContain(`pub const SLICE_PRINT_PARAM: &str = "${SLICE_PRINT_PARAM}";`);
  });

  // A basis with no option in the picker round-trips through the JSON view with no
  // way to author it, which is how `window_size_slots` shipped DB-only the first
  // time. The engine enumerates its bases once, in `WindowUnit::ALL`.
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
    // The label itself is built once, by `WindowSpec::label`, and the exit-reason
    // qualifier is that string in parentheses - so a reason parses back through
    // `WindowSpec::parse` by construction rather than by two sides agreeing.
    expect(metricsSrc).toContain('pub fn label(&self) -> String {');
    expect(metricsSrc).toContain('pub fn parse(s: &str) -> Option<Self> {');
    expect(eventSrc).toContain('format!("{}({})", metric.name(), w.label())');
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

describe('windowSpecFromStrict', () => {
  it('reads any size param, with the group lag', () => {
    expect(windowSpecFromStrict({ window_size_sec: 30 })).toEqual({
      size: 30,
      lag: 0,
      unit: 'sec',
    });
    expect(windowSpecFromStrict({ window_size_slots: 30, window_lag: 1 })).toEqual({
      size: 30,
      lag: 1,
      unit: 'slot',
    });
    // The one-transaction span: `gross_flow >= 10` over this is "this tx moved 10 SOL".
    expect(windowSpecFromStrict({ window_size_prints: 1 })).toEqual({
      size: 1,
      lag: 0,
      unit: 'print',
    });
    expect(windowSpecFromStrict({ window_size_prints: 20, window_lag: 1 })).toEqual({
      size: 20,
      lag: 1,
      unit: 'print',
    });
    expect(windowSpecFromStrict({})).toBeNull();
  });

  it('gives the burst axis the group lag and its own size', () => {
    expect(sliceSpecFromStrict({ window_size_slots: 30, window_lag: 1, slice_size_slots: 1 }))
      .toEqual({ size: 1, lag: 1, unit: 'slot' });
    expect(sliceSpecFromStrict({ window_size_prints: 20, slice_size_prints: 4 }))
      .toEqual({ size: 4, lag: 0, unit: 'print' });
    expect(sliceSpecFromStrict({ window_size_sec: 60 })).toBeNull();
  });
});

describe('readWindow', () => {
  // A slot window reports `null` under the legacy seconds key, so a reader that only
  // knows that key drops the window entirely rather than calling 30 slots 30 seconds.
  it('prefers the span object and falls back to the legacy scalar', () => {
    expect(readWindow({ window: { size: 30, lag: 1, unit: 'slot' }, window_size_sec: null }))
      .toEqual({ size: 30, lag: 1, unit: 'slot' });
    expect(readWindow({ window: { size: 1, lag: 0, unit: 'print' }, window_size_sec: null }))
      .toEqual({ size: 1, lag: 0, unit: 'print' });
    expect(readWindow({ window_size_sec: 60 })).toEqual({ size: 60, lag: 0, unit: 'sec' });
    expect(readWindow({ window_size_sec: null })).toBeNull();
  });
});
