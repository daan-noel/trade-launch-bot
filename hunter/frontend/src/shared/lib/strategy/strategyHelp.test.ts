import { describe, expect, it } from 'vitest';
import { METRIC_HELP, metricHelpBody, STRICT_PARAM_HELP } from './strategyHelp';

const SPEC = { unit: 'sol', eq_tolerance: 0.1, monotonic: false };

describe('metricHelpBody', () => {
  it('renders the REGISTRY definition, not the frontend copy', () => {
    // The root rule: a metric carries one definition, written where the metric is
    // defined and rendered into the UI from that same text. If the frontend copy could
    // win, a tooltip could say something the engine does not.
    const body = metricHelpBody('gross_flow', {
      ...SPEC,
      description: 'Buy + sell SOL over the trailing window.',
    });
    expect(body.startsWith('Buy + sell SOL over the trailing window.')).toBe(true);
  });

  it('appends unit and = tolerance from the same spec, never from prose', () => {
    const body = metricHelpBody('anything_at_all', {
      unit: 'count',
      eq_tolerance: 0.5,
      monotonic: true,
      description: 'A tally of things.',
    });
    expect(body).toContain('A tally of things.');
    // `count` renders bare — a tally with a unit glyph reads as a quantity of something
    // else, which is the bug the explicit unit map exists to prevent.
    expect(body).toContain('Unit: a plain count.');
    expect(body).toContain('±0.25');
    expect(body).toContain('Monotonic');
  });

  it('keeps extended prose BELOW the definition rather than replacing it', () => {
    const metric = Object.keys(METRIC_HELP).find((k) => METRIC_HELP[k]?.body);
    expect(metric).toBeTruthy();
    const description = 'The one-line registry definition.';
    const body = metricHelpBody(metric!, { ...SPEC, description });
    expect(body.indexOf(description)).toBe(0);
    // Both present, definition first — the guidance is additive, not a second definition.
    expect(body.indexOf(METRIC_HELP[metric!].body)).toBeGreaterThan(0);
  });

  it('falls back to the frontend copy only when the payload carries no description', () => {
    // A registry payload from a backend that predates the field must still render help
    // rather than the generic placeholder.
    const metric = Object.keys(METRIC_HELP).find((k) => METRIC_HELP[k]?.body)!;
    const body = metricHelpBody(metric, SPEC);
    expect(body.startsWith(METRIC_HELP[metric].body)).toBe(true);
  });

  it('documents the second window axis, so a burst row is not a mystery field', () => {
    const tip = STRICT_PARAM_HELP.slice_size_sec;
    expect(tip).toBeTruthy();
    expect(tip.body).toContain('window_size_sec');
    expect(tip.body).toContain('trade_share');
  });
});
