import { patternKeysFrom } from 'lib/flow/classifyFlow';
import { volumeIxPatternsFromConfig } from 'lib/strategy/registry';

/**
 * Non-empty `JSON.stringify(labels)` key set for the chart vol/non-vol overlay,
 * or `null` when unconfigured (same gate as `TokenPriceChart` / trades Vol badge).
 */
export function flowPatternKeysOf(
  patterns: readonly (readonly string[])[] | null | undefined,
): ReadonlySet<string> | null {
  if (!patterns?.length) return null;
  const keys = patternKeysFrom(patterns);
  return keys.size > 0 ? keys : null;
}

/** Read keys from a fingerprint `metric_config` blob. */
export function flowPatternKeysFromMetricConfig(
  cfg: Record<string, unknown> | null | undefined,
): ReadonlySet<string> | null {
  return flowPatternKeysOf(volumeIxPatternsFromConfig(cfg));
}
