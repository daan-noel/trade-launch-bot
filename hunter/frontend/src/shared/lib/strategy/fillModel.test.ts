// The fill-model id is a BARE STRING for every model, `lag_<ms>` included.
//
// It was not always: the backend serialized the lag model as `{lag_ms: 115}`, which is
// not a valid React child — every page that printed the model name crashed, and the
// sweep's TEXT column silently dropped it back to `worst_case` on reload. These pin the
// string contract from the frontend side.
import { describe, expect, it } from 'vitest';
import { FILL_MODELS, fillModelLabel, fillModelLagMs, type FillModelId } from './types';

describe('fill model ids', () => {
  it('labels every preset', () => {
    for (const m of FILL_MODELS) {
      expect(fillModelLabel(m.id)).toBe(m.label);
      expect(typeof fillModelLabel(m.id)).toBe('string');
    }
  });

  it('labels a lag the preset list does not name', () => {
    expect(fillModelLabel('lag_412')).toBe('Lag 412 ms');
    expect(fillModelLagMs('lag_412')).toBe(412);
  });

  it('reads the lag out of the id, and only for lag models', () => {
    expect(fillModelLagMs('lag_115')).toBe(115);
    expect(fillModelLagMs('lag_0')).toBe(0);
    expect(fillModelLagMs('worst_case')).toBeNull();
    expect(fillModelLagMs('lag_')).toBeNull();
    expect(fillModelLagMs('lag_abc')).toBeNull();
    expect(fillModelLagMs(null)).toBeNull();
  });

  it('never returns a non-string, whatever the backend sends', () => {
    // The exact shape that used to crash the page, plus the other unknowns.
    for (const bad of [undefined, null, '', 'nonsense'] as const) {
      expect(typeof fillModelLabel(bad)).toBe('string');
    }
    expect(fillModelLabel(String({ lag_ms: 115 }))).toBe('[object Object]');
  });

  it('carries both measured latency presets', () => {
    const ids = FILL_MODELS.map((m) => m.id);
    // p50 and p90 of the bot's own decide-to-fill: a rule is read at one and stressed
    // at the other, so both have to be one click away.
    expect(ids).toContain<FillModelId>('lag_115');
    expect(ids).toContain<FillModelId>('lag_235');
  });
});
