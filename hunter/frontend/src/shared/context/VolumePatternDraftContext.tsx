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

/**
 * How a subtree treats the app-wide draft. Three states, because "may I edit here"
 * and "does the draft change what I am reading" are different questions and
 * collapsing them into one boolean loses one of them:
 *
 * - `free` — the draft applies ambiently. Discovery / simulate / plain token charts,
 *   where the whole point is to see the staged set redraw everything.
 * - `decision` — this view REPORTS A DECISION already taken (a position and its
 *   engine-stamped exit). Editing stays available, but the draft is applied only
 *   when the reader asks for it, and is marked while it is. At rest the view shows
 *   the classification the decision was actually made under.
 * - `locked` — a stored run's own frozen `volume_ix_patterns`. Not the user's to
 *   edit here and never redrawn under anything else.
 */
export type VolumePatternScopeMode = 'free' | 'decision' | 'locked';

interface ScopeState {
  mode: VolumePatternScopeMode;
  /** `decision` only: the reader has asked to see the draft applied. */
  preview: boolean;
  setPreview: (on: boolean) => void;
}

const ScopeContext = createContext<ScopeState>({
  mode: 'free',
  preview: false,
  setPreview: () => {},
});

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
 * Declares how its subtree treats the draft (see {@link VolumePatternScopeMode}).
 *
 * `locked` is kept as a shorthand for `mode="locked"` — a finished sweep run's
 * numbers were computed under its own stored `volume_ix_patterns`, so redrawing it
 * under an unrelated draft would misreport that run.
 *
 * `preview` is per-scope state, not global: two modals open on different positions
 * each answer for themselves, and it resets to off on mount so a view always opens
 * showing what was decided.
 */
export function VolumePatternScope({
  mode,
  locked = false,
  children,
}: {
  mode?: VolumePatternScopeMode;
  locked?: boolean;
  children: ReactNode;
}) {
  const resolved: VolumePatternScopeMode = mode ?? (locked ? 'locked' : 'free');
  const [preview, setPreview] = useState(false);
  const value = useMemo<ScopeState>(
    () => ({ mode: resolved, preview: resolved === 'decision' && preview, setPreview }),
    [resolved, preview],
  );
  return <ScopeContext.Provider value={value}>{children}</ScopeContext.Provider>;
}

/** The subtree's scope state — mode plus the `decision` preview switch. */
export function useVolumePatternScope(): ScopeState {
  return useContext(ScopeContext);
}

export function useVolumePatternDraft(): VolumePatternDraft {
  return useContext(DraftContext);
}

/** True where patterns are not this subtree's to edit (`locked` only). */
export function useVolumePatternLocked(): boolean {
  return useContext(ScopeContext).mode === 'locked';
}

export interface EffectiveFlowPatternKeys {
  /** What this chart/table must classify against right now. */
  keys: ReadonlySet<string> | null;
  /** The draft is what `keys` came from (⇒ unsaved, and label it as such). */
  draftActive: boolean;
  /** This subtree may not edit patterns (see {@link VolumePatternScope}). */
  locked: boolean;
  /** The scope this resolution happened in. */
  mode: VolumePatternScopeMode;
  /** A draft is open and this scope can show it on request — render the switch. */
  canPreviewDraft: boolean;
  /**
   * A draft IS open but this subtree is locked, so the saved set is winning.
   *
   * Must be surfaced wherever it is true. The failure it exists to prevent is
   * silent and expensive: the reader sums the vol / non-vol split off a locked
   * chart, gets a number that disagrees with an engine decision, and concludes the
   * engine is wrong — when in fact their own unsaved draft is what disagrees.
   */
  draftIgnored: boolean;
}

/**
 * Layer the app-wide draft over a host's own saved `flowPatternKeys`. The ONE
 * place that resolution happens — the chart's overlay and the trades table's
 * badge both go through it, so they can never classify against different sets.
 */
/**
 * Which set a subtree classifies with, as a pure function of its inputs — the whole
 * scope decision in one testable place, since the hook around it needs a React tree
 * and this repo's tests run on `node`.
 *
 * `draftIgnored` is not bookkeeping: a staged-but-not-applied draft MUST be
 * surfaced, or a reader hand-sums a split that disagrees with the decision beside it
 * and concludes the engine is wrong.
 */
export function resolveFlowPatternKeys(input: {
  mode: VolumePatternScopeMode;
  preview: boolean;
  draftKeys: ReadonlySet<string> | null;
  propKeys: ReadonlySet<string> | null | undefined;
}): EffectiveFlowPatternKeys {
  const { mode, preview, draftKeys, propKeys } = input;
  const open = draftKeys != null;
  // `decision` applies the draft only on request — a view that reports a decision
  // must open showing the classification that decision was made under.
  const draftActive = open && (mode === 'free' || (mode === 'decision' && preview));
  return {
    keys: draftActive ? draftKeys : (propKeys ?? null),
    draftActive,
    locked: mode === 'locked',
    draftIgnored: open && !draftActive,
    mode,
    canPreviewDraft: mode === 'decision' && open,
  };
}

export function useEffectiveFlowPatternKeys(
  propKeys: ReadonlySet<string> | null | undefined,
): EffectiveFlowPatternKeys {
  const draft = useVolumePatternDraft();
  const { mode, preview } = useVolumePatternScope();
  const draftKeys = draft.keys;
  return useMemo(
    () => resolveFlowPatternKeys({ mode, preview, draftKeys, propKeys }),
    [mode, preview, draftKeys, propKeys],
  );
}
