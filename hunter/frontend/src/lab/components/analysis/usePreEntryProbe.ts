import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { useLocalStorage } from 'hooks/useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';
import { narrowedSetPayload } from 'lib/flow/ixPatternSets';
import { apiErrorMessage } from 'store/apiSlice';
import { useProbePreEntryIxMutation } from '@lab/store/labEndpoints';
import {
  probeSummary,
  type PreEntryProbeRequest,
  type PreEntryVerdict,
} from '@lab/lib/preEntryProbeTypes';
import type { TraderTokenRow } from 'types';
import type { TraderFlowLens } from './useTraderFlowLens';

/** Persisted probe knobs (`mt:form.traderPreEntryProbe`). Not part of the lens
 *  prefs: the SET is what the lens owns, and these describe a question asked
 *  with it — they survive a set swap unchanged. */
interface ProbePrefs {
  on: boolean;
  /** Which verdict the table shows. `all` ⇒ the probe only adds columns. */
  show: PreEntryShow;
  /** Window `W` in SLOTS. ~400ms a slot, so 25 is ~10s of tape. */
  windowSlots: number;
  minHits: number;
  minSol: number;
}

const DEFAULT_PREFS: ProbePrefs = {
  on: false,
  // Columns first: turning the probe on must not silently remove rows from a
  // table the user is already reading. The narrowing is one click away.
  show: 'all',
  // Wide enough to hold a decision node plus the burst that follows it, short
  // enough that "before he entered" still describes a trigger.
  windowSlots: 25,
  // Presence by default; the two floors are what separate a dust print from the
  // event, and are the user's to raise.
  minHits: 1,
  minSol: 0,
};

const MAX_WINDOW_SLOTS = 2_000;
/** Knob edits settle before a probe fires — a number input dragged from 5 to 50
 *  would otherwise fan out one full-table read per keystroke. */
const DEBOUNCE_MS = 350;

/** Which rows the table keeps. Narrowing NEVER changes the summary sentence —
 *  that is computed over every probed row, so the denominator survives the
 *  filter. A filter whose own counts move with it can only show confirmations. */
export type PreEntryShow = 'all' | 'matched' | 'no-match' | 'unknown';

export interface PreEntryProbe {
  on: boolean;
  setOn: (on: boolean) => void;
  show: PreEntryShow;
  setShow: (show: PreEntryShow) => void;
  /** Row predicate for the page's filter chain. Inert while the probe is off,
   *  while `show` is `all`, and before any answer has landed. */
  passes: (mint: string) => boolean;
  windowSlots: number;
  setWindowSlots: (n: number) => void;
  minHits: number;
  setMinHits: (n: number) => void;
  minSol: number;
  setMinSol: (n: number) => void;
  /** Verdict per mint. Empty while off, loading, or unanswerable. */
  verdicts: Map<string, PreEntryVerdict>;
  /** `matched 41/120 · control 38/120 · median lag 3.0 slots` — the counts the
   *  filter has to be read against. `null` when nothing has been probed. */
  summary: string | null;
  /** Rows the backend could not reach (its own anchor ceiling). */
  skipped: number;
  loading: boolean;
  error: string | null;
  /** Why the probe cannot run right now, or `null` when it can. */
  blocked: string | null;
}

/**
 * The **pre-entry ix probe**: for every token on screen, did a structure from
 * the flow lens land on the tape before this trader entered — and how often does
 * that happen in the window before that one?
 *
 * A presence count with no denominator is not evidence: a structure a crowd
 * shares sits before everything, so the control window travels with every answer
 * and the page reports both. The verdicts are a per-mint map rather than fields
 * on the row: `TraderTokenRow` is the server's shape, and this answer is a
 * question asked ABOUT those rows, re-asked whenever the window or the narrowing
 * changes without refetching them.
 *
 * Fires only while {@link PreEntryProbe.on}: a wallet study that never opens the
 * probe costs exactly what it did before.
 */
export function usePreEntryProbe(
  wallet: string | null,
  rows: readonly TraderTokenRow[],
  lens: TraderFlowLens,
): PreEntryProbe {
  const [prefs, setPrefs] = useLocalStorage<ProbePrefs>(
    STORAGE_KEYS.traderPreEntryProbe,
    DEFAULT_PREFS,
  );
  const [probe, { isLoading }] = useProbePreEntryIxMutation();
  const [verdicts, setVerdicts] = useState<Map<string, PreEntryVerdict>>(new Map());
  const [summary, setSummary] = useState<string | null>(null);
  const [skipped, setSkipped] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const { set, enabledUnits, side } = lens;

  const payload = useMemo(
    () => (set ? narrowedSetPayload(set, enabledUnits) : null),
    [set, enabledUnits],
  );
  const classifying = payload
    ? payload.patterns.length + payload.templates.length
    : 0;

  const blocked = !wallet
    ? 'analyze a wallet first'
    : !set
      ? 'pick a pattern set in the lens'
      : classifying === 0
        ? 'the lens narrows to nothing — no unit is classifying'
        : rows.length === 0
          ? 'no rows to probe'
          : null;

  const request = useMemo<PreEntryProbeRequest | null>(() => {
    if (!wallet || !payload || blocked) return null;
    return {
      wallet,
      // Every row, including the ones with no buy leg: the backend answers those
      // `unknown`, and deciding that here would put the rule in two places.
      anchors: rows.map((r) => ({
        mint: r.mint_address,
        entry_slot: r.wallet_entry_slot,
        entry_tx_index: r.wallet_entry_tx_index,
        entry_at: r.wallet_entry_at,
      })),
      window_slots: Math.min(Math.max(1, Math.round(prefs.windowSlots)), MAX_WINDOW_SLOTS),
      min_hits: Math.max(1, Math.round(prefs.minHits)),
      min_sol: Math.max(0, prefs.minSol),
      side,
      kind: payload.kind,
      patterns: payload.patterns,
      templates: payload.templates,
    };
  }, [wallet, payload, blocked, rows, prefs.windowSlots, prefs.minHits, prefs.minSol, side]);

  // Everything the answer depends on EXCEPT the anchor list, which is derived
  // from `rows` and compared by reference (RTK hands back a stable one).
  const signature = useMemo(
    () =>
      request
        ? JSON.stringify({ ...request, anchors: request.anchors.length })
        : null,
    [request],
  );

  const latest = useRef<PreEntryProbeRequest | null>(null);
  latest.current = request;
  // Only the newest probe may write: a slow answer landing after a narrower one
  // would repaint the table with counts from a window nobody is looking at.
  const generation = useRef(0);

  useEffect(() => {
    if (!prefs.on || !signature) {
      setVerdicts(new Map());
      setSummary(null);
      setSkipped(0);
      setError(null);
      return;
    }
    const mine = ++generation.current;
    const t = setTimeout(() => {
      const body = latest.current;
      if (!body) return;
      probe(body)
        .unwrap()
        .then((res) => {
          if (generation.current !== mine) return;
          setVerdicts(new Map(res.verdicts.map((v) => [v.mint_address, v])));
          setSummary(probeSummary(res.verdicts, res.skipped));
          setSkipped(res.skipped);
          setError(null);
        })
        .catch((e) => {
          if (generation.current !== mine) return;
          setVerdicts(new Map());
          setSummary(null);
          setError(apiErrorMessage(e as never, 'Pre-entry probe failed'));
        });
    }, DEBOUNCE_MS);
    return () => clearTimeout(t);
    // `rows` rides in through `signature`'s anchor COUNT plus its own reference:
    // a refetch hands back a new array, which is exactly when the probe must
    // re-ask even though every knob is unchanged.
  }, [prefs.on, signature, rows, probe]);

  // An answer-less probe passes everything: a `matched`-only filter that emptied
  // the table on every refetch would read as the probe deleting the page.
  const passes = useCallback(
    (mint: string) => {
      if (!prefs.on || prefs.show === 'all' || verdicts.size === 0) return true;
      return verdicts.get(mint)?.state === prefs.show;
    },
    [prefs.on, prefs.show, verdicts],
  );

  const patch = useCallback(
    (p: Partial<ProbePrefs>) => setPrefs((prev) => ({ ...prev, ...p })),
    [setPrefs],
  );

  return {
    on: prefs.on,
    setOn: (on) => patch({ on }),
    show: prefs.show ?? 'all',
    setShow: (show) => patch({ show }),
    passes,
    windowSlots: prefs.windowSlots,
    setWindowSlots: (windowSlots) => patch({ windowSlots }),
    minHits: prefs.minHits,
    setMinHits: (minHits) => patch({ minHits }),
    minSol: prefs.minSol,
    setMinSol: (minSol) => patch({ minSol }),
    verdicts,
    summary,
    skipped,
    loading: isLoading,
    error,
    blocked,
  };
}
