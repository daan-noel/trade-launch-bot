import { describe, expect, it } from 'vitest';

// Imported straight from the Rust crates - ONE copy of each name, so the two sides
// cannot drift into a UI that spells a param the backend rejects as unknown.
import metricsSrc from '../../../../../engine/src/metrics/mod.rs?raw';
import flowBurstSrc from '../../../../../engine/src/metrics/flow_burst.rs?raw';
import eventSrc from '../../../../../engine/src/event.rs?raw';
import {
  BURST_PARAM,
  BURST_SLOT_PARAM,
  burstSpecFromStrict,
  formatWindowSpec,
  readWindow,
  sameWindowSpec,
  WINDOW_LAG_PARAM,
  WINDOW_SEC_PARAM,
  WINDOW_SLOT_PARAM,
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
      ['WINDOW_LAG_PARAM', WINDOW_LAG_PARAM],
    ] as const) {
      expect(src).toContain(`pub const ${constant}: &str = "${value}";`);
    }
  });

  it('mirrors hunter_engine::metrics::flow_burst burst params', () => {
    const src = flowBurstSrc;
    expect(src).toContain(`pub const BURST_PARAM: &str = "${BURST_PARAM}";`);
    expect(src).toContain(`pub const BURST_SLOT_PARAM: &str = "${BURST_SLOT_PARAM}";`);
  });
});

describe('label vocabulary matches the engine', () => {
  // `format_metric_exit_name` writes these suffixes into the PERSISTED exit reason.
  // If the two sides drift, a stored reason stops resolving to the condition it
  // names and the chart draws the wrong lane (or none).
  it('uses the same unit suffixes as event::format_metric_exit_name', () => {
    const src = eventSrc;
    expect(src).toContain('WindowUnit::Sec => "s"');
    expect(src).toContain('WindowUnit::Slot => "sl"');
    expect(src).toContain('format!("{}({}{unit}{lag})"');
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
    expect(formatWindowSpec(null)).toBe('');
  });
});

describe('sameWindowSpec', () => {
  it('separates unit, size and lag', () => {
    const base = { size: 30, lag: 0, unit: 'sec' } as const;
    expect(sameWindowSpec(base, { ...base })).toBe(true);
    expect(sameWindowSpec(base, { ...base, unit: 'slot' })).toBe(false);
    expect(sameWindowSpec(base, { ...base, lag: 1 })).toBe(false);
    expect(sameWindowSpec(base, { ...base, size: 60 })).toBe(false);
    expect(sameWindowSpec(null, null)).toBe(true);
    expect(sameWindowSpec(base, null)).toBe(false);
  });
});

describe('windowSpecFromStrict', () => {
  it('reads either size param, with the group lag', () => {
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
    expect(windowSpecFromStrict({})).toBeNull();
  });

  it('gives the burst axis the group lag and its own size', () => {
    expect(burstSpecFromStrict({ window_size_slots: 30, window_lag: 1, burst_size_slots: 1 }))
      .toEqual({ size: 1, lag: 1, unit: 'slot' });
    expect(burstSpecFromStrict({ window_size_sec: 60 })).toBeNull();
  });
});

describe('readWindow', () => {
  // A slot window reports `null` under the legacy seconds key, so a reader that only
  // knows that key drops the window entirely rather than calling 30 slots 30 seconds.
  it('prefers the span object and falls back to the legacy scalar', () => {
    expect(readWindow({ window: { size: 30, lag: 1, unit: 'slot' }, window_size_sec: null }))
      .toEqual({ size: 30, lag: 1, unit: 'slot' });
    expect(readWindow({ window_size_sec: 60 })).toEqual({ size: 60, lag: 0, unit: 'sec' });
    expect(readWindow({ window_size_sec: null })).toBeNull();
  });
});
