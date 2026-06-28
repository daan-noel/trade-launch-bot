import type { RuleRecord } from 'types';
import type { SweepResultRecord } from '@lab/components/sweep/types';

export type Strategy = 'tpsl1' | 'tpsl2';
export type PasteMode = 'merge' | 'replace';

export interface RuleParamsBlob {
  strategy: Strategy;
  version: 1;
  params: Record<string, unknown>;
}

export interface ApplyResult {
  applied: number;
  skipped: number;
  dropped: number;
}

type Group = 'fingerprint' | 'sizing' | 'exit' | 'entry' | 'mode';

interface ParamMapping {
  field: string;
  group: Group;
  isArray?: boolean;
}

const SHARED_PARAMS: Record<string, ParamMapping> = {
  p_token_initial_buy_sol:   { field: 'initialBuy',          group: 'fingerprint' },
  p_token_cu_limit:          { field: 'cuLimit',             group: 'fingerprint' },
  p_token_cu_price:          { field: 'cuPrice',             group: 'fingerprint' },
  p_token_max_sol_cost:      { field: 'maxSolCost',          group: 'fingerprint' },
  p_token_spendable_sol_in:  { field: 'spendableSolIn',      group: 'fingerprint' },
  p_token_ix_labels:         { field: 'ixLabels',            group: 'fingerprint', isArray: true },
  tolerance_pct:             { field: 'tolerance',           group: 'fingerprint' },
  p_max_concurrent_tokens:   { field: 'maxConcurrentTokens', group: 'sizing' },
  p_max_total_tokens:        { field: 'maxTotalTokens',      group: 'sizing' },
  buy_amount:                { field: 'buyAmount',           group: 'sizing' },
  p_exit_take_profit:        { field: 'takeProfit',          group: 'exit' },
  p_exit_stop_loss:          { field: 'stopLoss',            group: 'exit' },
  p_exit_trailing_stop_pct:  { field: 'trailingStopPct',     group: 'exit' },
  p_exit_time_stop_secs:     { field: 'timeStopSecs',        group: 'exit' },
  p_exit_stall_secs:         { field: 'stallSecs',           group: 'exit' },
  p_exit_liquidity_drop_pct: { field: 'liquidityDropPct',    group: 'exit' },
  trade_mode:                { field: 'tradeMode',           group: 'mode' },
};

const TPSL2_EXTRA: Record<string, ParamMapping> = {
  p_entry_min_age_secs:      { field: 'minAgeSecs',      group: 'entry' },
  p_entry_max_age_secs:      { field: 'maxAgeSecs',      group: 'entry' },
  p_entry_min_alive_sol:     { field: 'minAliveSol',     group: 'entry' },
  p_entry_min_organic_sol:   { field: 'minOrganicSol',   group: 'entry' },
  p_entry_pullback_pct:      { field: 'pullbackPct',     group: 'entry' },
  p_entry_higher_low_secs:   { field: 'higherLowSecs',   group: 'entry' },
  p_entry_max_cohort_held:   { field: 'maxCohortHeld',   group: 'entry' },
  p_entry_min_liquidity_sol: { field: 'minLiquiditySol', group: 'entry' },
  p_entry_min_organic_liq:   { field: 'minOrganicLiq',   group: 'entry' },
  p_exit_cohort_ratio:       { field: 'cohortExitRatio', group: 'exit'  },
};

function mappingFor(strategy: Strategy): Record<string, ParamMapping> {
  return strategy === 'tpsl2' ? { ...SHARED_PARAMS, ...TPSL2_EXTRA } : { ...SHARED_PARAMS };
}

/** Serialize a rule's editable params to a clipboard-ready JSON blob. */
export function ruleToParamsJson(rule: RuleRecord, strategy: Strategy): string {
  const r = rule as unknown as Record<string, unknown>;
  const mapping = mappingFor(strategy);
  const params: Record<string, unknown> = {};
  for (const paramKey of Object.keys(mapping)) {
    params[paramKey] = r[paramKey] ?? null;
  }
  return JSON.stringify({ strategy, version: 1, params } satisfies RuleParamsBlob, null, 2);
}

// Sweep combo param keys lack the `p_` prefix (e.g. `exit_take_profit`).
// Normalize to the canonical form before wrapping so blobs are uniform.
function sweepKeyToParamKey(key: string): string {
  return key.startsWith('exit_') || key.startsWith('entry_') ? `p_${key}` : key;
}

/** Serialize a sweep combo's swept params to a clipboard-ready JSON blob.
 *  Combos only carry swept fields, so this is a partial paste. */
export function sweepComboToParamsJson(combo: SweepResultRecord, strategy: Strategy): string {
  const params: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(combo.params)) {
    params[sweepKeyToParamKey(k)] = v;
  }
  return JSON.stringify({ strategy, version: 1, params } satisfies RuleParamsBlob, null, 2);
}

/** Parse raw text as a RuleParamsBlob. Returns null on invalid JSON or missing fields. */
export function parseParamsBlob(raw: string): RuleParamsBlob | null {
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (typeof parsed !== 'object' || parsed === null) return null;
    const b = parsed as Record<string, unknown>;
    if (b.strategy !== 'tpsl1' && b.strategy !== 'tpsl2') return null;
    if (b.version !== 1) return null;
    if (typeof b.params !== 'object' || b.params === null || Array.isArray(b.params)) return null;
    return b as unknown as RuleParamsBlob;
  } catch {
    return null;
  }
}

/**
 * Apply a parsed blob to a form state (represented as a string-value map).
 *
 * merge  â€” overwrite only the fields present in the blob.
 * replace â€” reset to emptyForm first, then apply; ruleName is always kept.
 *
 * When `live` is true, only sizing-group params are applied; all other groups
 * are skipped to match the modal's lock-group guards.
 *
 * Callers own the typed form and cast to/from `Record<string,string>` so this
 * function stays generic-free.
 */
export function applyParamsToForm(
  current: Record<string, string>,
  emptyFn: () => Record<string, string>,
  blob: RuleParamsBlob,
  strategy: Strategy,
  live: boolean,
  mode: PasteMode,
): { next: Record<string, string>; result: ApplyResult } {
  const mapping = mappingFor(strategy);
  const base: Record<string, string> = mode === 'replace' ? { ...emptyFn() } : { ...current };
  base.ruleName = current.ruleName;

  let applied = 0;
  let skipped = 0;
  let dropped = 0;

  for (const [paramKey, value] of Object.entries(blob.params)) {
    const m = mapping[paramKey];
    if (!m) { dropped++; continue; }
    if (live && m.group !== 'sizing') { skipped++; continue; }
    let strVal: string;
    if (m.isArray) {
      strVal = Array.isArray(value) ? JSON.stringify(value) : '';
    } else if (value === null || value === undefined) {
      strVal = '';
    } else {
      strVal = String(value);
    }
    base[m.field] = strVal;
    applied++;
  }

  return { next: base, result: { applied, skipped, dropped } };
}
