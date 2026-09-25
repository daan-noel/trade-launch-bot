import { useMemo } from 'react';
import { flowPatternKeysFromTags } from 'lib/flow/flowPatternKeys';
import { useGetFingerprintsQuery, useGetStrategyRulesQuery } from 'store/sharedEndpoints';

/**
 * Where a surface's `@tag` / `@!tag` classification comes from: the fingerprint row
 * AND a key set read off it.
 *
 * The id is what matters: a chart handed it classifies with the fingerprint's whole
 * tag (every matcher and option) and writes "add to tag" clicks to that row. The keys
 * are the default tag's exact ix shapes, the fallback for a host that passes keys
 * alone. `tags` is not part of fingerprint identity, so a key set cannot be traced
 * back to one row: a surface handed keys alone can only guess its write target.
 */
export interface FlowPatternSource {
  /** Fingerprint the keys were read from — the row an edit writes to. */
  fingerprintId: string | null;
  /** Its default tag's `ix_shape` keys; `null` when that tag has none. */
  keys: ReadonlySet<string> | null;
}

/** Stable "nothing resolved" source. A fresh object per render would re-render
 *  every chart card in a grid on each parent tick. */
export const NO_FLOW_PATTERN_SOURCE: FlowPatternSource = { fingerprintId: null, keys: null };

/** Resolve the flow-pattern source from a fingerprint id. */
export function useFlowPatternSource(
  fingerprintId: string | null | undefined,
): FlowPatternSource {
  const { data: fingerprints = [] } = useGetFingerprintsQuery(undefined, {
    skip: !fingerprintId,
  });
  return useMemo(() => {
    if (!fingerprintId) return NO_FLOW_PATTERN_SOURCE;
    const fp = fingerprints.find((f) => f.id === fingerprintId);
    // The id stands even when the row has no tag yet - "no tags" is precisely the
    // state an "add to tag" click exists to leave.
    return {
      fingerprintId,
      keys: fp ? flowPatternKeysFromTags(fp.tags) : null,
    };
  }, [fingerprintId, fingerprints]);
}

/**
 * Resolve from a strategy rule id (Console History / Portfolio / open-position
 * detail — surfaces that know the rule but not the fingerprint).
 */
export function useFlowPatternSourceForRule(
  ruleId: string | null | undefined,
): FlowPatternSource {
  const { data: rules = [] } = useGetStrategyRulesQuery(undefined, {
    skip: !ruleId,
  });
  const fingerprintId = useMemo(() => {
    if (!ruleId) return null;
    return rules.find((r) => r.id === ruleId)?.fingerprint_id ?? null;
  }, [ruleId, rules]);
  return useFlowPatternSource(fingerprintId);
}

/**
 * Prefer an explicit fingerprint id; otherwise resolve via rule id. Use when the
 * caller may have only one of the two (e.g. Evidence with a null `rule` prop, or
 * History with `position.rule_id` only).
 */
export function useResolvedFlowPatternSource(opts: {
  fingerprintId?: string | null;
  ruleId?: string | null;
}): FlowPatternSource {
  const fromFp = useFlowPatternSource(opts.fingerprintId);
  const fromRule = useFlowPatternSourceForRule(opts.fingerprintId ? null : opts.ruleId);
  return fromFp.fingerprintId ? fromFp : fromRule;
}

// There is deliberately NO keys-only variant of these hooks. Returning a bare key
// set is what stranded every edit surface without a write target: the id was
// resolved, then dropped one prop before the code that needed it. A caller that
// only classifies reads `.keys` off the source and pays nothing for carrying the
// id along with it.
