import { useCallback, useMemo, useState } from 'react';

import type { FlowTag } from 'lib/flow/classifyFlow';
import { flowPatternKeysFromTags } from 'lib/flow/flowPatternKeys';
import { defaultTagName, flowTagOf, shapeTag } from 'lib/flow/tapeClassify';
import { patternKey, patternsFromKeys } from 'lib/flow/volumePatterns';
import { ixLabelsActions } from 'lib/ixLabels';
import {
  formatFeePins,
  patternRowKey,
  rowFromTrade,
  type IxPatternFeeMask,
  type IxPatternFeeSource,
  type IxPatternRow,
} from 'lib/strategy/ixPatternRows';
import { tagNames, withTagListValue, withTagShape } from 'lib/strategy/tagsDoc';
import { templateGrain, templateProgram } from 'lib/strategy/templateGrain';
import type { Fingerprint } from 'lib/strategy/types';
import { apiErrorMessage } from 'store/apiSlice';
import {
  useGetFingerprintsQuery,
  useGetStrategyRulesQuery,
  useUpdateFingerprintMutation,
} from 'store/sharedEndpoints';

// ── What a click writes ──────────────────────────────────────────────────────

/** The tag matchers a trade click can write: the trade's exact ix shape, its ix
 *  template, its program, or its wallet. */
export type StageMatcher = 'ix_shape' | 'ix_template' | 'program' | 'wallet';

export const STAGE_MATCHERS: readonly StageMatcher[] = ['ix_shape', 'ix_template', 'program', 'wallet'];

/** The trade fields a click reads. */
export interface StageTrade extends IxPatternFeeSource {
  instruction_labels?: readonly string[] | null;
  wallet_address?: string | null;
}

/** The one value a click on a trade writes under a matcher. */
export type StageValue =
  | { matcher: 'ix_shape'; row: IxPatternRow }
  | { matcher: 'ix_template' | 'program' | 'wallet'; value: string };

/** The value a click on `t` writes under `matcher` - its labels plus the pinned fee
 *  fields, its template, its program or its wallet. `null` = nothing to write (no
 *  labels captured, or no wallet). */
export function stageValueOf(
  matcher: StageMatcher,
  t: StageTrade,
  feePins?: IxPatternFeeMask | null,
): StageValue | null {
  const labels = t.instruction_labels ?? [];
  switch (matcher) {
    case 'wallet':
      return t.wallet_address ? { matcher, value: t.wallet_address } : null;
    case 'ix_shape':
      return labels.length > 0 ? { matcher, row: rowFromTrade(labels, t, feePins) } : null;
    case 'ix_template':
      return labels.length > 0 ? { matcher, value: templateGrain(labels) } : null;
    case 'program':
      return labels.length > 0 ? { matcher, value: templateProgram(labels) } : null;
  }
}

/** A value in words, for a tooltip: the shape's actions (and pins), the template,
 *  the program or the wallet. */
export function stageValueText(v: StageValue): string {
  if (v.matcher !== 'ix_shape') return v.value;
  const pins = formatFeePins(v.row);
  return `${ixLabelsActions([...v.row.labels])}${pins ? ` (${pins})` : ''}`;
}

/** Identity of a value under its matcher (a shape keys with its exact pins). */
export function stageValueKey(v: StageValue): string {
  return `${v.matcher}|${v.matcher === 'ix_shape' ? patternRowKey(v.row) : v.value}`;
}

/** Every value a tag lists, keyed by {@link stageValueKey} - built once per tag, so
 *  a table's per-row "listed" read is one lookup. */
export function listedKeysOf(tag: FlowTag | null): ReadonlySet<string> {
  const out = new Set<string>();
  if (!tag) return out;
  for (const row of tag.match.ix_shape ?? []) out.add(stageValueKey({ matcher: 'ix_shape', row }));
  for (const matcher of ['ix_template', 'program', 'wallet'] as const) {
    for (const value of tag.match[matcher] ?? []) out.add(stageValueKey({ matcher, value }));
  }
  return out;
}

/** `doc` with the value added to (or taken from) tag `name` - the one write every
 *  "add to tag" click makes, through the tags document's one writer. A missing tag
 *  is created. */
export function withStageValue(doc: unknown, name: string, v: StageValue, remove: boolean): Record<string, unknown> {
  return v.matcher === 'ix_shape'
    ? withTagShape(doc, name, v.row, remove)
    : withTagListValue(doc, name, v.matcher, v.value, remove);
}

/**
 * What an "add to tag" click writes into, whoever owns the list: a fingerprint tag
 * (the engine reads it), a flow-lens set (analysis only) or a discovery draft. Every
 * trades table renders this one shape, so the strip, the badge and its tooltip all
 * name the same tag and the same matcher.
 */
export interface TagStage {
  /** The tag a click writes, without the `@`. A lens names its set here. */
  tagName: string;
  /** The matcher a click writes under. */
  matcher: StageMatcher;
  /** Matchers this stage can write; more than one ⇒ the strip offers a switch. */
  matchers: readonly StageMatcher[];
  setMatcher: (m: StageMatcher) => void;
  /** Where the tag lives - a fingerprint name, a lens set, the draft - named in
   *  every click tooltip, since a fingerprint click is an immediate save. */
  ownerName: string | null;
  /** Fee fields an `ix_shape` click copies from the tx. All off = the shape alone. */
  feePins: IxPatternFeeMask;
  setFeePins: (mask: IxPatternFeeMask) => void;
  /** Whether the value is on the list (the badge reads pressed). */
  listed: (v: StageValue) => boolean;
  /** Listed, but narrowed out by a lens: a click brings it back rather than saving. */
  muted?: (v: StageValue) => boolean;
  /** Add the value when absent, remove it when listed. `null` ⇒ read-only. */
  toggle: ((v: StageValue) => void) | null;
  saving: boolean;
  error: string | null;
}

/** A staging tape (Flow Discovery): the stage plus the draft tag the chart above
 *  previews - the tag exactly as Apply would save it. */
export interface TagTape extends TagStage {
  tag: FlowTag | null;
}

// ── Which fingerprint ────────────────────────────────────────────────────────

/** Stable empty result for the skipped match pass. */
const NO_MATCHES: Fingerprint[] = [];

/** Same-membership test on two shape sets (order inside a shape counts). */
function sameShapeSet(a: readonly (readonly string[])[], b: readonly (readonly string[])[]): boolean {
  if (a.length !== b.length) return false;
  const bKeys = new Set(b.map(patternKey));
  return a.every((p) => bKeys.has(patternKey(p)));
}

export interface IxPatternTargetChoice {
  targetId: string | null;
  inferred: boolean;
  offHost: boolean;
}

/**
 * Which fingerprint an "add to tag" click lands on, as a pure function of its inputs.
 *
 * Precedence is explicit pick > the host's own fingerprint > a key-set match. A match
 * can never outrank an id: every fingerprint without shapes carries the same empty
 * set, so reading the match first fails exactly when authoring starts - uneditable
 * (several rows match) or, worse, writing to whichever unrelated row happened to be
 * empty.
 *
 * @param matchIds fingerprints carrying the host's set; taken only when there is
 *                 exactly one, and always reported as `inferred`
 */
export function resolveIxPatternTarget(input: {
  pickedId: string | null;
  hostFingerprintId: string | null;
  matchIds: readonly string[];
}): IxPatternTargetChoice {
  const { pickedId, hostFingerprintId, matchIds } = input;
  const inferredId = pickedId == null && !hostFingerprintId && matchIds.length === 1 ? matchIds[0] : null;
  const targetId = pickedId ?? hostFingerprintId ?? inferredId;
  return {
    targetId,
    inferred: targetId != null && targetId === inferredId,
    offHost: targetId != null && hostFingerprintId != null && targetId !== hostFingerprintId,
  };
}

/** Display name of the ad-hoc tag a host's bare key set classifies with. */
export const HOST_SHAPES_TAG = 'shapes';

export interface IxPatternTarget extends TagStage {
  /** Fingerprint a click writes to; `null` ⇒ read-only. */
  target: Fingerprint | null;
  /** Every fingerprint, for the target picker. */
  fingerprints: Fingerprint[];
  targetId: string | null;
  setTargetId: (id: string | null) => void;
  /** The target's tag names, for the tag picker. */
  tagNames: string[];
  /** Pick the tag a click writes and the chart classifies with. A name the target
   *  does not define yet is a new tag: the first click creates it. */
  setTagName: (name: string) => void;
  /** The tag the chart classifies with: the target's tag as stored, or - with no
   *  target - an ad-hoc tag of the host's exact shapes. `null` ⇒ nothing classifies
   *  (a new tag before its first click). */
  tag: FlowTag | null;
  /** `tag`'s exact shapes as keys, for a host that still hands keys down. */
  keys: ReadonlySet<string> | null;
  /** Active rules bound to the target - a write changes what they all read. */
  activeRuleCount: number;
  /** The target was matched by key set, not handed down: a guess. */
  inferred: boolean;
  /** The target is NOT the host's fingerprint, so the chart now reads the picked one. */
  offHost: boolean;
}

/**
 * Which fingerprint and which tag an "add to tag" click edits, and the write itself.
 *
 * A tag lives on exactly one row - the fingerprint - and that row is what the chart
 * lines, the metric panes and the running engine all classify from. So a click
 * edits it directly, through `withTagShape` / `withTagListValue`: a staging copy
 * would be a second answer to "what carries the tag", and the surfaces reading the
 * two copies then disagree on screen while both look authoritative.
 *
 * The target is the host's OWN fingerprint whenever it knows one. Only a host with
 * none falls back to matching by its key set, reported as {@link IxPatternTarget.inferred}
 * and taken only when exactly one row carries the set.
 *
 * @param fingerprintId the host's fingerprint - the write target when known
 * @param savedKeys     the host's key set; matched against only without an id, and
 *                      classified with as an ad-hoc shape tag when nothing resolves
 * @param enabled       `false` on a read-only host, which skips the fetches
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
  const [pickedTag, setPickedTag] = useState<string | null>(null);
  const [matcher, setMatcher] = useState<StageMatcher>('ix_shape');
  const [feePins, setFeePins] = useState<IxPatternFeeMask>({});
  const [error, setError] = useState<string | null>(null);

  const savedShapes = useMemo(() => patternsFromKeys(savedKeys), [savedKeys]);

  const needsMatch = pickedId == null && !fingerprintId;
  const matches = useMemo(
    () =>
      needsMatch
        ? fingerprints.filter((f) =>
            sameShapeSet(flowTagOf(f.tags, defaultTagName(f.tags))?.match.ix_shape?.map((r) => r.labels) ?? [], savedShapes),
          )
        : NO_MATCHES,
    [needsMatch, fingerprints, savedShapes],
  );

  const { targetId, inferred, offHost } = resolveIxPatternTarget({
    pickedId,
    hostFingerprintId: fingerprintId,
    matchIds: matches.map((f) => f.id),
  });
  const target = useMemo(() => fingerprints.find((f) => f.id === targetId) ?? null, [fingerprints, targetId]);
  const names = useMemo(() => tagNames(target?.tags), [target]);
  const tagName = pickedTag ?? defaultTagName(target?.tags);

  const tag = useMemo<FlowTag | null>(() => {
    if (target) return flowTagOf(target.tags, tagName);
    return savedShapes.length > 0 ? shapeTag(HOST_SHAPES_TAG, savedShapes.map((labels) => ({ labels }))) : null;
  }, [target, tagName, savedShapes]);

  const keys = useMemo(
    () => (target ? flowPatternKeysFromTags(target.tags, tagName) : (savedKeys ?? null)),
    [target, tagName, savedKeys],
  );

  const activeRuleCount = useMemo(
    () => (targetId ? rules.filter((r) => r.fingerprint_id === targetId && r.is_active).length : 0),
    [rules, targetId],
  );

  const listedKeys = useMemo(() => listedKeysOf(target ? tag : null), [target, tag]);
  const listed = useCallback((v: StageValue) => listedKeys.has(stageValueKey(v)), [listedKeys]);

  const toggle = useCallback(
    (v: StageValue) => {
      if (!target) return;
      setError(null);
      const tags = withStageValue(target.tags, tagName, v, listed(v));
      // Fire-and-report: the mutation invalidates `Fingerprint`, so the chart, the
      // badges and the metric panes all redraw from the row just written.
      void updateFingerprint({
        id: target.id,
        body: {
          name: target.name,
          // The whole row round-trips: a PUT replaces it, so an omitted axis would
          // silently WIDEN what this fingerprint matches, and `wildcard` omitted
          // defaults to false, which the write edge rejects as criterion-less.
          criteria: target.criteria,
          wildcard: target.wildcard,
          tags,
        },
      })
        .unwrap()
        .catch((e) => setError(apiErrorMessage(e as never, `Failed to save tag ${tagName}`)));
    },
    [target, tagName, listed, updateFingerprint],
  );

  const setTargetId = useCallback((id: string | null) => {
    setPickedId(id);
    // Another row has other tags: read its default until one is picked.
    setPickedTag(null);
  }, []);

  return {
    tagName,
    matcher,
    matchers: STAGE_MATCHERS,
    setMatcher,
    ownerName: target?.name ?? null,
    feePins,
    setFeePins,
    listed,
    toggle: target ? toggle : null,
    saving,
    error,
    target,
    fingerprints,
    targetId,
    setTargetId,
    tagNames: names,
    setTagName: setPickedTag,
    tag,
    keys,
    activeRuleCount,
    inferred,
    offHost,
  };
}
