import { describe, expect, it } from 'vitest';

import { CONDITION_GRAMMAR_HELP, RULE_FIELD_HELP } from './strategyHelp';

/**
 * Metric, span, tag and rule-part text lives in the registry, never here. This locks
 * the page-level help against the retired v1 vocabulary: a help line naming a group
 * the engine no longer has teaches a rule that cannot be written.
 */
const RETIRED = [
  'm_flow_window',
  'm_flow_lifetime',
  'm_price_window',
  'm_price_lifetime',
  'm_flow_ix',
  'm_dump_ix',
  'm_burst_slot',
  'm_burst_wave',
  'm_copy',
  'metric_config',
  'scale_out',
  'entry_event',
  'arm_above_pct',
  'window_size_sec',
  'untagged_',
];

describe('page-level help', () => {
  const texts = [CONDITION_GRAMMAR_HELP, ...Object.values(RULE_FIELD_HELP)].map((t) => `${t.title}\n${t.body}`);

  it('teaches no retired v1 name', () => {
    for (const t of texts) {
      for (const r of RETIRED) expect(t, `help names retired \`${r}\``).not.toContain(r);
    }
  });

  it('keeps metric definitions out of the page help', () => {
    // Per-metric tips come from the registry; a key here would be a second copy.
    for (const k of Object.keys(RULE_FIELD_HELP)) {
      expect(['takeProfit', 'stopLoss', 'reentry', 'exclusive', 'buyPctOfVsol', 'scaleOut']).not.toContain(k);
    }
  });
});
