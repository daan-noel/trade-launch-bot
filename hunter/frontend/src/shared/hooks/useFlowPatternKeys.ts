import { useMemo } from 'react';
import { flowPatternKeysFromMetricConfig } from 'lib/flow/flowPatternKeys';
import { useGetFingerprintsQuery, useGetStrategyRulesQuery } from 'store/sharedEndpoints';

/**
 * Where a surface's vol/non-vol classification comes from: the keys to classify
 * with AND the fingerprint row they were read off.
 *
 * The id travels WITH the keys because editing needs it and a key set cannot be
 * traced back to one row. `metric_config` is not part of fingerprint identity, so
 * any number of fingerprints may carry the same patterns — and every unconfigured
 * one carries the same empty set, which is exactly the state authoring starts
 * from. A surface handed keys alone has to guess its write target, and both
 * outcomes are wrong: refuse (the Tagged badge goes dead) or guess (the write lands
 * on an unrelated rule's fingerprint).
 *
 * Prefer these hooks over the keys-only wrappers below wherever the surface can
 * edit patterns; the wrappers are for read-only classification (chart overlay).
 */
export interface FlowPatternSource {
  /** Fingerprint the keys were read from — the row an edit writes to. */
  fingerprintId: string | null;
  /** Its `ix_patterns` keys; `null` when the row has none configured. */
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
    // The id stands even when the row carries no patterns yet — "unconfigured"
    // is precisely the state a Tagged-badge edit exists to leave.
    return {
      fingerprintId,
      keys: fp ? flowPatternKeysFromMetricConfig(fp.metric_config) : null,
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
