import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import { useDispatch } from 'react-redux';
import {
  connectSimulationFinished,
  connectSweepFinished,
  connectSwingDetectionFinished,
  sseSubscribe,
} from 'services/sse';
import {
  cancelGroupedSweep,
  cancelSimulation,
  cancelSwingRun,
  getJobsStatus,
} from 'services/api';
import { apiSlice } from 'store/apiSlice';
import type { AppDispatch } from '../store';
import type { SimulationProgressEvent, SweepProgressEvent } from 'types';

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
export type JobKind = 'sweep' | 'simulation' | 'swing';

/** Progress state for one named phase of a sweep (coarse / sweep / saving). */
export interface PhaseProgress {
  label: string;
  processed: number | null;
  total: number | null;
  /** True once the phase has finished (a later phase arrived or it hit 100%). */
  done: boolean;
}

const PHASE_LABELS: Record<string, string> = {
  coarse: 'Coarse sweep',
  sweep: 'Sweep',
  saving: 'Saving results',
};

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
  /** Per-phase progress, keyed by phase string ("coarse" / "sweep" / "saving").
   *  Empty until the first phase-tagged frame arrives (falls back to single bar). */
  phases: Map<string, PhaseProgress>;
  /** The phase key that most recently received a progress frame. */
  activePhase: string | null;
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
}

/** Live job state — changes on every `*_progress` frame; consumed only by the
 *  global indicator and the sweep page's run-state check. */
interface BackgroundJobsState {
  jobs: BackgroundJob[];
  /** Whether a job of this kind/id is currently tracked (running). */
  isRunning: (kind: JobKind, id: string) => boolean;
}

const BackgroundJobActionsContext = createContext<BackgroundJobActions | null>(null);
const BackgroundJobsStateContext = createContext<BackgroundJobsState | null>(null);

const keyOf = (kind: JobKind, id: string) => (kind === 'sweep' ? SWEEP_KEY : id);

export function BackgroundJobsProvider({ children }: { children: ReactNode }) {
  const dispatch = useDispatch<AppDispatch>();
  const [jobs, setJobs] = useState<Map<string, BackgroundJob>>(new Map());

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
        for (const s of status.simulations) {
          upsert('simulation', s.rule_id, { processed: s.processed, total: s.total });
        }
        for (const s of status.swings) {
          upsert('swing', s.run_id, { processed: s.processed, total: s.total });
        }
      })
      .catch(() => {
        /* no jobs / backend down — nothing to recover */
      });

    const offSweepProgress = sseSubscribe('sweep_progress', (e) => {
      if (typeof e.data !== 'string') return;
      try {
        const p = JSON.parse(e.data) as SweepProgressEvent;
        upsert('sweep', SWEEP_KEY, { processed: p.processed, total: p.total }, p.phase);
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
    const sweepFinished = connectSweepFinished(() => {
      remove('sweep', SWEEP_KEY);
      // A finished sweep persisted a new run — refresh the runs list app-wide.
      dispatch(apiSlice.util.invalidateTags(['GroupedSweep']));
    });
    const simFinished = connectSimulationFinished((ev) => remove('simulation', ev.rule_id));
    const swingFinished = connectSwingDetectionFinished((ev) => remove('swing', ev.run_id));

    return () => {
      alive = false;
      offSweepProgress();
      offSimProgress();
      sweepFinished.close();
      simFinished.close();
      swingFinished.close();
    };
  }, [upsert, remove, dispatch]);

  const markStarting = useCallback(
    (kind: JobKind, id: string, label: string) => {
      upsert(kind, id, { label, processed: null, total: null, cancelling: false, phases: new Map(), activePhase: null });
    },
    [upsert],
  );

  const markFinished = useCallback((kind: JobKind, id: string) => remove(kind, id), [remove]);

  const isRunning = useCallback((kind: JobKind, id: string) => jobs.has(keyOf(kind, id)), [jobs]);

  const cancel = useCallback(
    (job: BackgroundJob) => {
      upsert(job.kind, job.id, { cancelling: true });
      const req =
        job.kind === 'sweep'
          ? cancelGroupedSweep()
          : job.kind === 'swing'
            ? cancelSwingRun(job.id)
            : cancelSimulation(job.id);
      req.catch(() => upsert(job.kind, job.id, { cancelling: false }));
    },
    [upsert],
  );

  // `markStarting`/`cancel` depend only on the stable `upsert`, so this value is
  // referentially stable — action-only consumers skip the progress-tick renders.
  const actions = useMemo<BackgroundJobActions>(
    () => ({ markStarting, markFinished, cancel }),
    [markStarting, markFinished, cancel],
  );

  const state = useMemo<BackgroundJobsState>(
    () => ({ jobs: Array.from(jobs.values()), isRunning }),
    [jobs, isRunning],
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
