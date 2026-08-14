import { useCallback, useMemo, useState } from 'react';

import { flowPatternKeysOf } from 'lib/flow/flowPatternKeys';
import { patternKey, patternsFromKeys, togglePattern } from 'lib/flow/volumePatterns';
import { metricConfigWithVolumePatterns, volumeIxPatternsFromConfig } from 'lib/strategy/registry';
import { apiErrorMessage } from 'store/apiSlice';
import {
  useGetFingerprintsQuery,
  useGetStrategyRulesQuery,
  useUpdateFingerprintMutation,
} from 'store/sharedEndpoints';
import type { Fingerprint } from 'lib/strategy/types';

/** Same-membership test on two pattern sets. Order between patterns carries no
 *  meaning; order INSIDE a pattern does, and `patternKey` preserves it. */
function samePatternSet(
  a: readonly (readonly string[])[],
  b: readonly (readonly string[])[],
): boolean {
  if (a.length !== b.length) return false;
  const bKeys = new Set(b.map(patternKey));
  return a.every((p) => bKeys.has(patternKey(p)));
}

/** Stable empty result for the skipped match pass. */
const NO_MATCHES: Fingerprint[] = [];

export interface VolumePatternTargetChoice {
  targetId: string | null;
  inferred: boolean;
  offHost: boolean;
}

/**
 * Which fingerprint a Vol-badge edit lands on, as a pure function of its inputs —
 * the whole precedence rule in one testable place, since this repo's tests run on
 * `node` and the hook around it needs a React tree.
 *
 * Precedence is explicit pick > the host's own fingerprint > a pattern-set match.
 * The order is the point: a match on the SET can never outrank an id, because an
 * unconfigured host classifies with the empty set and every unconfigured
 * fingerprint carries that same set. Reading the match first therefore fails
 * exactly when authoring starts — either uneditable (several rows match) or,
 * worse, silently writing to whichever single unrelated row happened to be empty.
 *
 * @param matchIds fingerprints carrying the host's set; a match is taken only when
 *                 there is exactly one, and is always reported as `inferred`
 */
export function resolveVolumePatternTarget(input: {
  pickedId: string | null;
  hostFingerprintId: string | null;
  matchIds: readonly string[];
}): VolumePatternTargetChoice {
  const { pickedId, hostFingerprintId, matchIds } = input;
  // Only consulted when nothing better exists — a guess must never displace a fact.
  const inferredId =
    pickedId == null && !hostFingerprintId && matchIds.length === 1 ? matchIds[0] : null;
  const targetId = pickedId ?? hostFingerprintId ?? inferredId;
  return {
    targetId,
    inferred: targetId != null && targetId === inferredId,
    offHost: targetId != null && hostFingerprintId != null && targetId !== hostFingerprintId,
  };
}

/** `metric_config` with only the flow key replaced — every other group's config on
 *  the row survives the write. Dropping the last pattern removes the key entirely,
 *  which is how a fingerprint returns to "flow unconfigured" (metrics read `NaN`). */
function withVolumePatterns(
  cfg: Record<string, unknown> | null | undefined,
  patterns: string[][],
): Record<string, unknown> {
  const { m_flow_split: _flow, ...rest } = cfg ?? {};
  return { ...rest, ...metricConfigWithVolumePatterns(patterns) };
}

export interface VolumePatternTarget {
  /** Fingerprint a toggle writes to; `null` ⇒ the badge is read-only. */
  target: Fingerprint | null;
  /** Every fingerprint, for the target picker. */
  fingerprints: Fingerprint[];
  targetId: string | null;
  setTargetId: (id: string | null) => void;
  /** The target's current patterns — what the badge classifies against. */
  patterns: string[][];
  /** {@link patterns} as `flowPatternKeys`. Falls back to the host's own keys
   *  when nothing is targeted, so a read-only panel classifies as before. */
  keys: ReadonlySet<string> | null;
  /** Active rules bound to the target — a write changes what they all mean. */
  activeRuleCount: number;
  /** Add/remove one ordered `ix_labels` sequence and persist it. No-op with no target. */
  toggle: ((labels: readonly string[]) => void) | null;
  /** The target was matched by pattern set, not handed down — it is a guess, and
   *  the picker must say so rather than presenting it as the host's fingerprint. */
  inferred: boolean;
  /** The target is NOT the fingerprint this host classifies with, so the badge
   *  and the chart's lines above it now answer for different rows. Only reachable
   *  by an explicit pick, and must be surfaced wherever true. */
  offHost: boolean;
  saving: boolean;
  error: string | null;
}

/**
 * Which fingerprint a Vol-badge toggle edits, and the write itself.
 *
 * `volume_ix_patterns` lives on exactly one row — the fingerprint — and that row is
 * what the chart lines, the metric panes and the running engine all classify from.
 * So a toggle edits it directly: a staging copy would be a second answer to "what
 * counts as volume", and the surfaces reading the two copies then disagree on
 * screen while both look authoritative. The engine picks the edit up on its next
 * rules reload (`FlowState::set_patterns`), which is the point of writing through.
 *
 * The target is the host's OWN fingerprint whenever it knows one — a position, a
 * sim result and a rule's evidence all classify against a specific row, and asking
 * the reader to re-pick a row the app already resolved is both friction and a
 * chance to pick wrong. Only a host with no fingerprint at all (plain token detail,
 * flow preview) falls back to matching by pattern set, and that match is a guess:
 * it is reported as {@link VolumePatternTarget.inferred} and taken only when
 * exactly one row carries the set. Otherwise the target stays `null` and the caller
 * picks, because a write changes flow classification for every active rule bound to
 * that fingerprint.
 *
 * @param fingerprintId the host's fingerprint — the write target when known
 * @param savedKeys     the host's current pattern keys; matched against only when
 *                      there is no `fingerprintId` to use
 * @param enabled       `false` on a read-only host (a stored run's frozen snapshot),
 *                      which skips the fingerprint/rule fetches entirely
 */
export function useVolumePatternTarget({
  fingerprintId = null,
  savedKeys = null,
  enabled = true,
}: {
  fingerprintId?: string | null;
  savedKeys?: ReadonlySet<string> | null;
  enabled?: boolean;
} = {}): VolumePatternTarget {
  const { data: fingerprints = [] } = useGetFingerprintsQuery(undefined, { skip: !enabled });
  const { data: rules = [] } = useGetStrategyRulesQuery(undefined, { skip: !enabled });
  const [updateFingerprint, { isLoading: saving }] = useUpdateFingerprintMutation();

  const [pickedId, setPickedId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const savedPatterns = useMemo(() => patternsFromKeys(savedKeys), [savedKeys]);

  // Matching by pattern set is the last resort, so it runs only when there is
  // nothing better. An empty set matches every unconfigured fingerprint at once,
  // which is why it can never outrank an id the host actually knows.
  const needsMatch = pickedId == null && !fingerprintId;
  const matches = useMemo(
    () =>
      needsMatch
        ? fingerprints.filter((f) =>
            samePatternSet(volumeIxPatternsFromConfig(f.metric_config), savedPatterns),
          )
        : NO_MATCHES,
    [needsMatch, fingerprints, savedPatterns],
  );

  const { targetId, inferred, offHost } = resolveVolumePatternTarget({
    pickedId,
    hostFingerprintId: fingerprintId,
    matchIds: matches.map((f) => f.id),
  });
  const target = useMemo(
    () => fingerprints.find((f) => f.id === targetId) ?? null,
    [fingerprints, targetId],
  );

  const patterns = useMemo(
    () => (target ? volumeIxPatternsFromConfig(target.metric_config) : savedPatterns),
    [target, savedPatterns],
  );

  // The badge classifies against the row it writes to, or it reports a state its
  // own click cannot change. With no target that is the host's set unchanged —
  // reused by reference, since a re-parsed copy would rebuild every column.
  const keys = useMemo(
    () => (target ? flowPatternKeysOf(patterns) : (savedKeys ?? null)),
    [target, patterns, savedKeys],
  );

  const activeRuleCount = useMemo(
    () => (targetId ? rules.filter((r) => r.fingerprint_id === targetId && r.is_active).length : 0),
    [rules, targetId],
  );

  const toggle = useCallback(
    (labels: readonly string[]) => {
      if (!target || labels.length === 0) return;
      const next = togglePattern(volumeIxPatternsFromConfig(target.metric_config), labels);
      setError(null);
      // Fire-and-report: the mutation invalidates `Fingerprint`, which re-derives the
      // chart's keys AND refetches the metric series, so both redraw from the row
      // that was just written rather than from any local echo of it.
      void updateFingerprint({
        id: target.id,
        body: {
          name: target.name,
          cu_limit: target.cu_limit,
          cu_price: target.cu_price,
          init_buy_lamports: target.init_buy_lamports,
          max_cost_lamports: target.max_cost_lamports,
          spendable_lamports_in: target.spendable_lamports_in,
          first_slot_buy_lamports: target.first_slot_buy_lamports,
          first_slot_sell_lamports: target.first_slot_sell_lamports,
          bucket_size_amount: target.bucket_size_amount,
          ix_labels: target.ix_labels,
          metric_config: withVolumePatterns(target.metric_config, next),
        },
      })
        .unwrap()
        .catch((e) => setError(apiErrorMessage(e as never, 'Failed to save volume patterns')));
    },
    [target, updateFingerprint],
  );

  return {
    target,
    fingerprints,
    targetId,
    setTargetId: setPickedId,
    patterns,
    keys,
    activeRuleCount,
    toggle: target ? toggle : null,
    inferred,
    offHost,
    saving,
    error,
  };
}
