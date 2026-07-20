import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { useDispatch } from 'react-redux';
import {
  connectFlowDiscoveryFinished,
  connectSimulationFinished,
  connectSweepFinished,
  sseSubscribe,
} from 'services/sse';
import {
  cancelFlowDiscovery,
  cancelGroupedSweep,
  cancelSimulation,
  getJobsStatus,
} from 'services/api';
import { apiSlice } from 'store/apiSlice';
import { useToast } from 'components/ui/Toast';
import type { AppDispatch } from '@lab/store';
import type {
  FlowDiscoveryNoticeEvent,
  FlowDiscoveryProgressEvent,
  SimulationProgressEvent,
  SweepGroupDoneEvent,
  SweepNoticeEvent,
  SweepProgressEvent,
} from 'types';

/**
 * App-wide registry of long-running background jobs (grouped sweep, rule
 * simulation) and their live progress.
 *
 * Why a context (not page state): a sweep/backtest runs entirely on the backend
 * inside its still-open HTTP request — navigating away or refreshing the SPA
 * never stops it, but page-local `isLoading` + the page's SSE subscription die
 * with the unmounted page, so the UI loses the running job. Mounted once at the
 * app root, this provider:
 *   • keeps ONE set of SSE subscriptions alive for the whole session, so it sees
 *     every `*_progress` / `*_finished` frame regardless of the active route;
 *   • seeds itself from `GET /api/jobs/status` on first load, recovering a job
 *     that was already running before the page (re)loaded — SSE only replays
 *     future frames;
 *   • drives a global indicator (and any page's controls) from that single
 *     source of truth, so progress survives navigation.
 *
 * Keys: the sweep is single-flight → one fixed `'sweep'` slot. Simulations are
 * per-rule → keyed by `rule_id`. The map is only touched on sweep/sim SSE frames
 * (never on the SOL/USD or live-trade streams), so consumers don't re-render on
 * the high-frequency ticks.
 *
 * Split into two contexts so a progress tick re-renders only what reads the
 * live job *state*: the **actions** context ({@link useBackgroundJobActions})
 * exposes the stable `markStarting`/`cancel` callbacks (identity never changes →
 * the heavy strategy pages that only *start* jobs never re-render on a tick);
 * the **state** context ({@link useBackgroundJobsState}) carries the changing
 * `jobs`/`isRunning` and is consumed by the global indicator (and the sweep
 * page's run-state check) alone.
 */
export type JobKind = 'sweep' | 'simulation' | 'discovery';

/** Progress state for one named phase of a sweep (corpus / coarse / sweep). */
export interface PhaseProgress {
  label: string;
  processed: number | null;
  total: number | null;
  /** True once the phase has finished (a later phase arrived or it hit 100%). */
  done: boolean;
}

// Phases are strictly sequential on the wire (corpus → coarse → sweep) — that
// invariant is what lets `upsert` mark the previous phase done when a new one
// arrives. Persistence ("saving") is deliberately NOT a phase: it drains
// concurrently with the sweep fold, so its old phase frames interleaved with
// sweep frames and made both bars flip-flop between active/done. It now reports
// through `sweep_group_done` (the `groupsDone`/`groupCount` counter) instead.
const PHASE_LABELS: Record<string, string> = {
  corpus: 'Loading corpus',
  coarse: 'Coarse sweep',
  sweep: 'Sweep',
  score: 'Scoring structures',
};

/** Singleton key for the single-flight flow-discovery job. */
const DISCOVERY_KEY = 'discovery';

export interface BackgroundJob {
  kind: JobKind;
  /** Map key + cancel target: the fixed `'sweep'` slot, or a simulation's rule_id. */
  id: string;
  /** Human label for the indicator (rule name when known, else a generic name). */
  label: string;
  /** Null until the first progress frame declares the total (indeterminate bar).
   *  Kept for status-recovery backwards compat; phases map is the live source. */
  processed: number | null;
  total: number | null;
  /** A cancel has been requested but the cooperative abort hasn't landed yet. */
  cancelling: boolean;
  /** Epoch-ms of our first observation of this job (markStarting / seed / first
   *  frame). Baseline `t0` for the average-throughput ETA. */
  firstSeenAt: number;
  /** `processed` at {@link firstSeenAt} (0 for a fresh start, the seeded count
   *  for a job recovered mid-run from `/api/jobs/status`). Baseline so the rate
   *  measures work done *since we started watching*, keeping ETA honest for
   *  recovered jobs. */
  firstSeenProcessed: number;
  /** Per-phase progress, keyed by phase string ("corpus" / "coarse" / "sweep").
   *  Empty until the first phase-tagged frame arrives (falls back to single bar). */
  phases: Map<string, PhaseProgress>;
  /** The phase key that most recently received a progress frame. */
  activePhase: string | null;
  /** Groups committed to the DB so far (sweep jobs; from `sweep_group_done`).
   *  Null until the writer's announce frame. Groups are readable mid-run. */
  groupsDone: number | null;
  /** Total surviving groups this run will persist (sweep jobs). */
  groupCount: number | null;
  /** The run id the counters (and live group refetches) belong to. */
  runId: string | null;
}

/** Singleton key for the single-flight grouped sweep. */
const SWEEP_KEY = 'sweep';

/** Stable job actions — the value identity never changes across progress ticks,
 *  so consumers that only fire jobs (the strategy pages) don't re-render. */
interface BackgroundJobActions {
  /** Optimistically register a job the moment its request is fired, so the
   *  indicator shows before the first SSE frame (which only arrives once the
   *  backend finishes selecting/partitioning candidates). */
  markStarting: (kind: JobKind, id: string, label: string) => void;
  /** Clear an optimistically-registered job when the page's own request settles.
   *  The `*_finished` SSE event only fires when the backend actually ran the job,
   *  so a cache hit / dedupe / cancelled marker / missed frame would otherwise
   *  leave a phantom job whose progress bar spins forever. Idempotent with the
   *  SSE-driven removal. */
  markFinished: (kind: JobKind, id: string) => void;
  cancel: (job: BackgroundJob) => void;
  /** The active strategy page publishes the rule IDs it renders per-row here (and
   *  clears them on unmount). The global indicator hides exactly those simulation
   *  jobs — they already show as inline row bars — while still surfacing sweeps,
   *  swings, and any simulation not on the current page. */
  setCoveredSimIds: (ids: string[]) => void;
}

/** Live job state — changes on every `*_progress` frame; consumed only by the
 *  global indicator and the sweep page's run-state check. */
interface BackgroundJobsState {
  jobs: BackgroundJob[];
  /** Whether a job of this kind/id is currently tracked (running). */
  isRunning: (kind: JobKind, id: string) => boolean;
  /** Rule IDs the active strategy page shows a per-row sim bar for — the global
   *  indicator suppresses these simulation jobs (see {@link setCoveredSimIds}). */
  coveredSimIds: ReadonlySet<string>;
}

const BackgroundJobActionsContext = createContext<BackgroundJobActions | null>(null);
const BackgroundJobsStateContext = createContext<BackgroundJobsState | null>(null);

const keyOf = (kind: JobKind, id: string) => (kind === 'sweep' ? SWEEP_KEY : id);

export function BackgroundJobsProvider({ children }: { children: ReactNode }) {
  const dispatch = useDispatch<AppDispatch>();
  const { addToast } = useToast();
  const [jobs, setJobs] = useState<Map<string, BackgroundJob>>(new Map());
  // Rule IDs whose sim progress the mounted strategy page already shows per-row,
  // so the global indicator can hide those (and only those) simulation jobs.
  const [coveredSimIds, setCoveredSimIdsState] = useState<ReadonlySet<string>>(() => new Set());

  /** Insert or update one job, preserving fields the caller didn't supply.
   *  Pass `phase` when the update carries per-phase progress so the phases map
   *  is updated and the previous active phase is marked done. */
  const upsert = useCallback(
    (kind: JobKind, id: string, patch: Partial<BackgroundJob>, phase?: string) => {
      setJobs((prev) => {
        const key = keyOf(kind, id);
        const existing = prev.get(key);
        const processed =
          patch.processed !== undefined ? patch.processed : existing?.processed ?? null;

        // Build the updated phases map when a phase-tagged frame arrives.
        let newPhases: Map<string, PhaseProgress>;
        let newActivePhase: string | null;
        if (phase) {
          newPhases = new Map(existing?.phases ?? []);
          // Mark the previously active phase done when a different phase arrives.
          const prevActive = existing?.activePhase ?? null;
          if (prevActive && prevActive !== phase && newPhases.has(prevActive)) {
            const prev_ph = newPhases.get(prevActive)!;
            newPhases.set(prevActive, { ...prev_ph, done: true });
          }
          const existingPh = newPhases.get(phase);
          newPhases.set(phase, {
            label: PHASE_LABELS[phase] ?? phase,
            processed: patch.processed !== undefined ? patch.processed : existingPh?.processed ?? null,
            total: patch.total !== undefined ? patch.total : existingPh?.total ?? null,
            done: false,
          });
          newActivePhase = phase;
        } else {
          newPhases = existing?.phases ?? new Map();
          newActivePhase = existing?.activePhase ?? null;
        }

        const next = new Map(prev);
        next.set(key, {
          kind,
          id,
          label:
            patch.label ?? existing?.label ?? (kind === 'sweep' ? 'Grouped sweep' : 'Simulation'),
          processed,
          total: patch.total !== undefined ? patch.total : existing?.total ?? null,
          cancelling: patch.cancelling ?? existing?.cancelling ?? false,
          // Stamp the ETA baseline once, on first observation; afterwards carry it
          // forward so the average rate measures from when we started watching.
          firstSeenAt: existing?.firstSeenAt ?? Date.now(),
          firstSeenProcessed: existing?.firstSeenProcessed ?? processed ?? 0,
          phases: newPhases,
          activePhase: newActivePhase,
          groupsDone: patch.groupsDone !== undefined ? patch.groupsDone : existing?.groupsDone ?? null,
          groupCount: patch.groupCount !== undefined ? patch.groupCount : existing?.groupCount ?? null,
          runId: patch.runId !== undefined ? patch.runId : existing?.runId ?? null,
        });
        return next;
      });
    },
    [],
  );

  const remove = useCallback((kind: JobKind, id: string) => {
    setJobs((prev) => {
      const key = keyOf(kind, id);
      if (!prev.has(key)) return prev;
      const next = new Map(prev);
      next.delete(key);
      return next;
    });
  }, []);

  // Throttled per-run groups refetch driven by `sweep_group_done`: fire at most
  // one invalidation per window, with a trailing call so the last group of a
  // burst always lands. Single-flight sweep ⇒ one runId at a time, so plain refs
  // suffice.
  const groupsInvalidateTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const groupsInvalidateLastAt = useRef(0);
  const invalidateRunGroups = useCallback(
    (runId: string) => {
      const WINDOW_MS = 2000;
      const fire = () => {
        groupsInvalidateLastAt.current = Date.now();
        dispatch(apiSlice.util.invalidateTags([{ type: 'GroupedSweepGroups', id: runId }]));
      };
      const since = Date.now() - groupsInvalidateLastAt.current;
      if (since >= WINDOW_MS) {
        fire();
      } else if (!groupsInvalidateTimer.current) {
        groupsInvalidateTimer.current = setTimeout(() => {
          groupsInvalidateTimer.current = null;
          fire();
        }, WINDOW_MS - since);
      }
    },
    [dispatch],
  );

  // One set of subscriptions for the whole session; seed from the status snapshot
  // so a job already running at load time is recovered. The progress streams are
  // subscribed raw (no id filter) so the registry sees every job's frames.
  useEffect(() => {
    let alive = true;
    getJobsStatus()
      .then((status) => {
        if (!alive) return;
        if (status.sweep) {
          upsert('sweep', SWEEP_KEY, {
            processed: status.sweep.processed,
            total: status.sweep.total,
          });
        }
        if (status.discovery) {
          upsert('discovery', DISCOVERY_KEY, {
            label: 'Flow discovery',
            processed: status.discovery.processed,
            total: status.discovery.total,
          });
        }
        for (const s of status.simulations) {
          upsert('simulation', s.rule_id, { processed: s.processed, total: s.total });
        }
      })
      .catch(() => {
        /* no jobs / backend down — nothing to recover */
      });

    const offSweepProgress = sseSubscribe('sweep_progress', (e) => {
      if (typeof e.data !== 'string') return;
      try {
        const p = JSON.parse(e.data) as SweepProgressEvent;
        // `total: 0` = a declared-but-unsized phase (the corpus lake load) —
        // map to nulls so the bar renders indeterminate instead of a full 0/0.
        const sized = p.total > 0;
        upsert(
          'sweep',
          SWEEP_KEY,
          { processed: sized ? p.processed : null, total: sized ? p.total : null },
          p.phase,
        );
      } catch {
        /* ignore malformed frames */
      }
    });
    const offSweepGroupDone = sseSubscribe('sweep_group_done', (e) => {
      if (typeof e.data !== 'string') return;
      try {
        const g = JSON.parse(e.data) as SweepGroupDoneEvent;
        upsert('sweep', SWEEP_KEY, {
          groupsDone: g.groups_done,
          groupCount: g.group_count,
          runId: g.run_id,
        });
        // The group's rows are committed and readable NOW — refetch the run's
        // groups list so results stream into the table mid-run. Throttled: tiny
        // groups can persist many times per second, and each invalidation
        // refetches every subscribed groups query.
        if (g.group_index != null) invalidateRunGroups(g.run_id);
      } catch {
        /* ignore malformed frames */
      }
    });
    // Non-terminal advisory: the engine degraded its sizing to fit free RAM. The
    // sweep is still running (and still correct) — this only explains why it is
    // slower, so it is an info toast, not a danger one.
    const offSweepNotice = sseSubscribe('sweep_notice', (e) => {
      if (typeof e.data !== 'string') return;
      try {
        const n = JSON.parse(e.data) as SweepNoticeEvent;
        addToast('Sweep running slower', n.message, 'info');
      } catch {
        /* ignore malformed frames */
      }
    });
    const offSimProgress = sseSubscribe('simulation_progress', (e) => {
      if (typeof e.data !== 'string') return;
      try {
        const p = JSON.parse(e.data) as SimulationProgressEvent;
        upsert('simulation', p.rule_id, { processed: p.processed, total: p.total });
      } catch {
        /* ignore malformed frames */
      }
    });
    const offDiscoveryProgress = sseSubscribe('flow_discovery_progress', (e) => {
      if (typeof e.data !== 'string') return;
      try {
        const p = JSON.parse(e.data) as FlowDiscoveryProgressEvent;
        const sized = p.total > 0;
        upsert(
          'discovery',
          DISCOVERY_KEY,
          {
            label: 'Flow discovery',
            processed: sized ? p.processed : null,
            total: sized ? p.total : null,
            runId: p.run_id,
          },
          p.phase,
        );
      } catch {
        /* ignore malformed frames */
      }
    });
    const offDiscoveryNotice = sseSubscribe('flow_discovery_notice', (e) => {
      if (typeof e.data !== 'string') return;
      try {
        const n = JSON.parse(e.data) as FlowDiscoveryNoticeEvent;
        addToast('Flow discovery', n.message, 'info');
      } catch {
        /* ignore malformed frames */
      }
    });
    const sweepFinished = connectSweepFinished((ev) => {
      remove('sweep', SWEEP_KEY);
      // A finished sweep persisted a new run — refresh the runs list app-wide.
      // `GroupedSweepGroups` queries also provide 'GroupedSweep', so the final
      // groups state refetches here too.
      dispatch(apiSlice.util.invalidateTags(['GroupedSweep']));
      // `error` reaches the client only here: an engine failure or a short
      // write. The run row is always kept (`partial` when groups committed,
      // `failed` when none did), so this explains a run that is still open and
      // inspectable rather than one that silently thins.
      if (ev.error) addToast('Sweep problem', ev.error, 'danger');
    });
    const simFinished = connectSimulationFinished((ev) => remove('simulation', ev.rule_id));
    const discoveryFinished = connectFlowDiscoveryFinished((ev) => {
      remove('discovery', DISCOVERY_KEY);
      if (ev.error) addToast('Flow discovery problem', ev.error, 'danger');
    });

    return () => {
      alive = false;
      offSweepProgress();
      offSweepGroupDone();
      offSweepNotice();
      offSimProgress();
      offDiscoveryProgress();
      offDiscoveryNotice();
      sweepFinished.close();
      simFinished.close();
      discoveryFinished.close();
      if (groupsInvalidateTimer.current) {
        clearTimeout(groupsInvalidateTimer.current);
        groupsInvalidateTimer.current = null;
      }
    };
  }, [upsert, remove, dispatch, addToast, invalidateRunGroups]);

  const markStarting = useCallback(
    (kind: JobKind, id: string, label: string) => {
      upsert(kind, id, {
        label,
        processed: null,
        total: null,
        cancelling: false,
        phases: new Map(),
        activePhase: null,
        groupsDone: null,
        groupCount: null,
        runId: null,
      });
    },
    [upsert],
  );

  const markFinished = useCallback((kind: JobKind, id: string) => remove(kind, id), [remove]);

  // Replace the covered set, no-op'ing when the membership is unchanged so the
  // per-poll `rules` array churn never re-renders the indicator or state consumers.
  const setCoveredSimIds = useCallback((ids: string[]) => {
    setCoveredSimIdsState((prev) => {
      if (prev.size === ids.length && ids.every((id) => prev.has(id))) return prev;
      return new Set(ids);
    });
  }, []);

  const isRunning = useCallback((kind: JobKind, id: string) => jobs.has(keyOf(kind, id)), [jobs]);

  const cancel = useCallback(
    (job: BackgroundJob) => {
      upsert(job.kind, job.id, { cancelling: true });
      const req =
        job.kind === 'sweep'
          ? cancelGroupedSweep()
          : job.kind === 'discovery'
            ? cancelFlowDiscovery()
            : cancelSimulation(job.id);
      req.catch(() => upsert(job.kind, job.id, { cancelling: false }));
    },
    [upsert],
  );

  // `markStarting`/`cancel` depend only on the stable `upsert`, so this value is
  // referentially stable — action-only consumers skip the progress-tick renders.
  const actions = useMemo<BackgroundJobActions>(
    () => ({ markStarting, markFinished, cancel, setCoveredSimIds }),
    [markStarting, markFinished, cancel, setCoveredSimIds],
  );

  const state = useMemo<BackgroundJobsState>(
    () => ({ jobs: Array.from(jobs.values()), isRunning, coveredSimIds }),
    [jobs, isRunning, coveredSimIds],
  );

  return (
    <BackgroundJobActionsContext.Provider value={actions}>
      <BackgroundJobsStateContext.Provider value={state}>
        {children}
      </BackgroundJobsStateContext.Provider>
    </BackgroundJobActionsContext.Provider>
  );
}

/** Stable job actions (`markStarting`, `cancel`) — does not re-render on ticks. */
export function useBackgroundJobActions() {
  const ctx = useContext(BackgroundJobActionsContext);
  if (!ctx) throw new Error('useBackgroundJobActions must be used within BackgroundJobsProvider');
  return ctx;
}

/** Live job state (`jobs`, `isRunning`) — re-renders on each progress frame. */
export function useBackgroundJobsState() {
  const ctx = useContext(BackgroundJobsStateContext);
  if (!ctx) throw new Error('useBackgroundJobsState must be used within BackgroundJobsProvider');
  return ctx;
}
