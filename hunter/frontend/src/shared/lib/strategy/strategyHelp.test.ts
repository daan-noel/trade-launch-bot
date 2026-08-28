import { describe, expect, it } from 'vitest';
import { GROUP_HELP, METRIC_HELP, metricHelpBody, STRICT_PARAM_HELP } from './strategyHelp';

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

  it('documents the second window axis, so a slice row is not a mystery field', () => {
    const tip = STRICT_PARAM_HELP.slice_size_sec;
    expect(tip).toBeTruthy();
    expect(tip.body).toContain('window_size_sec');
    expect(tip.body).toContain('trade_share');
  });
});

/**
 * **The vocabulary lock.** Help text is the only place a metric's name is written
 * by hand, so a registry rename lands everywhere else and silently strands the
 * prose: after `m_snapshot` became `m_state` and `vol_*`/`nonvol_*` became
 * `tagged_*`/`untagged_*`, every one of these bodies still taught the retired
 * names, and nothing failed. Reading the Rust registry directly means the next
 * rename fails HERE until the prose follows it.
 */
describe('the help text speaks the registry vocabulary', () => {
  // Raw source of the Rust registry, via Vite's glob — the same mechanism
  // `fingerprintAxes.test.ts` uses to lock the axis table.
  const rust = Object.values(
    (
      import.meta as unknown as {
        glob(
          pattern: string,
          opts: { eager: true; query: string; import: string },
        ): Record<string, string>;
      }
    ).glob('../../../../../engine/src/metrics/mod.rs', {
      eager: true,
      query: '?raw',
      import: 'default',
    }),
  )[0];

  const groups = new Set([...rust.matchAll(/^\s*name: "(m_[a-z_]+)",$/gm)].map((m) => m[1]));
  const metrics = new Set(
    [...rust.matchAll(/^\s{16}name: "([a-z_]+)",$/gm)].map((m) => m[1]),
  );

  const bodies = (): [string, string][] =>
    [GROUP_HELP, METRIC_HELP, STRICT_PARAM_HELP].flatMap((table) =>
      Object.entries(table).flatMap(([key, tip]): [string, string][] => [
        [key, tip.title],
        [key, tip.body],
      ]),
    );

  it('reads the Rust registry — this guard is the lock', () => {
    expect(rust).toBeTruthy();
    expect(groups.has('m_flow_window')).toBe(true);
    expect(metrics.has('tagged_share')).toBe(true);
  });

  it('names no group the registry does not declare', () => {
    for (const [key, text] of bodies()) {
      for (const [name] of text.matchAll(/\bm_[a-z_]+\b/g)) {
        expect(groups, `${key} names a group the registry does not declare: ${name}`).toContain(
          name,
        );
      }
    }
  });

  it('teaches no retired metric name', () => {
    // Prefixes, not whole words: `vol_buy` and `nonvol_gross` are each a family.
    // Kept explicit rather than derived — a name is retired by a decision, and the
    // registry cannot tell "never existed" from "renamed away".
    const retired = /\b(?:non)?vol_[a-z]+\b|\bsnapshot\b|\bflow_split\b|\bflow_burst\b/;
    for (const [key, text] of bodies()) {
      const hit = text.match(retired);
      expect(hit?.[0], `${key} still teaches the retired name ${hit?.[0]}`).toBeUndefined();
    }
  });
});
