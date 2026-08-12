import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import { patternKeysFrom } from 'lib/flow/classifyFlow';
import {
  patternsDiff,
  patternsFromKeys,
  togglePattern,
  type PatternsDiff,
  type VolumePattern,
} from 'lib/flow/volumePatterns';
import { STORAGE_KEYS, getJSON, remove, setJSON } from 'lib/storage';

/**
 * One app-wide staging area for `m_flow_split.volume_ix_patterns`.
 *
 * `volume_ix_patterns` is a CORPUS-wide fact — "what does a volume bot's tx look
 * like" — not a per-page one, so refining it belongs in one place that every
 * chart reads. That is also what makes it usable: a draft opened on a Console
 * position survives the jump to Simulate, and both charts redraw against it.
 *
 * The draft is inert until opened, so a chart's own saved patterns keep winning
 * until the user asks to edit. Nothing here writes to a fingerprint — saving is
 * a separate, explicit act (see `VolumePatternDraftBar`), because `metric_config`
 * is NOT part of fingerprint identity: a write lands on the same id and silently
 * changes what every rule bound to it means.
 */

interface StoredDraft {
  patterns: string[][];
  base: string[][];
}

export interface VolumePatternDraft {
  /** Staged patterns, or `null` when no draft is open. */
  patterns: string[][] | null;
  /** `patterns` as `flowPatternKeys`, or `null` when no draft is open. */
  keys: ReadonlySet<string> | null;
  /** The set the draft was seeded from — the baseline for {@link diff}. */
  base: string[][] | null;
  diff: PatternsDiff;
  /** Stage a draft seeded from a host's saved keys (no-op if already open). */
  open: (baseKeys?: ReadonlySet<string> | null) => void;
  /** Add/remove one ordered `ix_labels` sequence. Opens a draft if none is. */
  toggle: (labels: VolumePattern, baseKeys?: ReadonlySet<string> | null) => void;
  /** Back to the seeded set, draft still open. */
  reset: () => void;
  /** Discard the draft — every chart falls back to its own saved patterns. */
  close: () => void;
  /** Re-baseline onto what was just persisted, so the diff reads clean. */
  commit: () => void;
}

const NO_DIFF: PatternsDiff = { added: 0, removed: 0, dirty: false };

/** Inert value used when no provider is mounted (isolated component tests).
 *  Deliberately not a throw: this is cross-cutting chrome on ~15 chart hosts,
 *  and a missing provider must degrade to "no draft", never break the page. */
const INERT: VolumePatternDraft = {
  patterns: null,
  keys: null,
  base: null,
  diff: NO_DIFF,
  open: () => {},
  toggle: () => {},
  reset: () => {},
  close: () => {},
  commit: () => {},
};

const DraftContext = createContext<VolumePatternDraft>(INERT);

/** Marks a subtree whose pattern set is NOT the user's to edit here. */
const LockedContext = createContext(false);

function loadDraft(): StoredDraft | null {
  const raw = getJSON<StoredDraft | null>(STORAGE_KEYS.volumePatternDraft, null);
  if (!raw || !Array.isArray(raw.patterns) || !Array.isArray(raw.base)) return null;
  return raw;
}

export function VolumePatternDraftProvider({ children }: { children: ReactNode }) {
  const [draft, setDraft] = useState<StoredDraft | null>(loadDraft);

  useEffect(() => {
    if (draft) setJSON(STORAGE_KEYS.volumePatternDraft, draft);
    else remove(STORAGE_KEYS.volumePatternDraft);
  }, [draft]);

  const open = useCallback((baseKeys?: ReadonlySet<string> | null) => {
    setDraft((prev) => {
      if (prev) return prev;
      const base = patternsFromKeys(baseKeys);
      return { patterns: base.map((p) => [...p]), base };
    });
  }, []);

  const toggle = useCallback(
    (labels: VolumePattern, baseKeys?: ReadonlySet<string> | null) => {
      setDraft((prev) => {
        const seeded = prev ?? {
          patterns: patternsFromKeys(baseKeys),
          base: patternsFromKeys(baseKeys),
        };
        return { ...seeded, patterns: togglePattern(seeded.patterns, labels) };
      });
    },
    [],
  );

  const reset = useCallback(() => {
    setDraft((prev) => (prev ? { ...prev, patterns: prev.base.map((p) => [...p]) } : prev));
  }, []);

  const close = useCallback(() => setDraft(null), []);

  const commit = useCallback(() => {
    setDraft((prev) => (prev ? { ...prev, base: prev.patterns.map((p) => [...p]) } : prev));
  }, []);

  const value = useMemo<VolumePatternDraft>(
    () => ({
      patterns: draft?.patterns ?? null,
      keys: draft ? patternKeysFrom(draft.patterns) : null,
      base: draft?.base ?? null,
      diff: draft ? patternsDiff(draft.patterns, draft.base) : NO_DIFF,
      open,
      toggle,
      reset,
      close,
      commit,
    }),
    [draft, open, toggle, reset, close, commit],
  );

  return <DraftContext.Provider value={value}>{children}</DraftContext.Provider>;
}

/**
 * Marks its subtree read-only for pattern editing. Use it wherever the pattern
 * set is a SNAPSHOT rather than a live setting — a finished sweep run's numbers
 * were computed under its own stored `volume_ix_patterns`, so redrawing it under
 * an unrelated draft would misreport that run.
 */
export function VolumePatternScope({
  locked = false,
  children,
}: {
  locked?: boolean;
  children: ReactNode;
}) {
  return <LockedContext.Provider value={locked}>{children}</LockedContext.Provider>;
}

export function useVolumePatternDraft(): VolumePatternDraft {
  return useContext(DraftContext);
}

export function useVolumePatternLocked(): boolean {
  return useContext(LockedContext);
}

export interface EffectiveFlowPatternKeys {
  /** What this chart/table must classify against right now. */
  keys: ReadonlySet<string> | null;
  /** The draft is what `keys` came from (⇒ unsaved, and label it as such). */
  draftActive: boolean;
  /** This subtree may not edit patterns (see {@link VolumePatternScope}). */
  locked: boolean;
}

/**
 * Layer the app-wide draft over a host's own saved `flowPatternKeys`. The ONE
 * place that resolution happens — the chart's overlay and the trades table's
 * badge both go through it, so they can never classify against different sets.
 */
export function useEffectiveFlowPatternKeys(
  propKeys: ReadonlySet<string> | null | undefined,
): EffectiveFlowPatternKeys {
  const draft = useVolumePatternDraft();
  const locked = useVolumePatternLocked();
  const draftActive = draft.keys != null && !locked;
  return useMemo(
    () => ({
      keys: draftActive ? draft.keys : (propKeys ?? null),
      draftActive,
      locked,
    }),
    [draftActive, draft.keys, propKeys, locked],
  );
}
