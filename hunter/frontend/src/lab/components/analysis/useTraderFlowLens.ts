import { useCallback, useMemo, useState } from 'react';

import { useLocalStorage } from 'hooks/useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';
import { apiErrorMessage } from 'store/apiSlice';
import {
  keysForSet,
  kindOf,
  mutedPatternForClick,
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

/** Persisted lens knobs (`mt:form.traderFlowLens`). Narrowing is per SET, so a
 *  set swap doesn't silently apply the previous set's narrowing. */
interface LensPrefs {
  setId: string | null;
  /** Enabled group names per EXACT set id; a set absent here means "all groups".
   *  A group exists only while some pattern carries its name, so an edit can
   *  retire one — storing what is ON is the shape that survives that. */
  groupsBySet: Record<string, string[]>;
  /** Muted grain ids per TEMPLATES set id; absent or empty ⇒ every grain
   *  classifies. Opposite polarity to {@link groupsBySet} on purpose: a grain is
   *  an entry the user adds by hand, so a freshly pasted or badge-clicked one has
   *  to classify at once — under an enabled-list it would land outside the filter
   *  and read as a failed write. */
  mutedBySet: Record<string, string[]>;
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
  mutedBySet: {},
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
  /** The selected set's narrowing axis: group names on an exact set, grain ids
   *  on a templates set. {@link enabledUnits} is which of them classify right now
   *  (`null` ⇒ all of them); {@link toggleUnit} flips one. View state only —
   *  narrowing never edits the stored set. */
  units: string[];
  enabledUnits: ReadonlySet<string> | null;
  toggleUnit: (unit: string) => void;
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
  const templates = useMemo(() => set?.working_templates ?? [], [set]);
  const units = useMemo(
    () => (kind === 'templates' ? templates : patternGroups(patterns)),
    [kind, templates, patterns],
  );

  const enabledUnits = useMemo(() => {
    if (!set) return null;
    if (kind === 'templates') {
      const muted = new Set(prefs.mutedBySet?.[set.id] ?? []);
      if (muted.size === 0) return null; // nothing muted ⇒ every grain
      // Muting every grain narrows to nothing, and that stands: the chips all
      // read off and the bar says "0/N classifying", so it can't be mistaken for
      // an unconfigured lens the way a silently-emptied group filter could.
      return new Set(units.filter((g) => !muted.has(g)));
    }
    const saved = prefs.groupsBySet[set.id];
    if (!saved) return null; // never narrowed ⇒ every group
    // Intersect with what the set actually carries now: a group can disappear
    // under an edit, and a stale name would silently narrow to nothing.
    const live = new Set(units);
    const kept = saved.filter((g) => live.has(g));
    return kept.length === 0 ? null : new Set(kept);
  }, [set, kind, prefs.groupsBySet, prefs.mutedBySet, units]);

  const keys = useMemo(
    () => (set ? keysForSet(set, enabledUnits) : null),
    [set, enabledUnits],
  );

  const rows = useMemo(
    () => (kind === 'exact' ? (patternRowsForGroups(patterns, enabledUnits) ?? []) : []),
    [kind, patterns, enabledUnits],
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

  const mutedGrains = useMemo(
    () => (set && kind === 'templates' ? (prefs.mutedBySet?.[set.id] ?? []) : []),
    [set, kind, prefs.mutedBySet],
  );

  /** Mute / un-mute ONE grain. Ids the set no longer carries are dropped on every
   *  write, so a removed-then-re-added grain comes back classifying instead of
   *  staying silently muted. */
  const setGrainMuted = useCallback(
    (grain: string, muted: boolean) => {
      if (!set) return;
      setPrefs((p) => {
        const live = new Set(set.working_templates);
        const kept = (p.mutedBySet?.[set.id] ?? []).filter((g) => g !== grain && live.has(g));
        return {
          ...p,
          mutedBySet: { ...(p.mutedBySet ?? {}), [set.id]: muted ? [...kept, grain] : kept },
        };
      });
    },
    [set, setPrefs],
  );

  const toggleUnit = useCallback(
    (unit: string) => {
      if (!set) return;
      if (kindOf(set) === 'templates') {
        setGrainMuted(unit, !(prefs.mutedBySet?.[set.id] ?? []).includes(unit));
        return;
      }
      setPrefs((p) => {
        const live = patternGroups(set.patterns);
        const current = p.groupsBySet[set.id] ?? live;
        const next = current.includes(unit)
          ? current.filter((g) => g !== unit)
          : [...current, unit];
        // Turning the last group off means "all" again rather than a blank
        // chart, which is indistinguishable from an unconfigured lens.
        const stored = next.length === 0 ? live : next;
        return { ...p, groupsBySet: { ...p.groupsBySet, [set.id]: stored } };
      });
    },
    [set, prefs.mutedBySet, setGrainMuted, setPrefs],
  );

  // A badge click files the new pattern under the ONE enabled group when the
  // lens is narrowed to exactly one — otherwise it would land in a group that is
  // filtered out and vanish on save, which reads as a failed write.
  const activeGroup =
    kind === 'exact' && enabledUnits && enabledUnits.size === 1
      ? [...enabledUnits][0] === UNGROUPED
        ? null
        : [...enabledUnits][0]
      : null;

  /**
   * A tape badge reports what the CHART classified with — the NARROWED key set —
   * so a click has to flip exactly that, or the badge stays put and the click
   * reads as broken.
   *
   * Either vocabulary can be off for two different reasons, and they take
   * opposite writes: not in the set at all (add it), or in the set but narrowed
   * out by the chips (bring that unit back). One write for both left the badge
   * unchanged and quietly dropped a row the reader could not even see.
   *
   * The exact side un-mutes the whole GROUP the stored pattern sits in, since
   * that is the unit there — and only when the muted row provably accepts this
   * click (unpinned, or its pins equal the pins the click carries). Unprovable
   * ⇒ fall through to the normal write rather than guess.
   */
  const toggle = useCallback(
    (labels: readonly string[], fee?: IxPatternFee) => {
      if (!set) return;
      if (kind === 'templates') {
        if (isLaunchGrain(labels)) return;
        const grain = templateGrain(labels);
        const muted = mutedGrains.includes(grain);
        if (muted && set.working_templates.includes(grain)) {
          setGrainMuted(grain, false);
          return;
        }
        // A mute left over from an earlier removal would swallow the add.
        if (muted) setGrainMuted(grain, false);
        void writeSet({
          working_templates: toggleWorkingTemplate(set.working_templates, grain),
        });
        return;
      }
      const hidden = mutedPatternForClick(set.patterns, enabledUnits, labels, fee);
      if (hidden) {
        toggleUnit(hidden.group ?? UNGROUPED);
        return;
      }
      void writeSet({
        patterns: toggleExactPattern(set.patterns, labels, fee, activeGroup),
      });
    },
    [set, kind, writeSet, activeGroup, mutedGrains, setGrainMuted, enabledUnits, toggleUnit],
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
        const { [set.id]: _muted, ...restMuted } = p.mutedBySet ?? {};
        return { ...p, setId: null, groupsBySet: rest, mutedBySet: restMuted };
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
    units,
    enabledUnits,
    toggleUnit,
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
