import { useCallback, useMemo, useState } from 'react';

import { useLocalStorage } from 'hooks/useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';
import { apiErrorMessage } from 'store/apiSlice';
import {
  patternGroups,
  patternKeysForGroups,
  toggleIxPattern,
  UNGROUPED,
  type IxPattern,
  type IxPatternSet,
} from 'lib/flow/ixPatternSets';
import type { FlowSide } from 'lib/flow/classifyFlow';
import type { FlowLensValue } from 'context/FlowLensContext';
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
  /** Replace the selected set's patterns (paste import), or create a new set. */
  savePatterns: (patterns: IxPattern[]) => Promise<void>;
  createSet: (name: string, patterns: IxPattern[]) => Promise<void>;
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
  const patterns = set?.patterns ?? [];
  const groups = useMemo(() => patternGroups(patterns), [patterns]);

  const enabledGroups = useMemo(() => {
    if (!set) return null;
    const saved = prefs.groupsBySet[set.id];
    if (!saved) return null; // never narrowed ⇒ every group
    // Intersect with what the set actually carries now: a group can disappear
    // under an edit, and a stale name would silently narrow to nothing.
    const live = new Set(groups);
    const kept = saved.filter((g) => live.has(g));
    return kept.length === 0 ? null : new Set(kept);
  }, [set, prefs.groupsBySet, groups]);

  const keys = useMemo(
    () => patternKeysForGroups(patterns, enabledGroups),
    [patterns, enabledGroups],
  );

  const excludeWallets = useMemo(
    () => (prefs.excludeSelf && wallet ? new Set([wallet]) : null),
    [prefs.excludeSelf, wallet],
  );

  const writeSet = useCallback(
    async (next: IxPattern[]) => {
      if (!set) return;
      setError(null);
      try {
        await updateSetMut({
          id: set.id,
          body: {
            name: set.name,
            wallet_address: set.wallet_address,
            notes: set.notes,
            patterns: next,
          },
        }).unwrap();
      } catch (e) {
        setError(apiErrorMessage(e as never, 'Failed to save the pattern set'));
      }
    },
    [set, updateSetMut],
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
    (labels: readonly string[]) => {
      if (!set) return;
      void writeSet(toggleIxPattern(set.patterns, labels, activeGroup));
    },
    [set, writeSet, activeGroup],
  );

  const value = useMemo<FlowLensValue>(
    () => ({
      contagion: prefs.contagion,
      excludeWallets,
      side: prefs.side ?? null,
      target: set
        ? { name: set.name, patterns: set.patterns, activeGroup, toggle, saving, error }
        : null,
    }),
    [prefs.contagion, prefs.side, excludeWallets, set, activeGroup, toggle, saving, error],
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
    async (name: string, next: IxPattern[]) => {
      setError(null);
      try {
        const created = await createSetMut({
          name,
          wallet_address: wallet,
          patterns: next,
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
            patterns: set.patterns,
          },
        }).unwrap();
      } catch (e) {
        setError(apiErrorMessage(e as never, 'Failed to rename the pattern set'));
      }
    },
    [set, updateSetMut],
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
    savePatterns: writeSet,
    createSet,
    renameSet,
    deleteSet,
    saving,
    error,
  };
}
