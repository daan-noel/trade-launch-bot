import { useMemo } from 'react';
import { flowPatternKeysFromMetricConfig } from 'lib/flow/flowPatternKeys';
import { useGetFingerprintsQuery, useGetStrategyRulesQuery } from 'store/sharedEndpoints';

/**
 * Resolve chart vol/non-vol `flowPatternKeys` from a fingerprint id.
 * `null` when the id is missing or the fingerprint has no `volume_ix_patterns`.
 */
export function useFlowPatternKeys(
  fingerprintId: string | null | undefined,
): ReadonlySet<string> | null {
  const { data: fingerprints = [] } = useGetFingerprintsQuery(undefined, {
    skip: !fingerprintId,
  });
  return useMemo(() => {
    if (!fingerprintId) return null;
    const fp = fingerprints.find((f) => f.id === fingerprintId);
    return fp ? flowPatternKeysFromMetricConfig(fp.metric_config) : null;
  }, [fingerprintId, fingerprints]);
}

/**
 * Resolve overlay keys from a strategy rule id (Console History / Portfolio /
 * open-position detail — surfaces that know the rule but not the fingerprint).
 */
export function useFlowPatternKeysForRule(
  ruleId: string | null | undefined,
): ReadonlySet<string> | null {
  const { data: rules = [] } = useGetStrategyRulesQuery(undefined, {
    skip: !ruleId,
  });
  const fingerprintId = useMemo(() => {
    if (!ruleId) return null;
    return rules.find((r) => r.id === ruleId)?.fingerprint_id ?? null;
  }, [ruleId, rules]);
  return useFlowPatternKeys(fingerprintId);
}

/**
 * Prefer an explicit fingerprint id; otherwise resolve via rule id.
 * Use when the caller may have only one of the two (e.g. Evidence with a
 * null `rule` prop, or History with `position.rule_id` only).
 */
export function useResolvedFlowPatternKeys(opts: {
  fingerprintId?: string | null;
  ruleId?: string | null;
}): ReadonlySet<string> | null {
  const fromFp = useFlowPatternKeys(opts.fingerprintId);
  const fromRule = useFlowPatternKeysForRule(opts.fingerprintId ? null : opts.ruleId);
  return fromFp ?? fromRule;
}
