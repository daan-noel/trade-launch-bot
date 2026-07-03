import type { RuleRecord } from 'types';

export interface RuleColorInfo {
  /** Class for token_fingerprint group cells (fingerprint params match). */
  fp: string;
  /** Class for entry group cells (fingerprint + entry params match). */
  entry: string;
}

// Fixed palette: 8 colors, two opacity levels per color.
// fp    /8  → fingerprint-group match (same token filter, entry may differ)
// entry /15 → fingerprint + entry match (tightest sub-group)
// All class names spelled out statically so Tailwind's scanner includes them.
const PALETTE: RuleColorInfo[] = [
  { fp: 'bg-blue-400/8',    entry: 'bg-blue-400/15'    },
  { fp: 'bg-amber-400/8',   entry: 'bg-amber-400/15'   },
  { fp: 'bg-pink-400/8',    entry: 'bg-pink-400/15'    },
  { fp: 'bg-emerald-400/8', entry: 'bg-emerald-400/15' },
  { fp: 'bg-purple-400/8',  entry: 'bg-purple-400/15'  },
  { fp: 'bg-orange-400/8',  entry: 'bg-orange-400/15'  },
  { fp: 'bg-cyan-400/8',    entry: 'bg-cyan-400/15'    },
  { fp: 'bg-rose-400/8',    entry: 'bg-rose-400/15'    },
];

function fingerprintKey(r: RuleRecord): string {
  const labels = Array.isArray(r.p_token_ix_labels)
    ? [...r.p_token_ix_labels].sort().join(',')
    : '';
  return [
    r.p_token_initial_buy_sol ?? '',
    r.p_token_cu_limit ?? '',
    r.p_token_cu_price ?? '',
    r.p_token_max_cost_lamports ?? '',
    r.p_token_spendable_lamports_in ?? '',
    labels,
    r.tolerance_pct,
  ].join('|');
}

function entryKey(r: RuleRecord): string {
  return [
    r.p_entry_min_age_secs ?? '',
    r.p_entry_max_age_secs ?? '',
    r.p_entry_min_alive_sol ?? '',
    r.p_entry_min_net_buy_sol ?? '',
    r.p_entry_pullback_pct ?? '',
    r.p_entry_higher_low_secs ?? '',
    r.p_entry_min_liquidity_sol ?? '',
  ].join('|');
}

/**
 * Returns a map from rule ID → RuleColorInfo:
 * - `fp`    class: applied to token_fingerprint group cells when ≥2 rules share fingerprint params
 * - `entry` class: applied to entry group cells when ≥2 rules also share entry params
 * Rules with a unique fingerprint are absent from the map.
 */
export function computeRuleColorClasses(rules: RuleRecord[]): Map<string, RuleColorInfo> {
  // Count fingerprint group sizes.
  const fpCount = new Map<string, number>();
  for (const r of rules) {
    const k = fingerprintKey(r);
    fpCount.set(k, (fpCount.get(k) ?? 0) + 1);
  }

  // Assign a palette index to each fingerprint group with ≥ 2 members.
  const fpColor = new Map<string, number>();
  let next = 0;
  for (const r of rules) {
    const k = fingerprintKey(r);
    if ((fpCount.get(k) ?? 0) >= 2 && !fpColor.has(k)) {
      fpColor.set(k, next % PALETTE.length);
      next++;
    }
  }

  // Count full-group (fingerprint + entry) sizes.
  const fullCount = new Map<string, number>();
  for (const r of rules) {
    const fk = fingerprintKey(r);
    if (!fpColor.has(fk)) continue;
    const k = fk + '\x00' + entryKey(r);
    fullCount.set(k, (fullCount.get(k) ?? 0) + 1);
  }

  // Build the result map.
  const out = new Map<string, RuleColorInfo>();
  for (const r of rules) {
    const fk = fingerprintKey(r);
    const idx = fpColor.get(fk);
    if (idx === undefined) continue;

    const colors = PALETTE[idx];
    const fullK = fk + '\x00' + entryKey(r);
    const entryMatches = (fullCount.get(fullK) ?? 0) >= 2;

    out.set(r.id, entryMatches ? colors : { fp: colors.fp, entry: '' });
  }
  return out;
}
