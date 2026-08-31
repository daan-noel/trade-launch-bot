import { useCallback, useMemo, useState } from 'react';

import { useLocalStorage } from 'hooks/useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';
import { apiErrorMessage } from 'store/apiSlice';
import {
  keysForSet,
  kindOf,
  patternGroups,
  patternRowsForGroups,
  tapeListForKind,
  toggleExactPattern,
  UNGROUPED,
  type IxPattern,
  type IxPatternSet,
  type IxPatternSetKind,
} from 'lib/flow/ixPatternSets';
import type { FlowSide } from 'lib/flow/classifyFlow';
import type { FlowLensValue } from 'context/FlowLensContext';
import {
  isLaunchGrain,
  templateGrain,
  toggleWorkingTemplate,
} from 'lib/strategy/templateGrain';
import type { IxPatternFee, IxPatternFeeMask } from 'lib/strategy/ixPatternRows';
import {
  useCreateIxPatternSetMutation,
  useDeleteIxPatternSetMutation,
  useGetIxPatternSetsQuery,
  useUpdateIxPatternSetMutation,
} from '@lab/store/labEndpoints';

/** Persisted lens knobs (`mt:form.traderFlowLens`). Group filter is per SET, so a
 *  set swap doesn't silently apply the previous set's narrowing. */
interface LensPrefs {
  setId: string | null;
  /** Enabled group names per set id; a set absent here means "all groups". */
  groupsBySet: Record<string, string[]>;
  contagion: boolean;
  excludeSelf: boolean;
  /** `null` ⇒ both legs. Absent in prefs written before the knob existed, which
   *  reads as both — the previous behavior. */
  side: FlowSide | null;
  feePins: IxPatternFeeMask;
}

const DEFAULT_PREFS: LensPrefs = {
  setId: null,
  groupsBySet: {},
  // Structural-only by DEFAULT, unlike the engine. A lens answers "which
  // STRUCTURES are around this moment"; forward-only wallet tagging turns that
  // into "which wallets ever matched once", which on a busy token is everyone
  // within seconds. See `FlowClassifyOptions.contagion`.
  contagion: false,
  // The studied wallet classifies itself otherwise, which is never the question.
  excludeSelf: true,
  // Both legs by default: narrowing is a deliberate act, and a lens that
  // silently showed one side would misread as "this structure is rare here".
  side: null,
  feePins: {},
};

export interface TraderFlowLens {
  /** Value for `FlowLensProvider` — classifier options + the write target. */
  value: FlowLensValue;
  /** Narrowed keys for the chart grid's `flowPatternKeys` prop; `null` ⇒ nothing
   *  to classify with, and the charts fall back to their old behavior. */
  keys: ReadonlySet<string> | null;
  sets: IxPatternSet[];
  set: IxPatternSet | null;
  setId: string | null;
  selectSet: (id: string | null) => void;
  /** Group names in the selected set, and which are currently classifying. */
  groups: string[];
  enabledGroups: ReadonlySet<string> | null;
  toggleGroup: (group: string) => void;
  contagion: boolean;
  setContagion: (on: boolean) => void;
  excludeSelf: boolean;
  setExcludeSelf: (on: boolean) => void;
  /** Which leg the split classifies; `null` ⇒ both. */
  side: FlowSide | null;
  setSide: (side: FlowSide | null) => void;
  feePins: IxPatternFeeMask;
  setFeePins: (mask: IxPatternFeeMask) => void;
  /** Replace the selected set's exact patterns (paste import). */
  savePatterns: (patterns: IxPattern[]) => Promise<void>;
  /** Replace the selected set's grain ids (paste import). */
  saveTemplates: (grains: string[]) => Promise<void>;
  createSet: (
    name: string,
    kind: IxPatternSetKind,
    patterns?: IxPattern[],
    templates?: string[],
  ) => Promise<void>;
  renameSet: (name: string) => Promise<void>;
  deleteSet: () => Promise<void>;
  saving: boolean;
  error: string | null;
}

/**
 * The Trader Analysis flow lens: which analysis-owned pattern set the page's
 * charts classify vol/non-vol with, how they classify, and the write-through a
 * Tagged-badge click performs.
 *
 * Everything persists to `ix_pattern_sets` immediately — same no-staging rule the
 * fingerprint Tagged badge follows, for the same reason: two copies of "what counts
 * as volume" on screen at once, both looking authoritative. The difference is
 * blast radius — a lens is analysis-only, so no rule changes meaning when it does.
 *
 * Kind is insert-only. Switching Exact ↔ Templates is picking a different set.
 *
 * @param wallet the studied address; excluded from classification while
 *               {@link TraderFlowLens.excludeSelf} is on
 */
export function useTraderFlowLens(wallet: string | null): TraderFlowLens {
  const [prefs, setPrefs] = useLocalStorage<LensPrefs>(
    STORAGE_KEYS.traderFlowLens,
    DEFAULT_PREFS,
  );
  const [error, setError] = useState<string | null>(null);

  const { data: sets = [] } = useGetIxPatternSetsQuery();
  const [createSetMut, { isLoading: creating }] = useCreateIxPatternSetMutation();
  const [updateSetMut, { isLoading: updating }] = useUpdateIxPatternSetMutation();
  const [deleteSetMut, { isLoading: deleting }] = useDeleteIxPatternSetMutation();
  const saving = creating || updating || deleting;

  // A stored id can outlive its set (deleted in another tab) — resolve through
  // the list rather than trusting the pref, so the lens fails to "off", never to
  // classifying with a set that no longer exists.
  const set = useMemo(
    () => sets.find((s) => s.id === prefs.setId) ?? null,
    [sets, prefs.setId],
  );
  const kind = kindOf(set);
  const patterns = set?.patterns ?? [];
  const groups = useMemo(() => patternGroups(patterns), [patterns]);

  const enabledGroups = useMemo(() => {
    if (!set || kind === 'templates') return null;
    const saved = prefs.groupsBySet[set.id];
    if (!saved) return null; // never narrowed ⇒ every group
    // Intersect with what the set actually carries now: a group can disappear
    // under an edit, and a stale name would silently narrow to nothing.
    const live = new Set(groups);
    const kept = saved.filter((g) => live.has(g));
    return kept.length === 0 ? null : new Set(kept);
  }, [set, kind, prefs.groupsBySet, groups]);

  const keys = useMemo(
    () => (set ? keysForSet(set, enabledGroups) : null),
    [set, enabledGroups],
  );

  const rows = useMemo(
    () => (kind === 'exact' ? (patternRowsForGroups(patterns, enabledGroups) ?? []) : []),
    [kind, patterns, enabledGroups],
  );

  const excludeWallets = useMemo(
    () => (prefs.excludeSelf && wallet ? new Set([wallet]) : null),
    [prefs.excludeSelf, wallet],
  );

  const writeBody = useCallback(
    (next: { patterns?: IxPattern[]; working_templates?: string[] }) => {
      if (!set) return null;
      return {
        name: set.name,
        wallet_address: set.wallet_address,
        notes: set.notes,
        kind,
        patterns: next.patterns ?? (kind === 'exact' ? set.patterns : []),
        working_templates:
          next.working_templates ?? (kind === 'templates' ? set.working_templates : []),
      };
    },
    [set, kind],
  );

  const writeSet = useCallback(
    async (next: { patterns?: IxPattern[]; working_templates?: string[] }) => {
      if (!set) return;
      const body = writeBody(next);
      if (!body) return;
      setError(null);
      try {
        await updateSetMut({ id: set.id, body }).unwrap();
      } catch (e) {
        setError(apiErrorMessage(e as never, 'Failed to save the pattern set'));
      }
    },
    [set, writeBody, updateSetMut],
  );

  // A badge click files the new pattern under the ONE enabled group when the
  // lens is narrowed to exactly one — otherwise it would land in a group that is
  // filtered out and vanish on save, which reads as a failed write.
  const activeGroup =
    enabledGroups && enabledGroups.size === 1
      ? [...enabledGroups][0] === UNGROUPED
        ? null
        : [...enabledGroups][0]
      : null;

  const toggle = useCallback(
    (labels: readonly string[], fee?: IxPatternFee) => {
      if (!set) return;
      if (kind === 'templates') {
        if (isLaunchGrain(labels)) return;
        void writeSet({
          working_templates: toggleWorkingTemplate(set.working_templates, templateGrain(labels)),
        });
        return;
      }
      void writeSet({
        patterns: toggleExactPattern(set.patterns, labels, fee, activeGroup),
      });
    },
    [set, kind, writeSet, activeGroup],
  );

  const setFeePins = useCallback(
    (mask: IxPatternFeeMask) => setPrefs((p) => ({ ...p, feePins: mask })),
    [setPrefs],
  );

  const value = useMemo<FlowLensValue>(
    () => ({
      contagion: prefs.contagion,
      excludeWallets,
      side: prefs.side ?? null,
      target: set
        ? {
            name: set.name,
            kind,
            list: tapeListForKind(kind),
            patterns: set.patterns,
            workingTemplates: set.working_templates,
            rows,
            activeGroup,
            toggle,
            feePins: prefs.feePins ?? {},
            setFeePins,
            saving,
            error,
          }
        : null,
    }),
    [
      prefs.contagion,
      prefs.side,
      prefs.feePins,
      excludeWallets,
      set,
      kind,
      rows,
      activeGroup,
      toggle,
      setFeePins,
      saving,
      error,
    ],
  );

  const selectSet = useCallback(
    (id: string | null) => setPrefs((p) => ({ ...p, setId: id })),
    [setPrefs],
  );

  const toggleGroup = useCallback(
    (group: string) => {
      if (!set) return;
      setPrefs((p) => {
        const live = patternGroups(set.patterns);
        const current = p.groupsBySet[set.id] ?? live;
        const next = current.includes(group)
          ? current.filter((g) => g !== group)
          : [...current, group];
        // Turning the last group off means "all" again rather than a blank
        // chart, which is indistinguishable from an unconfigured lens.
        const stored = next.length === 0 ? live : next;
        return { ...p, groupsBySet: { ...p.groupsBySet, [set.id]: stored } };
      });
    },
    [set, setPrefs],
  );

  const createSet = useCallback(
    async (
      name: string,
      newKind: IxPatternSetKind,
      nextPatterns: IxPattern[] = [],
      nextTemplates: string[] = [],
    ) => {
      setError(null);
      try {
        const created = await createSetMut({
          name,
          wallet_address: wallet,
          kind: newKind,
          patterns: newKind === 'exact' ? nextPatterns : [],
          working_templates: newKind === 'templates' ? nextTemplates : [],
        }).unwrap();
        setPrefs((p) => ({ ...p, setId: created.id }));
      } catch (e) {
        setError(apiErrorMessage(e as never, 'Failed to create the pattern set'));
      }
    },
    [createSetMut, wallet, setPrefs],
  );

  const renameSet = useCallback(
    async (name: string) => {
      if (!set || !name.trim()) return;
      setError(null);
      try {
        await updateSetMut({
          id: set.id,
          body: {
            name: name.trim(),
            wallet_address: set.wallet_address,
            notes: set.notes,
            kind,
            patterns: set.patterns,
            working_templates: set.working_templates,
          },
        }).unwrap();
      } catch (e) {
        setError(apiErrorMessage(e as never, 'Failed to rename the pattern set'));
      }
    },
    [set, kind, updateSetMut],
  );

  const deleteSet = useCallback(async () => {
    if (!set) return;
    setError(null);
    try {
      await deleteSetMut(set.id).unwrap();
      setPrefs((p) => {
        const { [set.id]: _dropped, ...rest } = p.groupsBySet;
        return { ...p, setId: null, groupsBySet: rest };
      });
    } catch (e) {
      setError(apiErrorMessage(e as never, 'Failed to delete the pattern set'));
    }
  }, [set, deleteSetMut, setPrefs]);

  return {
    value,
    keys,
    sets,
    set,
    setId: set?.id ?? null,
    selectSet,
    groups,
    enabledGroups,
    toggleGroup,
    contagion: prefs.contagion,
    setContagion: (on) => setPrefs((p) => ({ ...p, contagion: on })),
    excludeSelf: prefs.excludeSelf,
    setExcludeSelf: (on) => setPrefs((p) => ({ ...p, excludeSelf: on })),
    side: prefs.side ?? null,
    setSide: (side) => setPrefs((p) => ({ ...p, side })),
    feePins: prefs.feePins ?? {},
    setFeePins,
    savePatterns: (next) => writeSet({ patterns: next }),
    saveTemplates: (next) => writeSet({ working_templates: next }),
    createSet,
    renameSet,
    deleteSet,
    saving,
    error,
  };
}
