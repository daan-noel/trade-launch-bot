import { useCallback, useMemo, useState } from 'react';

import { flowPatternKeysOf } from 'lib/flow/flowPatternKeys';
import { patternKey, patternsFromKeys, togglePattern } from 'lib/flow/volumePatterns';
import {
  ixPatternsFromConfig,
  metricConfigWithList,
  metricConfigWithWorkingTemplates,
  patternsForList,
  workingTemplatesFromConfig,
  type IxPatternList,
} from 'lib/strategy/registry';
import { isLaunchGrain, templateGrain, toggleWorkingTemplate } from 'lib/strategy/templateGrain';
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

export interface IxPatternTargetChoice {
  targetId: string | null;
  inferred: boolean;
  offHost: boolean;
}

/**
 * Which fingerprint a Tagged-badge edit lands on, as a pure function of its inputs —
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
export function resolveIxPatternTarget(input: {
  pickedId: string | null;
  hostFingerprintId: string | null;
  matchIds: readonly string[];
}): IxPatternTargetChoice {
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

export type TapeList = IxPatternList | 'working';

/** The write used to rebuild `m_flow_ix` from the pattern rows alone, which deleted
 *  the marker masks and reverted `wallet_contagion` / `creator_is_tagged` to their
 *  `true` backend defaults — a different classifier, not a tighter one — on every
 *  badge toggle. `metricConfigWithList` routes through each group's own writer, which
 *  preserves what this surface does not render. */
const patternsOf = patternsForList;

export interface IxPatternTarget {
  /** Fingerprint a toggle writes to; `null` ⇒ the badge is read-only. */
  target: Fingerprint | null;
  /** Every fingerprint, for the target picker. */
  fingerprints: Fingerprint[];
  targetId: string | null;
  setTargetId: (id: string | null) => void;
  /** Which list a toggle writes into. */
  list: TapeList;
  setList: (list: TapeList) => void;
  /** The ACTIVE list's patterns — what the badge classifies against. Empty when
   *  {@link list} is `'working'` (use {@link workingTemplates}). */
  patterns: string[][];
  /** Grain ids when {@link list} is `'working'`. */
  workingTemplates: string[];
  /** Keys of the list a toggle is NOT writing into, so a row can show that the
   *  other one also counts it. A build may sit on both - the mark is information,
   *  not a conflict. */
  otherKeys: ReadonlySet<string> | null;
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
 * Which fingerprint a Tagged-badge toggle edits, and the write itself.
 *
 * `ix_patterns` lives on exactly one row — the fingerprint — and that row is
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
 * it is reported as {@link IxPatternTarget.inferred} and taken only when
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
export function useIxPatternTarget({
  fingerprintId = null,
  savedKeys = null,
  enabled = true,
}: {
  fingerprintId?: string | null;
  savedKeys?: ReadonlySet<string> | null;
  enabled?: boolean;
} = {}): IxPatternTarget {
  const { data: fingerprints = [] } = useGetFingerprintsQuery(undefined, { skip: !enabled });
  const { data: rules = [] } = useGetStrategyRulesQuery(undefined, { skip: !enabled });
  const [updateFingerprint, { isLoading: saving }] = useUpdateFingerprintMutation();

  const [pickedId, setPickedId] = useState<string | null>(null);
  const [list, setList] = useState<TapeList>('tagged');
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
            samePatternSet(ixPatternsFromConfig(f.metric_config), savedPatterns),
          )
        : NO_MATCHES,
    [needsMatch, fingerprints, savedPatterns],
  );

  const { targetId, inferred, offHost } = resolveIxPatternTarget({
    pickedId,
    hostFingerprintId: fingerprintId,
    matchIds: matches.map((f) => f.id),
  });
  const target = useMemo(
    () => fingerprints.find((f) => f.id === targetId) ?? null,
    [fingerprints, targetId],
  );

  const patterns = useMemo(
    () =>
      list === 'working'
        ? []
        : target
          ? patternsOf(target.metric_config, list)
          : savedPatterns,
    [target, savedPatterns, list],
  );

  const workingTemplates = useMemo(
    () => (target ? workingTemplatesFromConfig(target.metric_config) : []),
    [target],
  );

  // The other list, for the "also dump" / "also tagged" marker. Empty without a
  // target, and unused on the working list (grain ids are a different vocabulary).
  const otherKeys = useMemo(
    () =>
      target && list !== 'working'
        ? flowPatternKeysOf(patternsOf(target.metric_config, list === 'dump' ? 'tagged' : 'dump'))
        : null,
    [target, list],
  );

  // The badge classifies against the row it writes to, or it reports a state its
  // own click cannot change. With no target that is the host's set unchanged —
  // reused by reference, since a re-parsed copy would rebuild every column.
  const keys = useMemo(
    () =>
      list === 'working'
        ? new Set(workingTemplates)
        : target
          ? flowPatternKeysOf(patterns)
          : (savedKeys ?? null),
    [target, patterns, savedKeys, list, workingTemplates],
  );

  const activeRuleCount = useMemo(
    () => (targetId ? rules.filter((r) => r.fingerprint_id === targetId && r.is_active).length : 0),
    [rules, targetId],
  );

  const toggle = useCallback(
    (labels: readonly string[]) => {
      if (!target || labels.length === 0) return;
      if (list === 'working' && isLaunchGrain(labels)) return;
      setError(null);
      const metric_config =
        list === 'working'
          ? metricConfigWithWorkingTemplates(
              target.metric_config ?? {},
              toggleWorkingTemplate(
                workingTemplatesFromConfig(target.metric_config),
                templateGrain(labels),
              ),
            )
          : metricConfigWithList(
              target.metric_config ?? {},
              togglePattern(patternsOf(target.metric_config, list), labels),
              list,
            );
      // Fire-and-report: the mutation invalidates `Fingerprint`, which re-derives the
      // chart's keys AND refetches the metric series, so both redraw from the row
      // that was just written rather than from any local echo of it.
      void updateFingerprint({
        id: target.id,
        body: {
          name: target.name,
          // The whole criteria map is round-tripped: a PUT replaces the row, so an
          // omitted axis would silently WIDEN what this fingerprint matches. Same
          // reason `wildcard` is sent — omitted it defaults to false, which the
          // write edge then rejects as criterion-less.
          criteria: target.criteria,
          wildcard: target.wildcard,
          metric_config,
        },
      })
        .unwrap()
        .catch((e) =>
          setError(
            apiErrorMessage(
              e as never,
              list === 'working' ? 'Failed to save working templates' : `Failed to save ${list} patterns`,
            ),
          ),
        );
    },
    [target, updateFingerprint, list],
  );

  return {
    target,
    fingerprints,
    targetId,
    setTargetId: setPickedId,
    list,
    setList,
    patterns,
    workingTemplates,
    otherKeys,
    keys,
    activeRuleCount,
    toggle: target ? toggle : null,
    inferred,
    offHost,
    saving,
    error,
  };
}
