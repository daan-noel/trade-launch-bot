import { describe, expect, it } from 'vitest';
import {
  FINGERPRINT_FIELD_HELP,
  GROUP_HELP,
  METRIC_HELP,
  groupHelpTip,
  metricHelpBody,
  STRICT_PARAM_HELP,
} from './strategyHelp';

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

describe('groupHelpTip', () => {
  it('renders the REGISTRY description first, not the frontend copy', () => {
    const group = Object.keys(GROUP_HELP).find((k) => GROUP_HELP[k]?.body)!;
    const tip = groupHelpTip(group, { description: 'The one-line registry definition.' });
    expect(tip?.body.startsWith('The one-line registry definition.')).toBe(true);
    expect(tip?.body.indexOf(GROUP_HELP[group].body)).toBeGreaterThan(0);
  });

  it('falls back to GROUP_HELP when the payload carries no description', () => {
    const group = Object.keys(GROUP_HELP).find((k) => GROUP_HELP[k]?.body)!;
    const tip = groupHelpTip(group);
    expect(tip?.body).toBe(GROUP_HELP[group].body);
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
  // Every metric name, taken from the `MetricSpec` block it is declared in rather than
  // from its indentation: a `\s{16}` match is a guard that silently stops guarding the
  // day rustfmt or a nesting change moves the column, and an empty metric set makes
  // every assertion below vacuously pass.
  const metrics = new Set(
    [...rust.matchAll(/MetricSpec\s*\{[\s\S]*?\bname:\s*"([a-z_]+)"/g)].map((m) => m[1]),
  );

  const bodies = (): [string, string][] =>
    [GROUP_HELP, METRIC_HELP, STRICT_PARAM_HELP, FINGERPRINT_FIELD_HELP].flatMap((table) =>
      Object.entries(table).flatMap(([key, tip]): [string, string][] => [
        [key, tip.title],
        [key, tip.body],
      ]),
    );

  it('reads the Rust registry — this guard is the lock', () => {
    expect(rust).toBeTruthy();
    expect(groups.has('m_flow_window')).toBe(true);
    expect(metrics.has('tagged_share')).toBe(true);
    // A regex that stops matching makes every assertion below vacuously pass, so the
    // lock asserts it actually harvested a registry-sized vocabulary.
    expect(groups.size).toBeGreaterThanOrEqual(8);
    expect(metrics.size).toBeGreaterThanOrEqual(30);
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

  it('documents no metric the registry does not declare', () => {
    // The retired-name regex below is a list a human maintains; this is not. A metric
    // deleted from the registry leaves its METRIC_HELP entry unreachable
    // (`METRIC_HELP[row.metric]` only ever sees registry rows) and silently stale —
    // which is how `first_slot_buy`, `ix_count` and `prior_launches` outlived their
    // move to the fingerprint axes. Keys here are metric names and nothing else:
    // a strict param belongs in STRICT_PARAM_HELP.
    for (const key of Object.keys(METRIC_HELP)) {
      expect(metrics, `METRIC_HELP documents ${key}, which is not a registry metric`)
        .toContain(key);
    }
  });

  it('names no metric the registry does not declare, inside the prose too', () => {
    // The key check above is not enough: `ix_count` and `prior_launches` survived in
    // m_state's BODY for months after they moved to the fingerprint axes, because
    // nothing read the prose. Any snake_case token in a tip has to be registry
    // vocabulary - a metric, a group, a strict param or a fingerprint-config field -
    // or an English phrase listed here on purpose.
    // Strict params and config keys are mostly named through constants, so harvest the
    // constant VALUES across the metrics modules rather than the identifiers.
    const params = new Set(
      Object.values(
        import.meta.glob('../../../../../engine/src/metrics/*.rs', {
          eager: true,
          query: '?raw',
          import: 'default',
        }),
      ).flatMap((src) =>
        [...(src as string).matchAll(/const\s+[A-Z_0-9]+:\s*&str\s*=\s*"([a-z_]+)"/g)].map(
          (m) => m[1],
        ),
      ),
    );
    const fpFields = new Set(
      [...rust.matchAll(/FpConfigFieldSpec\s*\{[\s\S]*?\bname:\s*"([a-z_]+)"/g)].map((m) => m[1]),
    );
    // Fingerprint axes are vocabulary too - a tip may send a reader to `ix_count` or
    // `prior_launches`, as long as it does not call them metrics. Read from the axis
    // table so the list cannot drift.
    const axisRust = Object.values(
      import.meta.glob('../../../../../engine/src/fingerprint/axis.rs', {
        eager: true,
        query: '?raw',
        import: 'default',
      }),
    )[0] as string;
    const axes = new Set([...axisRust.matchAll(/\bkey:\s*"([a-z_]+)"/g)].map((m) => m[1]));
    // Words that are prose, not vocabulary: lake columns, on-chain field names and the
    // window params the strict-param specs name through constants.
    const PROSE = new Set([
      'ix_labels',
      'ix_patterns',
      'cu_limit',
      'cu_price',
      'tip_lamports',
      'window_size_sec',
      'window_lag',
      'after_age_sec',
      'take_profit',
      'stop_loss',
      'creator_wallet',
      'first_slot',
      'tx_index',
    ]);
    const known = (w: string) =>
      metrics.has(w) || groups.has(w) || params.has(w) || fpFields.has(w) || axes.has(w) ||
      PROSE.has(w);

    for (const [key, text] of bodies()) {
      for (const [word] of text.matchAll(/\b[a-z][a-z0-9]*(?:_[a-z0-9]+)+\b/g)) {
        if (word.startsWith('m_') || word.endsWith('_*') || word.includes('*')) continue;
        expect(known(word), `${key} names ${word}, which the registry does not declare`).toBe(true);
      }
    }
  });

  it('lists only its own metrics as a group tip bullet', () => {
    // The exact shape of the bug: m_state's tip carried "• ix_count — instructions in
    // the CREATION transaction", a bullet reading as a metric of that group, for an
    // axis that had moved out. A bullet in a GROUP_HELP body is a claim of membership.
    const byGroup = new Map<string, Set<string>>();
    for (const block of rust.split(/GroupSpec\s*\{/).slice(1)) {
      const g = block.match(/^\s*[\s\S]*?\bname:\s*"(m_[a-z_]+)"/)?.[1];
      if (!g) continue;
      byGroup.set(
        g,
        new Set([...block.matchAll(/MetricSpec\s*\{[\s\S]*?\bname:\s*"([a-z_]+)"/g)].map((m) => m[1])),
      );
    }
    for (const [group, tip] of Object.entries(GROUP_HELP)) {
      const set = byGroup.get(group);
      if (!set) continue;
      for (const [, name] of tip.body.matchAll(/^[•*-]\s+([a-z][a-z0-9_]*)\s+[—-]/gm)) {
        expect(set.has(name), `${group}'s tip bullets ${name}, which it does not declare`)
          .toBe(true);
      }
    }
  });

  it('attaches a metric to the group that actually declares it', () => {
    // `m_group.metric` in a tip is a promise about where that metric lives. A metric
    // moved between groups leaves the old spelling readable and wrong.
    const byGroup = new Map<string, Set<string>>();
    for (const block of rust.split(/GroupSpec\s*\{/).slice(1)) {
      const g = block.match(/^\s*[\s\S]*?\bname:\s*"(m_[a-z_]+)"/)?.[1];
      if (!g) continue;
      byGroup.set(
        g,
        new Set([...block.matchAll(/MetricSpec\s*\{[\s\S]*?\bname:\s*"([a-z_]+)"/g)].map((m) => m[1])),
      );
    }
    for (const [key, text] of bodies()) {
      for (const [, group, metric] of text.matchAll(/\b(m_[a-z_]+)\.([a-z][a-z0-9_]*)\b/g)) {
        const set = byGroup.get(group);
        if (!set) continue; // the group check above owns an unknown group name
        if (!metrics.has(metric)) continue; // a config field or param, not a metric
        expect(set.has(metric), `${key} puts ${metric} in ${group}, which does not declare it`)
          .toBe(true);
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
