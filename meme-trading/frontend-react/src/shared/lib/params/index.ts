// Public surface of the unified rule-params engine. Import from here (or the
// specific submodules) — never reach into `engine`/`specs` piecemeal from pages.

export * from './types';
export * from './engine';

import type { RuleParamsBlob, StrategySpec, Strategy } from './types';
import { TPSL1_SPEC } from './specs/tpsl1';
import { TPSL2_SPEC } from './specs/tpsl2';
import { SWING1_SPEC } from './specs/swing1';

export { TPSL1_SPEC, TPSL2_SPEC, SWING1_SPEC };

const SPECS: Record<Strategy, StrategySpec> = {
  tpsl1: TPSL1_SPEC,
  tpsl2: TPSL2_SPEC,
  swing_1: SWING1_SPEC,
};

/** The spec for a strategy. */
export function getSpec(strategy: Strategy): StrategySpec {
  return SPECS[strategy];
}

// ── swing1 detect-page adapter (spec-derived) ───────────────────────────────
//
// The detect page keeps its own flat map keyed by `Swing1DetectParams` bare names
// (`take_profit`, `kill_depth_min_pct`, `exit_next_kill_depth_min_pct`, …). These
// two helpers translate between that map and the `p_*`-keyed blob using the spec's
// `detectKey`, so there's no free-floating key table.

const SWING1_DETECT = SWING1_SPEC.fields.filter((f) => f.detectKey);

/** detectKey → column, built from the spec. */
const DETECT_TO_COL = new Map(SWING1_DETECT.map((f) => [f.detectKey!, f.column]));
/** column → detectKey, built from the spec. */
const COL_TO_DETECT = new Map(SWING1_DETECT.map((f) => [f.column, f.detectKey!]));

/** Serialize the detect page's flat param map to a blob (column-keyed), for the
 *  detect ⎘ copy button. Unknown detect keys are dropped. */
export function detectParamsToBlob(
  params: Record<string, number | null | undefined>,
): RuleParamsBlob {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(params)) {
    const col = DETECT_TO_COL.get(k);
    if (col) out[col] = v ?? null;
  }
  return { strategy: 'swing_1', version: 1, params: out };
}

/** JSON text of {@link detectParamsToBlob}. */
export function detectParamsToJson(
  params: Record<string, number | null | undefined>,
): string {
  return JSON.stringify(detectParamsToBlob(params), null, 2);
}

/** Map a swing1 blob (rule/combo/detect copy) to the detect page's flat param map.
 *  `allowedKeys` is the set of `Swing1DetectParams` keys (passed in so this stays
 *  free of a lab-tree import). Numbers pass through, nulls → null (off); anything
 *  else (arrays/strings — e.g. `p_token_ix_labels`) is dropped as non-scalar. */
export function blobToDetectParams(
  blob: RuleParamsBlob,
  allowedKeys: Set<string>,
): { params: Record<string, number | null>; applied: number; dropped: number } {
  const params: Record<string, number | null> = {};
  let applied = 0;
  let dropped = 0;
  for (const [col, value] of Object.entries(blob.params)) {
    const key = COL_TO_DETECT.get(col);
    if (!key || !allowedKeys.has(key)) { dropped++; continue; }
    if (value === null || value === undefined) {
      params[key] = null;
    } else if (typeof value === 'number') {
      params[key] = value;
    } else {
      dropped++;
      continue;
    }
    applied++;
  }
  return { params, applied, dropped };
}
